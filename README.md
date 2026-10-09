# MCP Tool Response Field Entitlement Filter — MuleSoft Omni/Flex Gateway Policy

An **inbound, body-inspecting** custom policy for the MuleSoft Omni/Flex Gateway
that **derives per-field sensitivity live from Informatica CDGC** and then, on the
response leg, **projects each field for the caller** — masking, nulling, or dropping
the sensitive fields the caller's **clearance** and **declared purpose** don't
entitle them to see.

The upstream returns the *same* record to everyone. What each caller receives is
decided at the gateway from *who they are*: a cleared fraud investigator sees
`unit_cost`; an analyst gets it masked. Nothing is configured per field — the field
set and which fields are sensitive come from CDGC.

Built with the PDK, Rust → `wasm32-wasip1`, split-model. Applies to **MCP APIs
only** (`assetTypes: mcp`). Each MCP tool is mapped to the CDGC schema of the data
product it returns (`toolSchemas`), and the policy filters that tool's `tools/call`
results. REST/HTTP APIs were dropped in 2.0.0: they have no tool name to map.

---

## How sensitivity is derived (catalog-driven)

Each `toolSchemas` entry names a **CDGC asset id** for a scanned schema (a flat
file, table, etc.) or an **MCP Tool** asset from a custom MCP Server catalog source. On a cache miss the policy authenticates to IDMC
(**Login → JWT**) and then, via the CDGC search API
**`POST cdgc-api…/ccgf-searchv2/api/v1/search`** (Elasticsearch DSL,
`X-INFA-SEARCH-LANGUAGE: elasticsearch`):

1. **Resolve the schema asset** (`core.identity = toolSchemas[].schemaId`) → its `core.location`, name, external id.
2. **Enumerate its columns** — assets of a `fieldClassTypes` class (`FlatField`, `worldtour.mcp.McpToolField` by default) under the schema location → field **names**.
3. **Enumerate column → Business Term links** — `elementType=RELATIONSHIP`, `type=IClassTechnicalGlossaryBase`.
4. **Resolve the linked terms** → `core.name`, `core.description` and the
   Security Level (`com.infa.ccgf.models.governance.securityClassification`).
5. **Build the field map**: one entry per column, flagged **sensitive** when its
   Business Term's Security Level is in `sensitiveLevels` (default
   `confidential, restricted`, case-insensitive). Only when a term has **no**
   Security Level does the policy fall back to the description containing
   `sensitiveMarker` (default *"confidential"*). Columns with no linked term are
   non-sensitive.

The map is cached in PDK DataStorage (lazy refresh, single-flight, `distributed`
for cross-replica). Credentials are `security:sensitive`; CDGC response bodies are
never logged. This mirrors the CDGC governance graph: **catalog source → scanned
table → columns → Business Terms**.

---

## Mapping MCP tools to schemas

One MCP server often exposes tools over **different** data products (e.g.
`get_products` over `dim_product.csv`, `get_customers` over `dim_customer.csv`).
The required `toolSchemas` list maps each tool to its own CDGC schema, and
optionally to its own `recordsPath`:

```json
"recordsPath": "products",
"toolSchemas": [
  { "tool": "get_products",  "schemaId": "<dim_product asset id>" },
  { "tool": "get_customers", "schemaId": "<dim_customer asset id>", "recordsPath": "customers" }
]
```

On a `tools/call`, the policy reads `params.name` and looks it up in `toolSchemas`.
That entry is the **only** way a schema is chosen: there is no default schema, and
callers can't pick one with a header or token claim.

- Tool names match **exactly** (case-sensitive).
- **Unmapped tools pass through unfiltered**, as do other MCP methods
  (`tools/list`, `resources/read`, `prompts/get`, …). Map every tool that returns
  governed data.
- The global `recordsPath` is the default for entries without their own.
- Each schema's field map is cached under its own key, so mapped schemas are
  fetched from CDGC once each and then served from cache.

---

## Worldtour Lausanne: Sales Order MCP tools

The Worldtour Lausanne demo MCP Server exposes three tools from the **Sales Order
Management API** (Exchange `sales-order-management-api` 1.0.1, managed instance
21221660 on Flex Gateway). Each tool is governed by its own CDGC flat-file schema.

