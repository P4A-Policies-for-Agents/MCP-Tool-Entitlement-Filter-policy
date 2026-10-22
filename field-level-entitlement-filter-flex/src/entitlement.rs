// Copyright 2026 Salesforce, Inc. All rights reserved.
//! Pure field-entitlement core (no PDK imports) — fully unit-testable.
//!
//! A per-field sensitivity map (derived from CDGC — each governed column plus
//! whether its Business Term marks it sensitive) is evaluated against the caller's
//! clearance and declared purpose. Sensitive fields the caller is not entitled to
//! see are withheld from each response record — masked, nulled, or dropped.
//!
//! The decision is deliberately **fail-closed**: a caller whose clearance is
//! absent or not in the cleared set (and, when purposes are enforced, whose purpose
//! is absent or not allowed) is treated as *not entitled*, so every sensitive field
//! is withheld. Non-sensitive fields are always passed through untouched.

use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// One governed field from the catalog. `sensitive` drives whether it is subject to
/// the entitlement check; `term` (the governing Business Term) is provenance only.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct GovernedField {
    pub name: String,
    #[serde(default)]
    pub sensitive: bool,
    #[serde(default)]
    pub term: Option<String>,
}

/// How a withheld sensitive field is rendered in the response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskMode {
    /// Replace the value with a mask token (default).
    Mask,
    /// Set the value to JSON null.
    Nullify,
    /// Remove the field from the record entirely.
    Drop,
}

impl MaskMode {
    pub fn parse(s: &str) -> MaskMode {
        match s.trim().to_lowercase().as_str() {
            "nullify" | "null" => MaskMode::Nullify,
            "drop" | "remove" => MaskMode::Drop,
            _ => MaskMode::Mask,
        }
    }
}

/// The caller's asserted identity for this request (from headers or JWT claims).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CallerContext {
    pub clearance: Option<String>,
    pub purpose: Option<String>,
}

/// The configured entitlement rule: who may see sensitive fields, and how to
/// withhold them from everyone else.
#[derive(Debug, Clone)]
pub struct EntitlementPolicy {
    pub cleared_levels: HashSet<String>,
    /// Empty = purpose is not enforced (clearance alone governs).
    pub allowed_purposes: HashSet<String>,
    pub mask_mode: MaskMode,
    pub mask_token: String,
}

/// Parse a comma-separated config value into a set of lowercased, trimmed tokens.
pub fn parse_csv_set(raw: &str) -> HashSet<String> {
    raw.split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parse a multi-select array config value into a set of lowercased, trimmed
/// tokens. When the property is unset (`None`), fall back to the comma-separated
/// `default_csv`. An explicit empty selection yields an empty set.
pub fn parse_level_set(values: Option<&[String]>, default_csv: &str) -> HashSet<String> {
    match values {
        Some(v) => v
            .iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect(),
        None => parse_csv_set(default_csv),
    }
}

/// Whether one Business Term marks its column sensitive: by its Security Level, or,
/// only when the term has no level, by `marker` appearing in its description.
pub fn term_is_sensitive(level: &str, desc: &str, levels: &HashSet<String>, marker: &str) -> bool {
    let level = level.trim().to_lowercase();
    if level.is_empty() {
        desc.to_lowercase().contains(marker)
    } else {
        levels.contains(&level)
    }
}

/// Classify a column from all its linked terms `(name, level, description)`. The column
/// is sensitive if any term is; the reported term is the first sensitive one, else the
/// first one. Taking only the first link would let a level-less term (e.g. one added by
/// the scanner's Glossary Association) hide a Confidential term on the same column.
pub fn classify_terms<'a>(
    terms: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>,
    levels: &HashSet<String>,
    marker: &str,
) -> (bool, Option<String>) {
    let mut first: Option<&str> = None;
    for (name, level, desc) in terms {
        if term_is_sensitive(level, desc, levels, marker) {
            return (true, Some(name.to_string()));
        }
        first.get_or_insert(name);
    }
    (false, first.map(str::to_string))
}

fn norm(v: &Option<String>) -> Option<String> {
    v.as_deref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty())
}

