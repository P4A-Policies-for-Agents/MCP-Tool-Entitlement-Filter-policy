// Copyright 2026 Salesforce, Inc. All rights reserved.
//! Resolve which CDGC schema (and records path) governs a request.
//!
//! An MCP API can expose several tools, each returning a different data product.
//! `toolSchemas` maps a tool name to its schema; this module picks the effective
//! schema for one request from the available sources, in precedence order:
//!
//! 1. `schemaIdClaim` — signed token binding (upstream-validated JWT)
//! 2. `toolSchemas` entry for the called tool — admin-configured
//! 3. `schemaIdHeader` — caller-supplied
//! 4. `schemaId` — the configured default
//!
//! The tool mapping outranks the header so a caller can't redirect a mapped tool
//! to a schema with no sensitive fields. The records path is a property of the
//! tool's result shape, so a mapped tool's `recordsPath` applies whichever source
//! supplied the schema. With no mapping, behavior is identical to single-schema mode.

use serde_json::Value;

/// A `toolSchemas` entry, decoupled from the generated config type.
pub struct ToolMapping<'a> {
    pub tool: &'a str,
    pub schema_id: &'a str,
    pub records_path: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub asset_id: String,
    pub records_path: String,
}

/// The tool name of an MCP `tools/call` JSON-RPC request; `None` for any other method.
pub fn called_tool(rpc: &Value) -> Option<&str> {
    if rpc.get("method").and_then(Value::as_str) != Some("tools/call") {
        return None;
    }
    rpc.get("params")?.get("name")?.as_str().map(str::trim).filter(|s| !s.is_empty())
}

/// The first mapping whose `tool` equals `tool` exactly.
pub fn find_mapping<'a>(mappings: &'a [ToolMapping<'a>], tool: Option<&str>) -> Option<&'a ToolMapping<'a>> {
    let tool = tool?;
    mappings.iter().find(|m| m.tool == tool)
}

pub fn resolve(
    claim: Option<String>,
    mapping: Option<&ToolMapping>,
    header: Option<String>,
    default_schema: &str,
    default_records_path: &str,
) -> Route {
    let non_empty = |s: &str| (!s.trim().is_empty()).then(|| s.to_string());
    let asset_id = claim
        .or_else(|| mapping.and_then(|m| non_empty(m.schema_id)))
        .or(header)
        .unwrap_or_else(|| default_schema.to_string());
    let records_path = mapping
        .and_then(|m| m.records_path)
        .unwrap_or(default_records_path)
        .to_string();
    Route { asset_id, records_path }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn maps() -> Vec<ToolMapping<'static>> {
        vec![
            ToolMapping { tool: "get_product", schema_id: "S-PRODUCT", records_path: Some("products") },
            ToolMapping { tool: "get_customer", schema_id: "S-CUSTOMER", records_path: None },
        ]
    }

    #[test]
    fn called_tool_only_for_tools_call() {
        let call = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_product"}});
        assert_eq!(called_tool(&call), Some("get_product"));
        let read = json!({"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"name":"get_product"}});
        assert_eq!(called_tool(&read), None);
        assert_eq!(called_tool(&json!({"method":"tools/call","params":{}})), None);
        assert_eq!(called_tool(&json!({"method":"tools/call","params":{"name":"  "}})), None);
        assert_eq!(called_tool(&json!({"products":[]})), None);
    }

    #[test]
    fn mapping_is_exact_match() {
        let m = maps();
        assert_eq!(find_mapping(&m, Some("get_customer")).map(|x| x.schema_id), Some("S-CUSTOMER"));
        assert!(find_mapping(&m, Some("Get_Customer")).is_none());
        assert!(find_mapping(&m, Some("unknown")).is_none());
        assert!(find_mapping(&m, None).is_none());
        assert!(find_mapping(&[], Some("get_product")).is_none());
    }

    #[test]
    fn no_mapping_preserves_single_schema_behavior() {
        let r = resolve(None, None, None, "S-DEFAULT", "");
        assert_eq!(r, Route { asset_id: "S-DEFAULT".into(), records_path: "".into() });
        let r = resolve(None, None, Some("S-HDR".into()), "S-DEFAULT", "rows");
        assert_eq!(r, Route { asset_id: "S-HDR".into(), records_path: "rows".into() });
        let r = resolve(Some("S-CLAIM".into()), None, Some("S-HDR".into()), "S-DEFAULT", "");
        assert_eq!(r.asset_id, "S-CLAIM");
    }

    #[test]
    fn mapping_beats_header_and_default() {
        let m = maps();
        let r = resolve(None, find_mapping(&m, Some("get_product")), Some("S-HDR".into()), "S-DEFAULT", "rows");
        assert_eq!(r, Route { asset_id: "S-PRODUCT".into(), records_path: "products".into() });
    }

    #[test]
    fn claim_beats_mapping_but_tool_records_path_applies() {
        let m = maps();
        let r = resolve(Some("S-CLAIM".into()), find_mapping(&m, Some("get_product")), None, "S-DEFAULT", "rows");
        assert_eq!(r, Route { asset_id: "S-CLAIM".into(), records_path: "products".into() });
    }

    #[test]
    fn mapping_without_records_path_uses_global() {
        let m = maps();
        let r = resolve(None, find_mapping(&m, Some("get_customer")), None, "S-DEFAULT", "rows");
        assert_eq!(r, Route { asset_id: "S-CUSTOMER".into(), records_path: "rows".into() });
    }

    #[test]
    fn blank_mapping_schema_falls_through() {
        let blank = [ToolMapping { tool: "t", schema_id: " ", records_path: None }];
        let r = resolve(None, find_mapping(&blank, Some("t")), Some("S-HDR".into()), "S-DEFAULT", "");
        assert_eq!(r.asset_id, "S-HDR");
    }
}
