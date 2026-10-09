# Agent notes

"MCP Tool Response Field Entitlement Filter": MuleSoft Omni/Flex Gateway custom policy (PDK, Rust → wasm32-wasip1), extended in
1.3.0 with per-MCP-tool schema mapping (`toolSchemas`).

Read `PROGRESS.md` first: it has the origin of this copy, design decisions, what
changed, and open tasks. Keep it updated as work progresses.

- After editing `field-level-entitlement-filter-definition/gcl.yaml`, regenerate the
  config struct locally with
  `cargo anypoint config-gen --manifest ../field-level-entitlement-filter-definition/gcl.yaml --output ./src/generated/config.rs`
  (run from `field-level-entitlement-filter-flex/`).
- Pure logic lives in `src/routing.rs`, `src/entitlement.rs`, `src/claims.rs`,
  `src/cdgc.rs` with unit tests; `src/lib.rs` is the gateway wiring.
- `demo/config.json` and `demo/env.local.sh` contain real credentials and are
  gitignored. Never commit them.
