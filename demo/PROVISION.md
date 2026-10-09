# Demo provisioning runbook (catalog-driven, caller-aware)

Stands up the live MCP Tool Response Field Entitlement Filter demo with `anypoint-cli-v4` + the
A2D MCP tools. Per-field sensitivity is derived from CDGC, so you need a real IDMC
tenant with a **scanned schema asset** whose columns are linked to Business Terms
(one of which is marked with the `sensitiveMarker`, e.g. "Confidential").

## Things this build wires up (fill each with your own tenant's values)

| Thing | Value |
|---|---|
| Anypoint org / env | `<orgId>` / Sandbox `<envId>` |
| A2D mock MCP server | `<mockServerId>` |
| Mock URL | `https://www.a2d-ai.com/api/platform/<mockServerId>/mcp` |
| Exchange asset | `product-catalog-entitlement/1.0.0` (type `mcp`) |
| API Manager instance | `<apiInstanceId>` (label `entitlement-filter-demo`) |
| Flex gateway target | `<gatewayId>` (a gateway with a public URL, e.g. v1.13.5) |
| Applied policy id | `<policyId>` (`field-level-entitlement-filter`) |
| Schema asset for `get_products` | `<productSchemaId>` (`dim_product.csv`) |
| Schema asset for `get_customers` | `<customerSchemaId>` (`dim_customer.csv`) |
| Governed endpoint | `https://<gatewayPublicHost>/entitlement-filter-demo/mcp` |

## 0. Find the schema-asset id (CDGC search API)

```bash
curl -s -X POST "https://cdgc-api.<pod>.informaticacloud.com/ccgf-searchv2/api/v1/search" \
  -H "Authorization: Bearer <jwt>" -H "X-INFA-ORG-ID: <orgId>" \
  -H "X-INFA-SEARCH-LANGUAGE: elasticsearch" -H "Content-Type: application/json" \
  -d '{"from":0,"size":25,"query":{"bool":{"must":[{"terms":{"core.classType":["com.infa.odin.models.file.flat.FlatFile"]}}]}}}'
```
`core.identity` of `dim_product.csv` → `<productSchemaId>`. (Get `<jwt>` via
`/identity-service/api/v1/Login` then `/jwt/Token` — the same chain the policy uses.)

`get_customers` needs a **second** scanned schema asset, e.g. `dim_customer.csv`,
with `credit_limit` linked to a Confidential Business Term. Its `core.identity` →
`<customerSchemaId>`.

## 1. A2D mock (returns the SAME full record for everyone)

`design_mcp_server` (type `mock`) + `add_mcp_tool get_products` with one scenario
(`condition: catalog === "products"`) returning all governed columns, **including
`unit_cost`** (the field whose Business Term is marked Confidential). The mock does
NOT vary by caller — the policy does.

Then `add_mcp_tool get_customers` with one scenario (`condition: segment === "retail"`)
returning the `dim_customer.csv` columns under `customers`, **including
`credit_limit`**:

```json
{ "customers": [ {
  "customer_id":"C-2001","customer_name":"Jane Doe","segment":"retail",
  "country":"CH","credit_limit":"15000.00" } ], "count": 1 }
```

The field names must match the column names in `dim_customer.csv`.

## 2. Publish + deploy the MCP Flex instance

```bash
anypoint-cli-v4 exchange:asset:upload --name "Product Catalog Entitlement Demo" \
  --type mcp --status published --properties='{"platform":"a2d"}' \
  --files='{"mcp-metadata.json":"./mcp-metadata.json"}' product-catalog-entitlement/1.0.0

anypoint-cli-v4 api-mgr:api:manage product-catalog-entitlement 1.0.0 \
  --environment Sandbox --isFlex --type mcp --withProxy \
  --scheme http --port 8081 --path "/entitlement-filter-demo/" \
  --uri "https://www.a2d-ai.com/api/platform/<mockServerId>/" \
  --apiInstanceLabel entitlement-filter-demo

anypoint-cli-v4 api-mgr:api:deploy <apiInstanceId> --environment Sandbox \
  --target <gatewayId> --gatewayVersion 1.13.5 --overwrite
```

## 3. Apply the filter (config = tool mapping + creds + entitlement rule)

```bash
cp config.json.example config.json   # fill cdgc creds/urls + toolSchemas + clearedLevels/allowedPurposes
anypoint-cli-v4 api-mgr:policy:apply <apiInstanceId> field-level-entitlement-filter \
  --environment Sandbox --groupId <orgId> --policyVersion 2.0.0 --configFile ./config.json
anypoint-cli-v4 api-mgr:api:redeploy <apiInstanceId> --environment Sandbox
```

`toolSchemas` maps `get_products` → `<productSchemaId>` and `get_customers` →
`<customerSchemaId>` with `recordsPath: customers`. A tool without an entry passes
through unfiltered. Policy 2.0.0 has no `schemaId`, so a 1.x config won't validate.
If 1.x is already applied to the instance, remove it and re-apply at 2.0.0.

## 4. Run

```bash
cp env.local.sh.example env.local.sh   # set CMP_GW_URL
./demo.sh
```

Expected, for each tool: analyst (`internal`/`analytics`) → `unit_cost` /
`credit_limit` masked, `x-entitlement-filtered: 1`; fraud investigator
(`restricted`/`fraud-detection`) → visible, `x-entitlement-filtered: 0`. Each tool's
`_entitlement.assetId` is its own mapped schema.

Quick manual check of both personas + the header:

```bash
GW="https://<gatewayPublicHost>/entitlement-filter-demo/mcp"
BODY='{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"get_products","arguments":{"catalog":"products"}}}'
curl -sS -i -X POST "$GW" -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" -H "mcp-session-id: e-demo" \
  -H "x-dp-clearance: internal" -H "x-dp-purpose: analytics" -d "$BODY" | grep -iE "x-entitlement-filtered|unit_cost"
```

## Notes
- Egress: the policy calls `cdgcLoginUrl` + `cdgcSearchUrl` (both `format:service`);
  confirm the gateway can reach `*.informaticacloud.com`.
- `recordsPath=products` because records live under `products` in the payload.
- Sensitivity comes from a field's Business Term description containing the
  `sensitiveMarker` (default "confidential"); the demo relies on `unit_cost`'s term.
- In production the clearance/purpose headers would be set by an upstream identity
  policy (or a JWT-to-header mapping), not by the caller directly.
