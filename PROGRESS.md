# MCP Tool Entitlement Filter — progress & memory

## Origin
- Copied on 2026-10-07 from `~/Documents/acb-workspaces/Field-Level Entitlement Filter`
  (policy 1.2.0). `target/` and `.git` were not copied; a fresh git repo was started
  here with the copy as the baseline commit.
- The gitignored local demo files `demo/config.json` and `demo/env.local.sh` were copied
  too (they hold the real CDGC id and credentials). Do not commit them.

## Goal
Before 1.3.0, the policy governed every request with one CDGC schema (`schemaId`,
optionally overridden per request by a header or JWT claim). It never looked at the
MCP tool name. 1.3.0 adds an optional per-tool mapping so each MCP tool can be
governed by its own schema, without breaking existing configs.

## Design decisions
- New optional config property `toolSchemas: [{tool, schemaId, recordsPath?}]`,
  default `[]`.
- Schema precedence: `schemaIdClaim` > `toolSchemas` > `schemaIdHeader` > `schemaId`.
  - The mapping outranks the header because the admin sets the mapping and the caller
    sets the header. Otherwise a caller could point a tool at a non-sensitive schema.
  - The claim still wins, to keep the 1.1.0 meaning ("the token binds the caller to a
    data product"). Confirmed by the user on 2026-10-07: a mismatch is not an error.
- A mapped tool's `recordsPath` applies whichever source supplied the schema, because
  it describes the tool's result shape.
- Tool names are matched exactly (case-sensitive). Only `tools/call` uses the mapping;
  `resources/read`, `prompts/get` and REST calls are unchanged.
- Asset ids stay the same (`field-level-entitlement-filter` / `-flex`). This is a
  backward-compatible minor version of the same policy, not a new Exchange asset.

## What changed (vs baseline commit)
- `field-level-entitlement-filter-definition/gcl.yaml`: added `toolSchemas`.
- `field-level-entitlement-filter-flex/src/generated/config.rs`: regenerated with
  `cargo anypoint config-gen --manifest ../field-level-entitlement-filter-definition/gcl.yaml --output ./src/generated/config.rs`
  (adds `ToolSchemas0Config` and `Config.tool_schemas`; additive only).
- `field-level-entitlement-filter-flex/src/routing.rs` (new, pure): `called_tool`,
  `find_mapping`, `resolve` → `Route { asset_id, records_path }` + 7 unit tests.
- `field-level-entitlement-filter-flex/src/lib.rs`: `Ctx.asset_id` → `Ctx.route`;
  the request filter resolves the route after reading the JSON-RPC body; the response
  filter uses `ctx.route.records_path` instead of the global `recordsPath`.
- Version 1.2.0 → 1.3.0 in `Cargo.toml` and `exchange.json`.
- `README.md`: new "Mapping MCP tools to schemas" section, config table row, layout,
  release notes.

## Status
- [x] Copy + baseline commit
- [x] Definition + generated config
- [x] Routing module + unit tests
- [x] Wire into lib.rs
- [x] `cargo test --lib`: 23/23 pass
- [x] `cargo build --target wasm32-wasip1 --release`: OK; clippy clean
- [x] README
- [x] Demo update (local files):
  - `demo/agent.py` calls `get_products` and `get_customers` for both personas, prints
    `_entitlement.assetId` per call, then runs an analyst call on `get_products` with a
    spoofed `x-dp-schema-id`. MCP `isError` results print as "tool call failed".
  - `demo/mcp-metadata.json` lists `get_customers` (records under `customers`).
  - `demo/config.json.example` has a two-entry `toolSchemas`.
  - Local `demo/config.json` (gitignored) maps only `get_products` to the real
    `dim_product.csv` id. Add `get_customers` once a customer schema id exists. A
    placeholder id would resolve to no field map, so that tool's responses would pass
    through unfiltered.
  - `PROVISION.md` (second schema, `get_customers` mock scenario, `--policyVersion 1.3.0`),
    `WALKTHROUGH.md` (two-tool + header-spoof section), README demo note.
- [ ] Not done: provision `get_customers` on the A2D mock and find a second scanned
      CDGC schema (e.g. `dim_customer.csv` with a Confidential `credit_limit`).
- [x] Header-schema fallback (decided 2026-10-07): when the schema came from
      `schemaIdHeader` and has no field map, the policy governs by `schemaId` instead
      of passing through. `Route.fallback_asset_id` in `routing.rs` (set only for
      header-sourced routes that differ from `schemaId`), used in `response_filter`;
      `_entitlement.assetId` reports the schema actually used. A real CDGC outage with
      no cached map still passes through (`failOpenOnCdgcError`). 23/23 tests pass.
- [x] Decided: claim vs tool-mapping mismatch → the claim wins (unchanged).
- [x] Published 2026-10-07: definition `field-level-entitlement-filter` 1.3.0, then
      implementation `field-level-entitlement-filter-flex` 1.3.0 (org
      `030e0aac-30d9-460f-9234-428c16a123c4`). A later change needs a version bump
      (1.3.1) in `Cargo.toml` and `exchange.json`.
- [ ] Not done: upgrade the demo API instance from 1.2.0 to 1.3.0 (remove + re-apply
      with `demo/config.json`, `--policyVersion 1.3.0`), then re-run `demo/demo.sh`.
      The spoof call should then report the `dim_product.csv` asset with `unit_cost`
      masked.
- [ ] Not done: two-schema end-to-end run (blocked on the `get_customers` mock + a
      second CDGC schema, above).
- [ ] Open, minor: each new bogus header id still triggers one CDGC lookup (up to the
      ~15s budget) before the fallback, so callers can add latency and CDGC load by
      rotating ids. Same as 1.2.0.

## Findings from the live demo run (2026-10-07, deployed policy is still 1.2.0)
- `get_products`, both personas: works. CDGC now also flags `list_price` as sensitive,
  so the analyst gets `withheld: [list_price, unit_cost]` (`x-entitlement-filtered: 2`).
  The README/WALKTHROUGH sample output still shows only `unit_cost`.
- `get_customers`: the mock returns `isError` "Tool get_customers not found" (not
  provisioned yet).
- Header spoof on 1.2.0: analyst + `x-dp-schema-id: 0000…` gets `unit_cost: 42.50`
  unmasked. The bogus id has no field map, and no map means pass-through. 1.3.0
  `toolSchemas` closes this for mapped tools only. Unmapped tools and REST calls are
  still exposed unless `schemaIdClaim` is used or the "no map" case fails closed.
  1.3.0 now falls back to `schemaId` in that case (see Status).
- `x-entitlement-filtered` is stamped before the body is inspected, so it also appears
  (e.g. `2`) on MCP error results and non-record payloads where nothing was masked.

## Commands
```bash
cd field-level-entitlement-filter-flex
cargo test --lib
cargo build --target wasm32-wasip1 --release
```