| MCP tool (exact name) | CDGC flat file | schemaId |
|---|---|---|
| `sales-order-management-api_search_sales_orders` | `sales_orders_search.csv` | `3da527db-220e-4667-a703-a54bc440d132` |
| `sales-order-management-api_create_sales_order` | `sales_order_create.csv` | `fd729864-37a1-4795-b93e-518b81a47504` |
| `sales-order-management-api_get_sales_order` | `sales_order_detail.csv` | `f5dffd5c-4c59-4175-9b81-6632f855117a` |

```json
"recordsPath": "orders",
"toolSchemas": [
  { "tool": "sales-order-management-api_search_sales_orders", "schemaId": "3da527db-220e-4667-a703-a54bc440d132", "recordsPath": "orders" },
  { "tool": "sales-order-management-api_create_sales_order",  "schemaId": "fd729864-37a1-4795-b93e-518b81a47504", "recordsPath": "orders" },
  { "tool": "sales-order-management-api_get_sales_order",     "schemaId": "f5dffd5c-4c59-4175-9b81-6632f855117a", "recordsPath": "orders" },
  { "tool": "inventory-fulfillment-api_check_inventory",      "schemaId": "a578e1ec-0eb0-4969-837c-38d2ec885bfe", "recordsPath": "stock_items" },
  { "tool": "inventory-fulfillment-api_get_inventory_item",   "schemaId": "e133a436-bb53-428c-b983-6a9196d84220", "recordsPath": "stock_items" }
],
"sensitiveLevels": ["confidential", "restricted"]
```

**How the catalog side is set up (IDMC Data Catalog > Flat Files):**

- **Catalog source.** A dedicated File System source, **Worldtour Sales Orders**
  (`d7161228-e860-344a-b9f9-7c4c3c32be8b`), scans
  `/data/csv/worldtour-sales-orders/` on the Secure Agent's persistent volume.
- **One sub-folder per CSV.** Each CSV sits in its own sub-folder
  (`<name>/<name>.csv`). The three files share the same columns, and in a single
  folder the scanner merges same-schema files into one partitioned flat file.
- **Capabilities.** Only Metadata Extraction (+ Glossary Association) is on. The
  policy needs the columns only, not profiling or DQ.
- **Columns.** Each file has the 30 fields of an `orders[]` record. 19 columns are
  linked (`IClassTechnicalGlossaryBase`) to dedicated **WTL** Business Terms, so
  the shared `dim_product` terms are untouched.

| Security Level | Columns | Result for a non-entitled caller |
|---|---|---|
| Restricted | `card_number`, `card_expiry`, `iban`, `date_of_birth`, `tax_id` | withheld |
| Confidential | `customer_name`, `customer_email`, `customer_phone`, `billing_address`, `credit_limit`, `unit_cost`, `margin_pct`, `sales_rep_commission` | withheld |
| Internal / Public | `order_id`, `customer_id`, `sku`, `order_total`, `unit_price`, `status` | visible |
| (no term) | the other 11 columns | visible |

**Verification.** The policy's lookup chain (steps 1–5 above) was replayed against
the tenant for all three schemaIds on 2026-10-07. Each returned 30 columns, 19
links, and the 13 sensitive fields listed, with no column linked to more than one
term. An end-to-end MCP call through the gateway with this config hasn't been run
yet. To change what is masked, change a WTL term's Security Level in CDGC. The
policy picks it up after `refreshIntervalSeconds`.

**Inventory tools (2026-10-09).** Two inventory tools from the **Inventory &
Fulfillment API** (`inventory-fulfillment-api` 1.0.1) are governed the same way,
included in the config above. The tool names are assumed from the sales prefix
pattern, so check them in the MCP Server's tool list.

| MCP tool (assumed name) | CDGC flat file | schemaId |
|---|---|---|
| `inventory-fulfillment-api_check_inventory` | `check_inventory.csv` | `a578e1ec-0eb0-4969-837c-38d2ec885bfe` |
| `inventory-fulfillment-api_get_inventory_item` | `get_inventory_item.csv` | `e133a436-bb53-428c-b983-6a9196d84220` |

- **Catalog source.** A dedicated source, **Worldtour Inventory Fulfillment**
  (`f22d3107-7213-3b15-a213-488673319d34`), scans `/data/csv/worldtour-inventory/`
  with the same capabilities.
