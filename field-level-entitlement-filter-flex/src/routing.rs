// Copyright 2026 Salesforce, Inc. All rights reserved.
//! Resolve which CDGC schema (and records path) governs an MCP tool call.
//!
//! `toolSchemas` is the only schema source: each MCP tool maps to the schema of the
//! data product it returns. A `tools/call` whose tool has no entry, and any other
//! JSON-RPC method, resolves to no route and its response passes through unfiltered.

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

/// The route for `tool`: the first mapping whose `tool` equals it exactly and whose
/// `schemaId` is non-blank. The mapping's `recordsPath` overrides the global one.
pub fn resolve(mappings: &[ToolMapping], tool: Option<&str>, default_records_path: &str) -> Option<Route> {
    let tool = tool?;
    let m = mappings.iter().find(|m| m.tool == tool)?;
    let asset_id = m.schema_id.trim();
    if asset_id.is_empty() {
        return None;
    }
    Some(Route {
        asset_id: asset_id.to_string(),
        records_path: m.records_path.unwrap_or(default_records_path).to_string(),
    })
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
    fn mapped_tool_gets_its_schema_and_records_path() {
        let r = resolve(&maps(), Some("get_product"), "rows");
        assert_eq!(r, Some(Route { asset_id: "S-PRODUCT".into(), records_path: "products".into() }));
    }

    #[test]
    fn mapping_without_records_path_uses_global() {
        let r = resolve(&maps(), Some("get_customer"), "rows");
        assert_eq!(r, Some(Route { asset_id: "S-CUSTOMER".into(), records_path: "rows".into() }));
    }

    #[test]
    fn unmapped_or_missing_tool_has_no_route() {
        let m = maps();
        assert_eq!(resolve(&m, Some("Get_Product"), ""), None);
        assert_eq!(resolve(&m, Some("unknown"), ""), None);
        assert_eq!(resolve(&m, None, ""), None);
        assert_eq!(resolve(&[], Some("get_product"), ""), None);
    }

    #[test]
    fn blank_schema_id_has_no_route() {
        let blank = [ToolMapping { tool: "t", schema_id: " ", records_path: None }];
        assert_eq!(resolve(&blank, Some("t"), ""), None);
    }
}
