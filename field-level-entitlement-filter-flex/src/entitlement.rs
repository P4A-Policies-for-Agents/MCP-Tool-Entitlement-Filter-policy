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

/// Apply the projection to one record in place. Returns the sorted names of the
/// fields actually withheld (only fields present in the record are acted on).
pub fn apply(record: &mut Map<String, Value>, projection: &Projection) -> Vec<String> {
    let mut applied = Vec::new();
    for name in &projection.withhold {
        if !record.contains_key(name) {
            continue;
        }
        match projection.mode {
            MaskMode::Mask => {
                record.insert(name.clone(), Value::String(projection.token.clone()));
            }
            MaskMode::Nullify => {
                record.insert(name.clone(), Value::Null);
            }
            MaskMode::Drop => {
                record.remove(name);
            }
        }
        applied.push(name.clone());
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

    #[test]
    fn mask_mode_parse() {
        assert_eq!(MaskMode::parse("mask"), MaskMode::Mask);
        assert_eq!(MaskMode::parse("nullify"), MaskMode::Nullify);
        assert_eq!(MaskMode::parse("drop"), MaskMode::Drop);
        assert_eq!(MaskMode::parse("weird"), MaskMode::Mask);
    }
}
