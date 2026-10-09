# MCP Tool Response Field Entitlement Filter — progress & memory

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
- [x] Released 2026-10-09 into the **worldtour-lausanne** org
      (`7e7dd113-5f1b-4a58-be93-21acdc20f1f7`, now the CLI's connected org): definition
      and implementation 1.3.0. `group_id` in `Cargo.toml` and `groupId` in
      `exchange.json` now point there. The 2026-10-07 release in `030e0aac-…` is
      untouched, but the current CLI login can't see that org.
  - Build gotcha: `cargo anypoint build-policy` failed with "No such file or directory"
    because cargo had pruned
    `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cargo-anypoint-1.9.0/`
    (it reads `resources/gcl/base.yaml` from there). Restored it from the crates.io
    `.crate`; `cargo install cargo-anypoint@1.9.0 --force` also fixes it.
- [x] 1.3.1 released 2026-10-09 into worldtour-lausanne: display name is now
      "MCP Tool Response Field Entitlement Filter" (`gcl.yaml` `title`, all docs and
      code comments, `../demo.md`). Asset ids are unchanged. The published 1.3.1
      definition metadata carries the new name.
  - [ ] Exchange still lists both assets under the old name ("Field-Level Entitlement
        Filter" / "… Flex"). Exchange keeps the name from the first publish.
        `anypoint-cli-v4 exchange:asset:modify <g>/<a>/<v> --name …` reported success
        for 1.3.0 and 1.3.1 but didn't change it. Rename in the Exchange UI (asset page
        → edit name) for both assets.
- [x] 2.0.0 (breaking, decided 2026-10-09): `toolSchemas` is the only schema source.
  - Removed `schemaId`, `schemaIdHeader`, `schemaIdClaim` (and the 1.3.0 header
    fallback). `toolSchemas` is required.
  - Unmapped tools and non-`tools/call` methods pass through unfiltered (user's
    choice over blocking them).
  - MCP only: `assetTypes: mcp`, `interfaceScope: api`; the REST payload handling
    (`Place::Rest`) is gone.
  - `routing.rs` is now `called_tool` + `resolve(mappings, tool, default_records_path)
    -> Option<Route>`, 5 tests; 20/20 tests total. Global `recordsPath` stays as the
    default for entries without one.
  - Demo: `agent.py` header-spoof call removed; local `demo/config.json` no longer
    has `schemaId`. README, PROVISION, WALKTHROUGH, `../demo.md` updated.
  - Version 2.0.0 in `exchange.json` and `Cargo.toml`.
  - Released 2026-10-09 into worldtour-lausanne (definition + implementation 2.0.0).
    The implementation publish first failed: Exchange caps an implementation's
    description at 256 characters, and it is copied from the definition's (258).
    The definition accepted it. Fixed by shortening the description in the
    generated `target/.../implementation/exchange.json` and running
    `anypoint-cli-v4 pdk policy-wasm publish` directly. `gcl.yaml` now has the
    short description (178 chars), so the next version (2.0.1+) builds normally.
    Keep `metadata.labels.description` under 256 characters.
- [x] 2.1.0 released 2026-10-09 into worldtour-lausanne (definition + implementation).
      Multi-term columns: `classify_terms` in `entitlement.rs` marks a column sensitive
      if any linked term is (the first sensitive term is reported). Config unchanged.
      22/22 tests.
- [x] Credential check for pushing to a remote (2026-10-09): the real CDGC username,
      password and gateway URL from `demo/config.json` / `demo/env.local.sh` appear in
      no tracked file and in no commit; both files are gitignored and were never
      committed. No tokens, keys or secret patterns in the tree or history. Remaining
      identifiers (not secrets): Anypoint org ids as `groupId`, CDGC schema/source
      ids, the commit author email.
- [ ] Not done: upgrade the demo API instance from 1.2.0 to 2.1.0 (remove + re-apply
      with `demo/config.json`, `--policyVersion 2.0.0`), then re-run `demo/demo.sh`.
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

## Worldtour Lausanne Sales Order schemas (2026-10-07)
- Three CDGC flat-file schemas were created, one per Sales Order MCP tool. IDs, the
  `toolSchemas` config and the term model are in README, "Worldtour Lausanne: Sales
  Order MCP tools". Rebuild steps and scripts are in `../demo.md` section 10 and
  `../cdgc/`.
- Verified only by replaying the lookup chain against CDGC: 13 sensitive fields per
  schema.
- [ ] Not done: apply the 3-tool `toolSchemas` config to the MCP Server instance and
      run both personas end to end.
- Found: error bodies outside `recordsPath` are not projected. The 402/409 sales
  mocks leak `card_number` / `iban`.
- README "How sensitivity is derived" now describes the real logic: Security Level
  first, description marker only as a fallback.
- All 57 sales column→term links were re-created with `core.curationStatus: ACCEPTED`
  (DELETE + INSERT, `../cdgc/link.py`). Without that status the policy still sees the
  link, but the CDGC UI Glossaries column stays empty. The status can't be UPDATEd.
  Now documented in `../demo.md` section 10 step 6.

## Worldtour Lausanne Inventory schemas (2026-10-09)
- Two CDGC flat-file schemas, both returning `stock_items` with 19 columns:
  - `inventory-fulfillment-api_check_inventory` → `check_inventory.csv`
    `a578e1ec-0eb0-4969-837c-38d2ec885bfe`
  - `inventory-fulfillment-api_get_inventory_item` → `get_inventory_item.csv`
    `e133a436-bb53-428c-b983-6a9196d84220`
- Catalog source **Worldtour Inventory Fulfillment**
  `f22d3107-7213-3b15-a213-488673319d34`, path `/data/csv/worldtour-inventory/`.
  It was cloned from the sales source.
- 12 dedicated WTL terms (`INVENTORY_TERMS` in `../cdgc/terms.py`, ids in
  `../cdgc/term_ids_inventory.json`), linked as ACCEPTED with `link.py inventory`.
- Verified with `verify.py`: 5 sensitive fields per schema (`supplier_bank_account`,
  `unit_cost`, `inventory_value`, `supplier_contract_price`, `supplier_contact_email`).
- The scanner's Glossary Association also auto-linked `sku` and `unit_cost` to the
  shared terms "Stock Keeping Unit" and "Unit Cost". Those have no Security Level, so
  the policy ignores them. They were left in place.
- The ingest took longer than 25 minutes after the sync. It had finished by 2026-10-09.
- README, `../demo.md` sections 6, 8 and 10 updated; the stray empty heading was removed.
- [x] Tool names confirmed by the user (2026-10-09). The MCP Server exposes exactly 3 tools:
      `sales-order-management-api_search_sales_orders`,
      `sales-order-management-api_create_sales_order` and
      `inventory-fulfillment-api_check_inventory`. The `get_sales_order` /
      `get_inventory_item` schemas exist in CDGC but are not used by this server.
- [ ] Not done: apply the 5-tool `toolSchemas` config and run end to end.

## Commands
```bash
cd field-level-entitlement-filter-flex
cargo test --lib
cargo build --target wasm32-wasip1 --release
```

## CDGC custom model for the MCP Server (draft, 2026-10-09)
- Files in `../cdgc/custom-model/`, built by `gen_model.py` from the A2D mocks:
  - `worldtour.mcp.json`: package `worldtour.mcp`. McpServer (core.DataSource) contains
    McpTool (core.DataSet), which contains McpToolField (core.DataElement), via
    core.ParentChild.
  - `worldtour_mcp.zip`: 1 server, the 3 tools and 78 fields (30/29/19).
- Based on the MCC "Custom Metadata Integration Reference" (July 2026). The model is
  uploaded as JSON and the content as a ZIP of CSVs plus `links.csv`.
- [x] Done in the MCC UI by the user (2026-10-09): the model was published, the Custom
      Catalog Source Type "MCP Server" was created, and the catalog source "Worldtour
      Lausanne MCP Server" `848893d6-6cc2-3657-a911-475a278af76c` was run (83 assets).
  - McpTool ids: search_sales_orders `22ec6387-c0f2-4a1f-bf8e-9e69db90171f`,
    create_sales_order `113f2119-b11d-4aba-b9e1-5dcaba97b3de`, check_inventory
    `1127fd2e-17ff-4028-ba5d-983e2592fd02`.
- [x] 50 field→term links written ACCEPTED (`../cdgc/link_mcp.py`). IClassTechnicalGlossaryBase
      works on custom classes, and the `path_hierarchy.parent` lookup finds the fields.
      `verify.py` (now accepts McpToolField) gives 13/13/5 sensitive fields, the same as the CSV schemas.
- [x] 2.2.0 released 2026-10-09 into worldtour-lausanne (definition + implementation):
      new `fieldClassTypes` config (default FlatField + `worldtour.mcp.McpToolField`,
      `cdgc::field_class_types`, 2 tests, 24/24). Cause it fixed: on sd-mcp the search
      tool was mapped to McpTool `22ec6387-…`, 2.1.0 found no FlatField children
      ("no columns found"), and `failOpenOnCdgcError` passed the response through.
  - sd-mcp instance **21226262** (Sandbox), policy 9483012, upgraded 2.1.0 → 2.2.0 with
    `PATCH …/apis/21226262/policies/9483012 {"assetVersion":"2.2.0"}` (the CLI's
    `policy edit` has no version flag; the PATCH kept the sensitive credentials).
    Note: 21221660 is the REST sales-orders instance behind it, not the MCP one.
  - Verified live: search tool, no clearance → `x-entitlement-filtered: 13`, `iban` etc.
    masked; `x-dp-clearance: restricted` → 0 withheld.
  - [ ] Only the search tool is mapped on 21226262. Add create_sales_order
        (`113f2119-…`, `orders`) and check_inventory (`1127fd2e-…`, `stock_items`).
  - [ ] sd-mcp has no Client ID Enforcement / JWT policy, and clearance is a caller
        header, so any caller can send `x-dp-clearance: restricted`.


## Multi-term column fix (2026-10-09, unreleased, local only)
- Bug: `fetch_field_map` kept only the FIRST term link per column (`or_insert`). The
  inventory `sku` / `unit_cost` columns also carry level-less shared terms from the
  scanner's Glossary Association, and CDGC returns those first. `unit_cost` was only
  still masked because "Unit Cost"'s description contains "Confidential".
  (`../cdgc/verify.py` unions all terms, so it did not show the problem.)
- Fix: the column is sensitive if ANY linked term is (`entitlement::classify_terms`,
  2 new unit tests, 22/22 pass, wasm build + clippy clean).
- [x] Released in 2.1.0.
