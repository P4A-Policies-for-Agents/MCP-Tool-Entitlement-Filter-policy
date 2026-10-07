# Demo walkthrough — how the filter decides, field by field

This traces the two demo callers end to end: the raw upstream record, the
sensitivity map the policy derived from CDGC, and exactly why one caller sees
`unit_cost` and the other gets it masked. Nothing here is configured per field —
the only catalog input is the one `schemaId`.

## The sensitivity map (derived live from CDGC — not configured)

On the first call the policy resolves `schemaId = <schemaId>` (the CDGC asset id of `dim_product.csv`)
in CDGC, enumerates the scanned asset's columns, follows each column → Business Term
link, and builds this map for `dim_product.csv` (then caches it, TTL
`refreshIntervalSeconds = 86400`):

- **11 governed columns**: `brand, category, department, is_sellable, launch_date,
  lifecycle_state, list_price, product_name, sku, subcategory, unit_cost`
- **sensitive** (the term's description contains `sensitiveMarker` = `confidential`): `unit_cost`

`recordsPath = products` tells the policy to project **each element of the `products`
array** in the tool result.

## The upstream record (identical for every caller)

The mock `get_products` returns the same record no matter who calls:

```json
{ "products": [ {
  "sku":"SKU-1001","product_name":"Blender X","brand":"Acme","category":"Kitchen",
  "subcategory":"Small Appliances","department":"Home","list_price":"99.99",
  "unit_cost":"42.50","is_sellable":"true","launch_date":"2025-01-10",
  "lifecycle_state":"active" } ], "count": 1 }
```

Only `unit_cost` is sensitive. The differentiator is the **caller**, expressed as
two request headers.

## The entitlement rule (this demo's config)

```
clearedLevels   = restricted
allowedPurposes = fraud-detection
maskMode        = mask   (maskToken = ***)
```

A caller sees sensitive fields **iff** clearance ∈ {`restricted`} **and** purpose ∈
{`fraud-detection`}. Otherwise every sensitive field present is masked. The check is
**fail-closed**: missing/blank claims are never entitled.

---

## Caller A — analyst → `unit_cost` masked

Headers: `x-dp-clearance: internal`, `x-dp-purpose: analytics`.

**Policy evaluation**
- clearance `internal` ∉ `clearedLevels` → **not cleared** → not entitled.
- `unit_cost` is sensitive and present → **withheld** (masked to `***`).
- The other 10 columns are non-sensitive → untouched.

**What the client receives** (`unit_cost` masked; `x-entitlement-filtered: 1`):

```json
{ "_entitlement": { "entitled": false, "clearance":"internal", "purpose":"analytics",
    "mode":"mask", "withheld":["unit_cost"], "assetId":"<schemaId>",
    "name":"dim_product.csv", "externalId":"<externalId>", "source":"cdgc" },
  "count": 1,
  "products": [ { "brand":"Acme","category":"Kitchen","department":"Home",
    "is_sellable":"true","launch_date":"2025-01-10","lifecycle_state":"active",
    "list_price":"99.99","product_name":"Blender X","sku":"SKU-1001",
    "subcategory":"Small Appliances","unit_cost":"***" } ] }
```

---

## Caller B — fraud investigator → `unit_cost` visible

Headers: `x-dp-clearance: restricted`, `x-dp-purpose: fraud-detection`.

**Policy evaluation**
- clearance `restricted` ∈ `clearedLevels` **and** purpose `fraud-detection` ∈
  `allowedPurposes` → **entitled**.
- Nothing is withheld.

**What the client receives** (`unit_cost` intact; `x-entitlement-filtered: 0`):

```json
{ "_entitlement": { "entitled": true, "clearance":"restricted", "purpose":"fraud-detection",
    "mode":"mask", "withheld":[], "assetId":"<schemaId>",
    "name":"dim_product.csv", "externalId":"<externalId>", "source":"cdgc" },
  "count": 1,
  "products": [ { "…":"…", "unit_cost":"42.50" } ] }
```

---

## Why this matters

Same tool, same data, same gateway endpoint. The gateway — not the upstream and not
the agent — enforces *who may see what*, and it does so from **catalog-derived
sensitivity** (`unit_cost` is Confidential in CDGC) rather than a hand-maintained
field list. Change the caller's clearance or purpose and the projection changes; mark
a new column Confidential in CDGC and it's protected on the next cache refresh, with
no policy edit.

## Try the other mask modes

Set `maskMode` to `nullify` (value → `null`) or `drop` (field removed) in
`config.json`, re-apply the policy, and re-run — the analyst's `unit_cost` becomes
`null` or disappears entirely.

## Run it yourself

```bash
cp env.local.sh.example env.local.sh   # set CMP_GW_URL to the governed endpoint
./demo.sh                              # or: CMP_GW_URL=… python3 agent.py
```

Provisioning (mock + gateway instance + policy) is in [`PROVISION.md`](./PROVISION.md).