/// True when the caller is entitled to see sensitive fields: their clearance is in
/// the cleared set AND (if purposes are enforced) their purpose is in the allowed
/// set. Absent/blank claims never satisfy the check — fail-closed.
pub fn is_entitled(caller: &CallerContext, policy: &EntitlementPolicy) -> bool {
    let cleared = match norm(&caller.clearance) {
        Some(c) => policy.cleared_levels.contains(&c),
        None => false,
    };
    if !cleared {
        return false;
    }
    if policy.allowed_purposes.is_empty() {
        return true;
    }
    match norm(&caller.purpose) {
        Some(p) => policy.allowed_purposes.contains(&p),
        None => false,
    }
}

/// The set of sensitive field names to withhold for this caller. Entitled callers
/// withhold nothing; everyone else withholds every sensitive field.
#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    pub withhold: BTreeSet<String>,
    pub mode: MaskMode,
    pub token: String,
    pub entitled: bool,
}

pub fn plan(fields: &[GovernedField], caller: &CallerContext, policy: &EntitlementPolicy) -> Projection {
    let entitled = is_entitled(caller, policy);
    let withhold = if entitled {
        BTreeSet::new()
    } else {
        fields.iter().filter(|f| f.sensitive).map(|f| f.name.clone()).collect()
    };
    Projection { withhold, mode: policy.mask_mode, token: policy.mask_token.clone(), entitled }
}

fn withhold(record: &mut Map<String, Value>, key: &str, projection: &Projection) {
    match projection.mode {
        MaskMode::Mask => {
            record.insert(key.to_string(), Value::String(projection.token.clone()));
        }
        MaskMode::Nullify => {
            record.insert(key.to_string(), Value::Null);
        }
        MaskMode::Drop => {
            record.remove(key);
        }
    }
}

/// Split a nested field name — "header.customerName", "items[].netValue", or the
/// JSONPath form "$.items[*].netValue" — into (key, is-array) segments. `None` for a
/// plain name, which is only ever matched as a top-level key.
fn path_segments(name: &str) -> Option<Vec<(&str, bool)>> {
    let path = name.strip_prefix("$.").unwrap_or(name);
    if !path.contains('.') && !path.contains('[') {
        return None;
    }
    path.split('.')
        .map(|seg| match seg.strip_suffix("[]").or_else(|| seg.strip_suffix("[*]")) {
            Some(key) => (key, true),
            None => (seg, false),
        })
        .map(|(key, arr)| if key.is_empty() { None } else { Some((key, arr)) })
        .collect()
}

/// Withhold the field at `segs` under `record`, fanning out over arrays. A trailing
/// array segment ("tags[]") withholds the whole array. True if any value was withheld.
fn apply_path(record: &mut Map<String, Value>, segs: &[(&str, bool)], projection: &Projection) -> bool {
    let Some(((key, is_array), rest)) = segs.split_first() else { return false };
    if rest.is_empty() {
        if !record.contains_key(*key) {
            return false;
        }
        withhold(record, key, projection);
        return true;
    }
    match record.get_mut(*key) {
        Some(Value::Array(items)) if *is_array => {
            let mut any = false;
            for item in items.iter_mut() {
                if let Some(o) = item.as_object_mut() {
                    any |= apply_path(o, rest, projection);
                }
            }
            any
        }
        Some(Value::Object(o)) if !*is_array => apply_path(o, rest, projection),
        _ => false,
    }
}

