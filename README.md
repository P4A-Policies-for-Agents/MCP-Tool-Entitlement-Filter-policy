# Field-Level Entitlement Filter — MuleSoft Omni/Flex Gateway Policy

An **inbound, body-inspecting** custom policy for the MuleSoft Omni/Flex Gateway
that **derives per-field sensitivity live from Informatica CDGC** and then, on the
response leg, **projects each field for the caller** — masking, nulling, or dropping
the sensitive fields the caller's **clearance** and **declared purpose** don't
entitle them to see.

The upstream returns the *same* record to everyone. What each caller receives is
decided at the gateway from *who they are*: a cleared fraud investigator sees
`unit_cost`; an analyst gets it masked. Nothing is configured per field — the field
set and which fields are sensitive come from CDGC.

Built with the PDK, Rust → `wasm32-wasip1`, split-model. Applies to **MCP**
(`tools/call`) and **REST/HTTP APIs** (`assetTypes: mcp,rest,http`) — both bind to
a data-product schema, so per-field governance maps cleanly. (A2A was dropped:
agents aren't bound to a schema, so field-level derivation doesn't apply.) The
policy unwraps the MCP JSON-RPC envelope when present, and otherwise treats the
REST response body as the payload directly.

---

## How sensitivity is derived (catalog-driven)

You configure it with **one id** — a **CDGC asset id** for the scanned schema (a
flat file, table, etc.). On a cache miss the policy authenticates to IDMC
(**Login → JWT**) and then, via the CDGC search API
**`POST cdgc-api…/ccgf-searchv2/api/v1/search`** (Elasticsearch DSL,
`X-INFA-SEARCH-LANGUAGE: elasticsearch`):

1. **Resolve the schema asset** (`core.identity = schemaId`) → its `core.location`, name, external id.
2. **Enumerate its columns** — `FlatField` assets under the schema location → field **names**.
3. **Enumerate column → Business Term links** — `elementType=RELATIONSHIP`, `type=IClassTechnicalGlossaryBase`.
4. **Resolve the linked terms** → `core.name`, `core.description`.
5. **Build the field map**: one entry per column, flagged **sensitive** when its
   Business Term description contains `sensitiveMarker` (default *"confidential"*).

The map is cached in PDK DataStorage (lazy refresh, single-flight, `distributed`
for cross-replica). Credentials are `security:sensitive`; CDGC response bodies are
never logged. This mirrors the CDGC governance graph: **catalog source → scanned
table → columns → Business Terms**.

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

---

## Configuration reference

| Property | Type | Default | Description |
|---|---|---|---|
| `cdgcLoginUrl` | string (service) | required | IDMC login base URL. |
| `cdgcSearchUrl` | string (service) | required | CDGC search host (serves `ccgf-searchv2`). |
| `cdgcOrgUsername` / `cdgcOrgPassword` | string (sensitive) | required | IDMC read-only service account. |
| `schemaId` | string | required | CDGC asset id of the scanned schema whose columns/terms define sensitivity. |
| `schemaIdHeader` | string | `x-dp-schema-id` | Per-request schema-asset id override. |
| `schemaIdClaim` | string | _unset_ | Optional JWT claim name to read `schemaId` from; when set + present it wins over `schemaIdHeader`. Needs an upstream JWT Validation policy. |
| `recordsPath` | string | `""` | `/`-path to the record(s) projected (`products`); array = each element. |
| `sensitiveMarker` | string | `confidential` | Case-insensitive substring in a field's term description that marks it sensitive. |
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
  src/claims.rs       # PURE: decode caller Bearer-JWT claims (opt-in clearance/purpose/schema source) — unit-tested
demo/  # dim_product-shaped mock, config (schemaId + entitlement rule), two-persona agent, PROVISION, WALKTHROUGH
```

### Sourcing caller claims from a JWT

By default clearance and purpose are read from request headers. Set
`clearanceClaim` / `purposeClaim` (and optionally `schemaIdClaim`) to read them
from the caller's Bearer JWT instead — a configured claim wins over its header,
and if unset or absent the header is used (fully backward compatible). The token
is only **decoded** here; put a **JWT Validation policy upstream** on the same
instance to verify the signature/expiry, otherwise the claims are attacker-controlled.
`schemaIdClaim` is a hardening lever: it binds the caller to a data product via
the signed token so a spoofed `x-dp-schema-id` header can't redirect the policy.

---

## Build, test & release

```bash
cd field-level-entitlement-filter-definition && make release
cd ../field-level-entitlement-filter-flex
make build-asset-files && cargo build --target wasm32-wasip1 --release
cargo test --lib            # 11 pure unit tests
make release
```
Published at **1.2.0** (1.1.0 added opt-in JWT-claims sourcing for clearance /
purpose / schema id — see "Sourcing caller claims from a JWT"; header mode
remains the default. **1.2.0 turns `sensitiveLevels` and `clearedLevels` into
multi-select dropdowns** — `type: array` of `[public, internal, confidential,
restricted]` in API Manager, instead of a comma-separated string). Requires
**PDK 1.10**.

---

## Caveats & scope

- **Requires an MCC scan** so the schema asset has governed columns; sensitivity
  needs columns linked to Business Terms whose description carries the marker.
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