- **Columns and terms.** Each file has the 19 `stock_items` columns. 12 are linked to
  dedicated WTL stock/supplier terms.
- **Sensitive fields.** `supplier_bank_account` (Restricted), plus `unit_cost`,
  `inventory_value`, `supplier_contract_price` and `supplier_contact_email`
  (Confidential).
- **Scanner links.** Glossary Association also linked `sku` and `unit_cost` to the
  shared terms "Stock Keeping Unit" and "Unit Cost". Those have no Security Level, so
  they don't change the result.
- **Verification.** Replaying the lookup chain returned 5 sensitive fields per schema.

**UI visibility.** Links must have `core.curationStatus: ACCEPTED` to show in the
CDGC Glossaries column. The policy reads them either way.

**MCP Server catalog source (2.2.0+).** The same tools are also catalogued as
MCP Tool assets in the custom catalog source **Worldtour Lausanne MCP Server**
(`848893d6-6cc2-3657-a911-475a278af76c`), with their fields linked to the same WTL
terms. Point `toolSchemas` at the MCP Tool ids instead of the flat files:

| MCP tool | MCP Tool asset id | recordsPath |
|---|---|---|
| `sales-order-management-api_search_sales_orders` | `22ec6387-c0f2-4a1f-bf8e-9e69db90171f` | `orders` |
| `sales-order-management-api_create_sales_order` | `113f2119-b11d-4aba-b9e1-5dcaba97b3de` | `orders` |
| `inventory-fulfillment-api_check_inventory` | `1127fd2e-17ff-4028-ba5d-983e2592fd02` | `stock_items` |

Verified live on 2026-10-09 against sd-mcp (instance 21226262) for the search tool:
13 fields masked without clearance, none with `x-dp-clearance: restricted`.

**Not covered.** Error results aren't records under `orders`, so they pass
through unfiltered. The 402 `PAYMENT_DECLINED` response returns `card_number`
and the 409 `CREDIT_LIMIT_EXCEEDED` response returns `iban`. Fix them in the
mock if those scenarios are demoed.

---

## How it decides — caller entitlement

The caller's identity is read on the request leg from two headers (set by an
upstream identity/JWT-mapping policy, or by the agent for testing):

| Header (default) | Meaning |
|---|---|
| `x-dp-clearance` | the caller's clearance level (e.g. `internal`, `restricted`) |
| `x-dp-purpose` | the caller's declared processing purpose (e.g. `analytics`, `fraud-detection`) |

A caller is **entitled to see sensitive fields** iff:

- their clearance is in `clearedLevels` (multi-select), **and**
- if `allowedPurposes` is non-empty, their purpose is in it too.

The decision is **fail-closed**: absent or insufficient claims withhold *every*
sensitive field. Non-sensitive fields always pass through. When a caller isn't
entitled, each sensitive field present in a record is withheld per `maskMode`:

| `maskMode` | Effect on a withheld field |
|---|---|
| `mask` (default) | value replaced with `maskToken` (default `***`) |
| `nullify` | value set to JSON `null` |
| `drop` | field removed from the record |

The policy stamps an **`x-entitlement-filtered: <n>`** response header (how many
sensitive fields are withheld for this caller) and rewrites the payload with an
`_entitlement` annotation so the decision is auditable in-band:

```json
"_entitlement": { "entitled": false, "clearance": "internal", "purpose": "analytics",
  "mode": "mask", "withheld": ["unit_cost"], "assetId": "<schemaId>",
  "name": "dim_product.csv", "externalId": "<externalId>", "source": "cdgc" }
```

The policy is **fail-open only on its own CDGC outage** (`failOpenOnCdgcError`,
default `true`): with a cached map it keeps enforcing; with no map it passes the
response through rather than blocking on its own dependency. The *caller-entitlement*
decision itself is always fail-closed.

---

## Live demo (verified against the real governed `dim_product.csv`)

```
── analyst (clearance=internal, purpose=analytics) ──
  x-entitlement-filtered header : 1
  entitled                      : False
  withheld fields               : ['unit_cost']  (mode=mask)
  unit_cost the caller sees     : ***

── fraud investigator (clearance=restricted, purpose=fraud-detection) ──
  x-entitlement-filtered header : 0
  entitled                      : True
  withheld fields               : (none)  (mode=mask)
  unit_cost the caller sees     : 42.50
```