/// Apply the projection to one record in place. Returns the sorted names of the
/// fields actually withheld (only fields present in the record are acted on). A name
/// that is a top-level key is withheld as is; otherwise a nested name (MCP Tool Output
/// Fields such as "items[].netValue") is followed into the record.
pub fn apply(record: &mut Map<String, Value>, projection: &Projection) -> Vec<String> {
    let mut applied = Vec::new();
    for name in &projection.withhold {
        let hit = if record.contains_key(name) {
            withhold(record, name, projection);
            true
        } else {
            path_segments(name).is_some_and(|segs| apply_path(record, &segs, projection))
        };
        if hit {
            applied.push(name.clone());
        }
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fields() -> Vec<GovernedField> {
        vec![
            GovernedField { name: "sku".into(), sensitive: false, term: None },
            GovernedField { name: "list_price".into(), sensitive: false, term: None },
            GovernedField { name: "unit_cost".into(), sensitive: true, term: Some("Unit Cost".into()) },
        ]
    }

    fn policy(mode: MaskMode) -> EntitlementPolicy {
        EntitlementPolicy {
            cleared_levels: parse_csv_set("restricted"),
            allowed_purposes: parse_csv_set("fraud-detection"),
            mask_mode: mode,
            mask_token: "***".into(),
        }
    }

    fn rec(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn entitled_caller_sees_everything() {
        let caller = CallerContext { clearance: Some("restricted".into()), purpose: Some("fraud-detection".into()) };
        assert!(is_entitled(&caller, &policy(MaskMode::Mask)));
        let proj = plan(&fields(), &caller, &policy(MaskMode::Mask));
        assert!(proj.entitled);
        assert!(proj.withhold.is_empty());
    }

    #[test]
    fn uncleared_caller_withholds_sensitive() {
        let caller = CallerContext { clearance: Some("internal".into()), purpose: Some("fraud-detection".into()) };
        assert!(!is_entitled(&caller, &policy(MaskMode::Mask)));
        let proj = plan(&fields(), &caller, &policy(MaskMode::Mask));
        assert_eq!(proj.withhold, BTreeSet::from(["unit_cost".to_string()]));
    }

    #[test]
    fn cleared_but_wrong_purpose_withholds() {
        let caller = CallerContext { clearance: Some("restricted".into()), purpose: Some("analytics".into()) };
        assert!(!is_entitled(&caller, &policy(MaskMode::Mask)));
    }

    #[test]
    fn missing_claims_fail_closed() {
        let caller = CallerContext { clearance: None, purpose: None };
        assert!(!is_entitled(&caller, &policy(MaskMode::Mask)));
        let proj = plan(&fields(), &caller, &policy(MaskMode::Mask));
        assert_eq!(proj.withhold, BTreeSet::from(["unit_cost".to_string()]));
    }

    #[test]
    fn purpose_not_enforced_when_empty() {
        let mut p = policy(MaskMode::Mask);
        p.allowed_purposes = parse_csv_set("");
        let caller = CallerContext { clearance: Some("restricted".into()), purpose: None };
        assert!(is_entitled(&caller, &p));
    }

    #[test]
    fn apply_mask_replaces_value() {
        let proj = plan(&fields(), &CallerContext::default(), &policy(MaskMode::Mask));
        let mut r = rec(json!({"sku":"SKU-1","list_price":"9.99","unit_cost":"4.20"}));
        let applied = apply(&mut r, &proj);
        assert_eq!(applied, vec!["unit_cost".to_string()]);
        assert_eq!(r.get("unit_cost"), Some(&Value::String("***".into())));
        assert_eq!(r.get("sku"), Some(&json!("SKU-1"))); // non-sensitive untouched
    }

    #[test]
    fn apply_nullify_sets_null() {
        let proj = plan(&fields(), &CallerContext::default(), &policy(MaskMode::Nullify));
        let mut r = rec(json!({"unit_cost":"4.20"}));
        apply(&mut r, &proj);
        assert_eq!(r.get("unit_cost"), Some(&Value::Null));
    }

    #[test]
    fn apply_drop_removes_field() {
        let proj = plan(&fields(), &CallerContext::default(), &policy(MaskMode::Drop));
        let mut r = rec(json!({"sku":"SKU-1","unit_cost":"4.20"}));
        apply(&mut r, &proj);
        assert!(!r.contains_key("unit_cost"));
        assert!(r.contains_key("sku"));
    }

    #[test]
    fn apply_ignores_absent_sensitive_field() {
        let proj = plan(&fields(), &CallerContext::default(), &policy(MaskMode::Mask));
        let mut r = rec(json!({"sku":"SKU-1"})); // unit_cost not present
        let applied = apply(&mut r, &proj);
        assert!(applied.is_empty());
    }

    fn nested_proj(mode: MaskMode, names: &[&str]) -> Projection {
        Projection {
            withhold: names.iter().map(|s| s.to_string()).collect(),
            mode,
            token: "***".into(),
            entitled: false,
        }
    }

    #[test]
    fn apply_nested_output_fields() {
        let proj = nested_proj(MaskMode::Mask, &["header.customerName", "items[].netValue", "credit.missing"]);
        let mut r = rec(json!({
            "header": {"salesOrderId": "SO-1", "customerName": "ACME"},
            "items": [{"material": "M-1", "netValue": 10}, {"material": "M-2", "netValue": 20}, "x"],
        }));
        let applied = apply(&mut r, &proj);
        assert_eq!(applied, vec!["header.customerName".to_string(), "items[].netValue".to_string()]);
        assert_eq!(r["header"], json!({"salesOrderId": "SO-1", "customerName": "***"}));
        assert_eq!(r["items"], json!([{"material": "M-1", "netValue": "***"}, {"material": "M-2", "netValue": "***"}, "x"]));
    }

    #[test]
    fn apply_nested_drop_and_jsonpath_form() {
        let proj = nested_proj(MaskMode::Drop, &["$.items[*].netValue", "tags[]"]);
        let mut r = rec(json!({"items": [{"netValue": 1, "uom": "EA"}], "tags": ["a"]}));
        apply(&mut r, &proj);
        assert_eq!(r, rec(json!({"items": [{"uom": "EA"}]})));
    }

    #[test]
    fn apply_literal_key_wins_and_shape_mismatch_is_ignored() {
        let proj = nested_proj(MaskMode::Mask, &["a.b", "items[].x", "obj.y"]);
        // "a.b" is a literal key (e.g. a flat-file column); "items" is not an array and
        // "obj" is an array, so the paths don't match.
        let mut r = rec(json!({"a.b": 1, "a": {"b": 2}, "items": {"x": 3}, "obj": [{"y": 4}]}));
        let applied = apply(&mut r, &proj);
        assert_eq!(applied, vec!["a.b".to_string()]);
        assert_eq!(r, rec(json!({"a.b": "***", "a": {"b": 2}, "items": {"x": 3}, "obj": [{"y": 4}]})));
    }

    #[test]
    fn path_segments_parse() {
        assert_eq!(path_segments("sku"), None);
        assert_eq!(path_segments("items[].netValue"), Some(vec![("items", true), ("netValue", false)]));
        assert_eq!(path_segments("$.header.customerName"), Some(vec![("header", false), ("customerName", false)]));
        assert_eq!(path_segments("a..b"), None);
    }

    #[test]
    fn mask_mode_parse() {
        assert_eq!(MaskMode::parse("mask"), MaskMode::Mask);
        assert_eq!(MaskMode::parse("nullify"), MaskMode::Nullify);
        assert_eq!(MaskMode::parse("drop"), MaskMode::Drop);
        assert_eq!(MaskMode::parse("weird"), MaskMode::Mask);
    }

    #[test]
    fn classify_any_sensitive_term_wins() {
        let lv = parse_csv_set("confidential,restricted");
        let terms = [("Unit Cost", "", "landed cost"), ("Stock Unit Cost", "Confidential", "")];
        assert_eq!(classify_terms(terms, &lv, "confidential"), (true, Some("Stock Unit Cost".into())));
    }

    #[test]
    fn classify_level_beats_description_marker() {
        let lv = parse_csv_set("confidential,restricted");
        // A level is set, so the description marker is ignored.
        let terms = [("SKU", "Internal", "not confidential at all")];
        assert_eq!(classify_terms(terms, &lv, "confidential"), (false, Some("SKU".into())));
        // No level: fall back to the marker.
        let terms = [("Unit Cost", " ", "Confidential: supplier terms")];
        assert!(classify_terms(terms, &lv, "confidential").0);
        assert_eq!(classify_terms(std::iter::empty(), &lv, "confidential"), (false, None));
    }
}
