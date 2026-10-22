// Copyright 2026 Salesforce, Inc. All rights reserved.
//! Pure CDGC helpers (no PDK imports) — the JWT nonce, the field class types and
//! the cached field-sensitivity types. The HTTP fetch lives in lib.rs (needs the HttpClient).

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::entitlement::GovernedField;

/// Cached, parsed per-field sensitivity map for one asset, plus its governed
/// identity (so the policy can self-describe the response — one CDGC fetch does both).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CachedFieldMap {
    pub fields: Vec<GovernedField>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub external_id: Option<String>,
    /// Unix seconds when fetched — drives the refresh TTL.
    pub timestamp: i64,
}

/// Single-initiator refresh lock entry (stampede control).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RefreshLock {
    pub acquired_at: i64,
}

/// Per-request JWT nonce: nanoseconds since the Unix epoch as a decimal string.
pub fn nonce_from_time(now: SystemTime) -> String {
    now.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

/// Field class types used when `fieldClassTypes` is unset: flat-file columns and the
/// fields of an MCP Tool from the `protocol.mcp` custom metadata model.
pub const DEFAULT_FIELD_CLASS_TYPES: &[&str] = &[
    "com.infa.odin.models.file.flat.FlatField",
    "protocol.mcp.McpToolField",
];

/// The `core.classType` values a schema's fields may have. Blank entries are ignored;
/// unset or all-blank falls back to `DEFAULT_FIELD_CLASS_TYPES`.
pub fn field_class_types(values: Option<&[String]>) -> Vec<String> {
    let v: Vec<String> = values.unwrap_or_default().iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if v.is_empty() {
        DEFAULT_FIELD_CLASS_TYPES.iter().map(|s| s.to_string()).collect()
    } else {
        v
    }
}

/// The MCP protocol field classes an `mcpClassMapping` entry can map to a CDGC class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpFieldClass {
    /// A field of the tool's response records (single-field-class models).
    Field,
    /// A property of the tool's output; may be a nested path.
    Output,
    /// A tool argument; never a response field.
    Input,
}

impl McpFieldClass {
    pub fn parse(s: &str) -> Option<McpFieldClass> {
        match s.trim() {
            "McpToolField" => Some(McpFieldClass::Field),
            "McpToolOutputField" => Some(McpFieldClass::Output),
            "McpToolInputField" => Some(McpFieldClass::Input),
            _ => None,
        }
    }
}

/// `mcpClassMapping` used when it is unset: the A2D agentic model's output/input fields.
pub const DEFAULT_MCP_CLASS_MAPPING: &[(McpFieldClass, &str)] = &[
    (McpFieldClass::Output, "a2d.agentic.McpToolOutputField"),
    (McpFieldClass::Input, "a2d.agentic.McpToolInputField"),
];

/// The CDGC class types of a schema's response fields: `fieldClassTypes` (read as MCP
/// Tool Fields) plus every `mcpClassMapping` Field/Output class, minus any class mapped
/// as an Input field. Unset mapping falls back to `DEFAULT_MCP_CLASS_MAPPING`; entries
/// with an unknown MCP class or a blank class type are ignored.
pub fn response_field_classes(field_types: Option<&[String]>, mapping: Option<&[(String, String)]>) -> Vec<String> {
    let mapping: Vec<(McpFieldClass, String)> = match mapping {
        Some(m) => m.iter()
            .filter_map(|(mcp, idmc)| Some((McpFieldClass::parse(mcp)?, idmc.trim().to_string())))
            .filter(|(_, idmc)| !idmc.is_empty())
            .collect(),
        None => DEFAULT_MCP_CLASS_MAPPING.iter().map(|(c, t)| (*c, t.to_string())).collect(),
    };
    let input: Vec<&String> = mapping.iter().filter(|(c, _)| *c == McpFieldClass::Input).map(|(_, t)| t).collect();
    let mut out: Vec<String> = Vec::new();
    let candidates = field_class_types(field_types).into_iter()
        .chain(mapping.iter().filter(|(c, _)| *c != McpFieldClass::Input).map(|(_, t)| t.clone()));
    for t in candidates {
        if !input.contains(&&t) && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }
    #[test]
    fn field_classes_default_to_flatfield_and_mcp_tool_field() {
        let d = field_class_types(None);
        assert_eq!(d, DEFAULT_FIELD_CLASS_TYPES);
        assert_eq!(field_class_types(Some(&[" ".to_string()])), d);
    }
    #[test]
    fn field_classes_configured_replace_default() {
        let v = field_class_types(Some(&[" acme.Field ".to_string(), "".to_string()]));
        assert_eq!(v, vec!["acme.Field".to_string()]);
    }
    #[test]
    fn response_classes_default_add_a2d_output_not_input() {
        let v = response_field_classes(None, None);
        assert_eq!(v, vec![
            "com.infa.odin.models.file.flat.FlatField".to_string(),
            "protocol.mcp.McpToolField".to_string(),
            "a2d.agentic.McpToolOutputField".to_string(),
        ]);
    }
    #[test]
    fn response_classes_legacy_field_class_types_still_work() {
        let legacy = ["acme.legacy.McpToolField".to_string()];
        let v = response_field_classes(Some(&legacy), Some(&[]));
        assert_eq!(v, vec!["acme.legacy.McpToolField".to_string()]);
    }
    #[test]
    fn response_classes_mapping_combines_and_input_is_excluded() {
        let legacy = ["acme.Column".to_string()];
        let m = pairs(&[
            ("McpToolField", "acme.legacy.McpToolField"),
            ("McpToolOutputField", " acme.mcp.Out "),
            ("McpToolInputField", "acme.mcp.In"),
            ("McpToolInputField", "acme.Column"), // explicitly an input: wins over fieldClassTypes
            ("Unknown", "acme.x"),
            ("McpToolOutputField", " "),
        ]);
        let v = response_field_classes(Some(&legacy), Some(&m));
        assert_eq!(v, vec!["acme.legacy.McpToolField".to_string(), "acme.mcp.Out".to_string()]);
    }
    #[test]
    fn nonce_is_decimal() {
        let n = nonce_from_time(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1));
        assert_eq!(n, "1000000000");
    }
}
