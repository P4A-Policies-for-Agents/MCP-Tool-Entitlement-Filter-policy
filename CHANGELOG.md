# Changelog

## 2.3.0

- The default `fieldClassTypes` MCP field class is now the generic
  `protocol.mcp.McpToolField`. If your CDGC custom model uses another package,
  list its field class in `fieldClassTypes` explicitly, otherwise MCP Tool assets
  resolve to no fields and responses pass through unfiltered.
- README restructured; release history moved here; p4a.ai submission kit added
  (`P4A.md`).

## 2.2.0

- Adds `fieldClassTypes`, so a `schemaId` can be an MCP Tool asset of a custom MCP
  Server catalog source. Before, only `FlatField` columns were read, so an MCP Tool
  id found no fields and the response passed through unfiltered. Existing configs
  keep working.

## 2.1.0

- Checks every Business Term linked to a column, not just the first. The column is
  sensitive if any of its terms is, so a level-less term added by the scanner's
  Glossary Association can no longer hide a Confidential term. Config unchanged.

## 2.0.0 (breaking)

- `toolSchemas` is the only schema source and is required. `schemaId`,
  `schemaIdHeader` and `schemaIdClaim` are removed.
- Unmapped tools pass through unfiltered.
- Applies to MCP APIs only (`assetTypes: mcp`).
- Upgrade a 1.x config by moving `schemaId` into a `toolSchemas` entry per tool and
  deleting the three removed properties.

## 1.3.1

- Display name is now "MCP Tool Response Field Entitlement Filter". Asset ids
  (`field-level-entitlement-filter` / `-flex`) and behavior are unchanged.

## 1.3.0

- Adds the optional `toolSchemas` per-tool schema mapping, backward compatible with
  1.2.0. A header-supplied schema with no field map falls back to `schemaId`
  instead of passing the response through.

## 1.2.0

- `sensitiveLevels` and `clearedLevels` become multi-select dropdowns (`type: array`
  of `[public, internal, confidential, restricted]`) instead of comma-separated
  strings.

## 1.1.0

- Opt-in JWT-claims sourcing for clearance, purpose and schema id. Header mode
  remains the default.
