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
    data product"). Open question for the user: should a claim/mapping mismatch
    fail closed (withhold all sensitive fields) instead? Not implemented.
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
- [x] `cargo test --lib`: 22/22 pass
- [x] `cargo build --target wasm32-wasip1 --release`: OK; clippy clean
- [x] README
- [ ] Not done: end-to-end run against a gateway with two real CDGC schemas
      (needs a second scanned schema asset id, e.g. a customer table).
- [ ] Not done: demo update (a second tool in `demo/agent.py` / mock upstream + a
      `toolSchemas` entry in `demo/config.json`).
- [ ] Not done: publish. `make release` in the definition folder, then in the flex
      folder. Note `make build-asset-files` fetches the definition from Exchange, so
      publish the definition before releasing the implementation.
- [ ] Decide: fail-closed on claim vs tool-mapping mismatch?

## Commands
```bash
cd field-level-entitlement-filter-flex
cargo test --lib
cargo build --target wasm32-wasip1 --release
```
