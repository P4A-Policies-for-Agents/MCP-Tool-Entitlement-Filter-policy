# p4a.ai submission kit

Everything needed to submit this policy on [p4a.ai](https://www.p4a.ai) and fill
its catalog page. Keep it in step with `gcl.yaml` and the README when the policy
changes.

## Submission metadata

| Field | Value |
|---|---|
| `name` | `MCP Tool Response Field Entitlement Filter` |
| `projectType` | `split` |
| `githubUrl` | `https://github.com/P4A-Policies-for-Agents/MCP-Tool-Entitlement-Filter-policy/tree/main/field-level-entitlement-filter-definition` |
| `implementationGithubUrl` | `https://github.com/P4A-Policies-for-Agents/MCP-Tool-Entitlement-Filter-policy/tree/main/field-level-entitlement-filter-flex` |
| `category` | `Security` (alternative: `MCP`) |
| `examplesUrl` | `https://github.com/P4A-Policies-for-Agents/MCP-Tool-Entitlement-Filter-policy/blob/main/demo/WALKTHROUGH.md` |
| `description` | the "Description" section below, verbatim |

## Pre-submit checklist

- [x] Repo is public on GitHub (`P4A-Policies-for-Agents/MCP-Tool-Entitlement-Filter-policy`).
- [x] Split layout: definition (`gcl.yaml`, `exchange.json`, `Makefile`) and
      implementation (`Cargo.toml`, `Makefile`, `src/lib.rs`).
- [x] `pdk` 1.10 (≥ 1.8), table-form `definition_asset_id`, `cargo-anypoint` 1.9.
- [x] Every `gcl.yaml` property has a description; credentials are `security:sensitive`.
- [x] `metadata.labels.description` is under 256 characters (Exchange limit).
- [x] No real credentials committed (`demo/config.json`, `demo/env.local.sh` are gitignored).
- [ ] Optional: add `icon.svg` / `icon.png` at the repo root for a catalog icon.
- [ ] Fill the See Also "Related P4A policies" list from `search_policies`
      (approved and published only) once the P4A MCP tools are connected.

---

## Description

**MCP Tool Response Field Entitlement Filter** masks, nulls or drops sensitive
fields in MCP `tools/call` results on the Omni/Flex Gateway, based on the caller's
clearance and declared purpose, with field sensitivity derived live from
Informatica CDGC Business Terms.

> **Key caveat:** clearance and purpose are read from request headers by default,
> and any client can set those. For a real control boundary, put a JWT Validation
> policy in front and set `clearanceClaim` / `purposeClaim`. The entitlement
> decision is fail-closed; the policy itself is fail-open on a CDGC outage.

### How it works

1. Each MCP tool is mapped to the CDGC asset that describes its result (a scanned
   file or table, or an MCP Tool asset from a custom catalog source).
2. On a cache miss the policy reads that asset's fields and their linked Business
   Terms from CDGC. A field is sensitive when a linked term's Security Level is in
   `sensitiveLevels`.
3. On the response leg, every sensitive field in the records at `recordsPath` is
   withheld for callers who aren't entitled. Nothing is configured per field.

The response carries an `x-entitlement-filtered: <n>` header and an `_entitlement`
annotation listing the withheld fields.

### Configuration summary

| Property | Default | Purpose |
|---|---|---|
| `cdgcLoginUrl`, `cdgcSearchUrl` | required | IDMC login and CDGC search hosts |
| `cdgcOrgUsername`, `cdgcOrgPassword` | required | read-only IDMC service account (sensitive) |
| `toolSchemas` | required | `[{tool, schemaId, recordsPath?}]`; unmapped tools pass through |
| `recordsPath` | `""` | default path to the records in the tool result |
| `sensitiveLevels` | `[confidential, restricted]` | Security Levels that make a field sensitive |
| `clearedLevels` | `[restricted]` | caller clearances entitled to see sensitive fields |
| `allowedPurposes` | `""` (not checked) | comma-separated purposes also required |
| `clearanceHeader` / `purposeHeader` | `x-dp-clearance` / `x-dp-purpose` | where caller claims are read |
| `clearanceClaim` / `purposeClaim` | unset | read claims from the Bearer JWT instead |
| `maskMode` / `maskToken` | `mask` / `***` | `mask`, `nullify` or `drop` |
| `refreshIntervalSeconds` | `86400` | field-map cache TTL |

### Example

Mask confidential and restricted product and customer fields for everyone except
fraud investigators with `restricted` clearance:

```json
{
  "cdgcLoginUrl": "https://dm-us.informaticacloud.com",
  "cdgcSearchUrl": "https://cdgc-api.dm-us.informaticacloud.com",
  "cdgcOrgUsername": "<idmc-user>",
  "cdgcOrgPassword": "<idmc-password>",
  "recordsPath": "products",
  "toolSchemas": [
    { "tool": "get_products",  "schemaId": "<dim_product asset id>" },
    { "tool": "get_customers", "schemaId": "<dim_customer asset id>", "recordsPath": "customers" }
  ],
  "clearedLevels": ["restricted"],
  "allowedPurposes": "fraud-detection",
  "maskMode": "mask"
}
```

### Positioning

Apply it to an MCP API instance. It runs after authentication: pair it with JWT
Validation (and `clearanceClaim` / `purposeClaim`) so claims can't be spoofed. It
only touches `tools/call` results of mapped tools, so it composes with rate-limit,
tool-allowlist and prompt-guard policies without ordering conflicts.

---

## Documentation tabs

Write these with `set_policy_doc_tab`, always passing the full `bodyMd`.

### `overview` — Overview (sortOrder 0)

Same body as the Description above, from the lead sentence through Positioning,
without the configuration table.

### `configuration` — Configuration (sortOrder 1)

```markdown
## Properties

| Property | Type | Default | Description |
|---|---|---|---|
| `cdgcLoginUrl` | string (service) | required | IDMC login base URL. |
| `cdgcSearchUrl` | string (service) | required | CDGC search host (serves `ccgf-searchv2`). |
| `cdgcOrgUsername` / `cdgcOrgPassword` | string (sensitive) | required | IDMC read-only service account. |
| `toolSchemas` | array | required | Maps each MCP tool to the CDGC asset governing its results. |
| `fieldClassTypes` | array of string | `[com.infa.odin.models.file.flat.FlatField, protocol.mcp.McpToolField]` | `core.classType` values of an asset's fields, read as MCP Tool Fields. Add your custom model's field class. |
| `mcpClassMapping` | array | A2D agentic Output / Input Field | Maps the MCP classes `McpToolField`, `McpToolOutputField`, `McpToolInputField` to your model's class types. Input fields are never filtered. |
| `recordsPath` | string | `""` | Default `/`-path to the record(s) projected; arrays project each element. |
| `sensitiveLevels` | array | `[confidential, restricted]` | Business Term Security Levels that make a field sensitive. |
| `sensitiveMarker` | string | `confidential` | Fallback for terms with no Security Level: substring in the term description. |
| `clearanceHeader` | string | `x-dp-clearance` | Request header carrying the caller's clearance. |
| `clearanceClaim` | string | unset | JWT claim used instead of the header when set and present. |
| `purposeHeader` | string | `x-dp-purpose` | Request header carrying the caller's purpose. |
| `purposeClaim` | string | unset | JWT claim used instead of the header when set and present. |
| `clearedLevels` | array | `[restricted]` | Clearances entitled to see sensitive fields. |
| `allowedPurposes` | string | `""` | Comma-separated purposes also required. Empty means not checked. |
| `maskMode` | enum | `mask` | `mask`, `nullify` or `drop`. |
| `maskToken` | string | `***` | Replacement value in `mask` mode. |
| `refreshIntervalSeconds` | integer | `86400` | Field-map cache TTL. |
| `failOpenOnCdgcError` | boolean | `true` | Keep the last good map on CDGC errors; with no map, pass through. |
| `distributed` | boolean | `false` | Share the cache and refresh lock across replicas. |
| `timeout` | integer (ms) | `5000` | Per-CDGC-call timeout. |

## `toolSchemas[]`

| Field | Required | Description |
|---|---|---|
| `tool` | yes | Exact MCP tool name (`params.name`), case-sensitive. |
| `schemaId` | yes | CDGC asset id (`core.identity`) of the scanned file/table or MCP Tool. |
| `recordsPath` | no | Overrides the global `recordsPath` for this tool. |

## Failure behavior

- Missing or insufficient caller claims: every sensitive field is withheld.
- Unmapped tools and non-`tools/call` methods: passed through unfiltered.
- CDGC unreachable: with `failOpenOnCdgcError: true` the cached map keeps
  enforcing; with no cached map (or with `false`) the response passes through.
```

### `examples` — Examples (sortOrder 2)

```markdown
### Mask sensitive fields for everyone below `restricted`

    { "toolSchemas": [{ "tool": "get_products", "schemaId": "<asset id>", "recordsPath": "products" }],
      "clearedLevels": ["restricted"], "maskMode": "mask" }

### Drop fields unless the caller also declares an allowed purpose

    { "toolSchemas": [{ "tool": "get_customers", "schemaId": "<asset id>", "recordsPath": "customers" }],
      "clearedLevels": ["confidential", "restricted"], "allowedPurposes": "fraud-detection,audit",
      "maskMode": "drop" }

### Read clearance and purpose from a validated JWT

    { "toolSchemas": [{ "tool": "get_products", "schemaId": "<asset id>", "recordsPath": "products" }],
      "clearanceClaim": "clearance", "purposeClaim": "purpose" }

### Govern tools catalogued as MCP Tool assets with your own custom model

    { "fieldClassTypes": ["com.infa.odin.models.file.flat.FlatField", "acme.mcp.McpToolField"],
      "toolSchemas": [{ "tool": "search_orders", "schemaId": "<MCP Tool asset id>", "recordsPath": "orders" }] }

### Map a model that splits MCP Tool Input and Output Fields

    { "mcpClassMapping": [
        { "mcpClass": "McpToolOutputField", "idmcClass": "acme.mcp.ToolOutputField" },
        { "mcpClass": "McpToolInputField",  "idmcClass": "acme.mcp.ToolInputField" }],
      "toolSchemas": [{ "tool": "get_sales_order", "schemaId": "<MCP Tool asset id>" }] }

A two-persona walkthrough (analyst vs fraud investigator) is in
[demo/WALKTHROUGH.md](https://github.com/P4A-Policies-for-Agents/MCP-Tool-Entitlement-Filter-policy/blob/main/demo/WALKTHROUGH.md).
```

### `security` — Security (sortOrder 3)

```markdown
- **Trust model.** Header claims are caller-controlled. Use `clearanceClaim` /
  `purposeClaim` with a JWT Validation policy on the same instance; the policy only
  decodes the token and relies on that upstream policy for signature and expiry.
- **Fail-closed entitlement, fail-open dependency.** A caller without matching
  claims never sees sensitive fields. A CDGC outage with no cached map passes
  responses through unfiltered. Keep `failOpenOnCdgcError` at `true`: `false`
  discards the cached map on a failed refresh, which also passes through.
- **Scope.** Only records at `recordsPath` are projected. Sensitive values elsewhere
  (for example in error bodies) are not filtered. Unmapped tools are not filtered.
- **Secrets.** CDGC credentials are `security:sensitive`; CDGC response bodies are
  never logged.
```

### `faq` — FAQ & See Also (sortOrder 4)

```markdown
**Where does sensitivity come from?** From the Business Terms linked to each field
in CDGC. Change a term's Security Level and the policy picks it up after
`refreshIntervalSeconds`.

**Do I configure fields one by one?** No. You map tools to CDGC assets; the field
list and which fields are sensitive come from the catalog.

**What happens to a tool I didn't map?** Its results pass through unfiltered. Map
every tool that returns governed data.

**Can I use it on REST APIs?** No. Since 2.0.0 it applies to MCP APIs only, because
the mapping key is the MCP tool name.

**My tools are catalogued with a custom model. Will it find their fields?** Yes, if
its field classes are mapped. The defaults cover `protocol.mcp.McpToolField` and
the A2D agentic `McpToolOutputField`; otherwise add your class to `fieldClassTypes`
or map it in `mcpClassMapping` (`McpToolField`, `McpToolOutputField` or
`McpToolInputField`). Input fields are never treated as response fields, and nested
output names like `items[].netValue` are followed into the result.

**Does it handle streaming responses?** JSON and single-message SSE results only.

## See Also

- [Model Context Protocol specification](https://modelcontextprotocol.io/specification)
- [JWT Validation policy](https://docs.mulesoft.com/gateway/latest/policies-included-jwt-validation): validate the token before using `clearanceClaim` / `purposeClaim`.
- [Flex Gateway Policy Development Kit](https://docs.mulesoft.com/pdk/latest/)
- Related P4A policies: to be filled from `search_policies` (approved and published only).
```