Same tool, same upstream record, same gateway endpoint — only the caller's clearance
+ purpose differ. `unit_cost`'s sensitivity came from CDGC (its Business Term is
marked Confidential); nothing was configured per field. Run:
`cp demo/config.json.example demo/config.json` (fill id/creds) → provision per
[`demo/PROVISION.md`](demo/PROVISION.md) → `./demo/demo.sh`.
[`demo/WALKTHROUGH.md`](demo/WALKTHROUGH.md) traces both personas field by field.
The demo agent also calls a second tool, `get_customers`, governed by its own
schema through `toolSchemas`. The output above is from the single-tool 1.2.0 run;
the two-tool run hasn't been verified against a live gateway yet.

---

## Configuration reference

| Property | Type | Default | Description |
|---|---|---|---|
| `cdgcLoginUrl` | string (service) | required | IDMC login base URL. |
| `cdgcSearchUrl` | string (service) | required | CDGC search host (serves `ccgf-searchv2`). |
| `cdgcOrgUsername` / `cdgcOrgPassword` | string (sensitive) | required | IDMC read-only service account. |
| `toolSchemas` | array of `{tool, schemaId, recordsPath?}` | required | Maps each MCP tool to the CDGC schema (and records path) governing its results. Unmapped tools pass through. See "Mapping MCP tools to schemas". |
| `fieldClassTypes` | array of string | `[com.infa.odin.models.file.flat.FlatField, worldtour.mcp.McpToolField]` | `core.classType` values of a schema's fields. Add the field class of any other custom model. |
| `recordsPath` | string | `""` | Default `/`-path to the record(s) projected (`products`); array = each element. Overridden per tool by `toolSchemas[].recordsPath`. |
| `sensitiveLevels` | array (multi-select) | `[confidential, restricted]` | Business Term Security Levels that make a linked column sensitive. |
| `sensitiveMarker` | string | `confidential` | Fallback only, for terms with no Security Level: case-insensitive substring in the term description that marks the field sensitive. |
| `clearanceHeader` | string | `x-dp-clearance` | Request header carrying the caller's clearance level. |
| `clearanceClaim` | string | _unset_ | Optional JWT claim name for the caller's clearance; when set + present it is used instead of `clearanceHeader`. Needs an upstream JWT Validation policy. |
| `purposeHeader` | string | `x-dp-purpose` | Request header carrying the caller's declared purpose. |
| `purposeClaim` | string | _unset_ | Optional JWT claim name for the caller's purpose; when set + present it is used instead of `purposeHeader`. Needs an upstream JWT Validation policy. |
| `clearedLevels` | array (multi-select) | `[restricted]` | Clearance values entitled to see sensitive fields (`public` / `internal` / `confidential` / `restricted`). |
| `allowedPurposes` | string | `""` | Comma-separated purposes entitled to see sensitive fields. Empty = purpose not checked. |
| `maskMode` | enum | `mask` | How a withheld field is rendered (`mask` / `nullify` / `drop`). |
| `maskToken` | string | `***` | Replacement value when `maskMode = mask`. |
| `refreshIntervalSeconds` | integer | `86400` | Field-map cache TTL. |
| `failOpenOnCdgcError` | boolean | `true` | Serve last-known-good map on transient CDGC error; no map → pass through. |
| `distributed` | boolean | `false` | Share cache + refresh lock across replicas. |
| `timeout` | integer (ms) | `5000` | Per-CDGC-call timeout (≤ ~15s chained budget). |

---

## Repository layout

```
field-level-entitlement-filter-definition/   # gcl.yaml, exchange.json, Makefile
field-level-entitlement-filter-flex/          # Rust implementation
  src/lib.rs          # CDGC auth + ccgf-searchv2 sensitivity derivation + cache-aside + body projection
  src/entitlement.rs  # PURE: caller entitlement + mask/nullify/drop projection — 11 unit tests
  src/cdgc.rs         # PURE: nonce + cached field-map types
  src/claims.rs       # PURE: decode caller Bearer-JWT claims (opt-in clearance/purpose source) — unit-tested
  src/routing.rs      # PURE: tool → schema/recordsPath resolution — 5 unit tests
demo/  # two-tool mock (get_products, get_customers), config (toolSchemas + entitlement rule), two-persona agent, PROVISION, WALKTHROUGH
```

### Sourcing caller claims from a JWT

By default clearance and purpose are read from request headers. Set
`clearanceClaim` / `purposeClaim` to read them from the caller's Bearer JWT
instead — a configured claim wins over its header, and if unset or absent the
header is used. The token is only **decoded** here; put a **JWT Validation policy
upstream** on the same instance to verify the signature/expiry, otherwise the
claims are attacker-controlled.

---

## Build, test & release

```bash
cd field-level-entitlement-filter-definition && make release
cd ../field-level-entitlement-filter-flex
make build-asset-files && cargo build --target wasm32-wasip1 --release
cargo test --lib            # 24 pure unit tests
make release
```
**2.2.0** adds `fieldClassTypes`, so a `schemaId` can be an MCP Tool asset of the
custom MCP Server catalog source (its fields are `worldtour.mcp.McpToolField`).
Before, only `FlatField` columns were read, so an MCP Tool id found no fields and
the response passed through unfiltered. Existing configs keep working.
**2.1.0** checks every Business Term linked to a column, not just the first. The
column is sensitive if any of its terms is, so a level-less term added by the
scanner's Glossary Association can no longer hide a Confidential term. Config is
unchanged from 2.0.0.
**2.0.0** (breaking) makes `toolSchemas` the only schema source. `schemaId`,
`schemaIdHeader` and `schemaIdClaim` are removed, and `toolSchemas` is required.
Unmapped tools pass through unfiltered. The policy now applies to MCP APIs only
(`assetTypes: mcp`). To upgrade a 1.x config, move `schemaId` into a
`toolSchemas` entry per tool and delete the three removed properties.
**1.3.1** renames the policy's display name to "MCP Tool Response Field Entitlement
Filter". Asset ids (`field-level-entitlement-filter` / `-flex`) and behavior are
unchanged.
**1.3.0** adds the optional `toolSchemas` per-tool schema mapping, fully backward
compatible with 1.2.0 configs. A header-supplied schema with no field map now falls
back to `schemaId` instead of passing the response through.
Previously published at **1.2.0** (1.1.0 added opt-in JWT-claims sourcing for clearance /
purpose / schema id — see "Sourcing caller claims from a JWT"; header mode
remains the default. **1.2.0 turns `sensitiveLevels` and `clearedLevels` into
multi-select dropdowns** — `type: array` of `[public, internal, confidential,
restricted]` in API Manager, instead of a comma-separated string). Requires
**PDK 1.10**.

---

## Caveats & scope

- **Requires an MCC scan** so the schema asset has governed columns; sensitivity
  needs columns linked to Business Terms with a Security Level (or, failing that,
  a description carrying the marker).
- **Only mapped tools are filtered.** A tool missing from `toolSchemas` (or whose
  name doesn't match exactly) returns its response unfiltered.
- **Only records at `recordsPath` are projected.** Sensitive values elsewhere in
  the payload (e.g. top-level fields of an error body) pass through.
- **Body-inspecting** → JSON / single-message-SSE `tools/call` results; whole-stream
  SSE rewrites are out of scope (event-local rewrite is future work).
- Reads caller claims from **request headers**; JWT-claim extraction is a natural
  growth path (the PDK `jwt` feature is available).
- The entitlement decision is **fail-closed**; the policy is fail-open on its own
  CDGC outage.
- Calls the `ccgf-searchv2` API on `cdgc-api` (a `format:service` egress) — the
  gateway must reach `*.informaticacloud.com`.

---

## Skills used

- **PDK** (`omni-gateway-pdk-skills`): `pdk-create-policy`, `pdk-mcp`,
  `pdk-request-headers-bodies`, `pdk-share-data-request-response`, `pdk-data-storage`,
  `pdk-schema-definition`, `pdk-policy-violations`, `pdk-sse-parsing`,
  `pdk-code-style`, `pdk-coding-best-practices`, `pdk-unit-tests`, `pdk-publish-policies`.
- **P4A** (`p4a-skills`): `p4a-build-policy`, `p4a-mcp-usage`,
  `p4a-test-mcp-policies-with-a2d`.
- Reuses the CDGC `ccgf-searchv2` derivation engine from the sibling
  **Data Product Contract Conformance Guard** policy.
