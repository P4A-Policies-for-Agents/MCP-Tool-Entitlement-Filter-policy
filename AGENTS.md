# Agent notes

"MCP Tool Response Field Entitlement Filter": MuleSoft Omni/Flex Gateway custom policy (PDK, Rust → wasm32-wasip1), extended in
1.3.0 with per-MCP-tool schema mapping (`toolSchemas`). Since 2.0.0, `toolSchemas` is the
only schema source (no `schemaId` / header / claim) and the policy is MCP-only.

This repo is public and published on p4a.ai: keep it free of tenant-specific
names, ids and hosts. Record releases in `CHANGELOG.md` and keep `P4A.md` (the
submission kit and catalog doc tabs) in step with `gcl.yaml` and the README.

- After editing `field-level-entitlement-filter-definition/gcl.yaml`, regenerate the
  config struct locally with
  `cargo anypoint config-gen --manifest ../field-level-entitlement-filter-definition/gcl.yaml --output ./src/generated/config.rs`
  (run from `field-level-entitlement-filter-flex/`).
- Pure logic lives in `src/routing.rs`, `src/entitlement.rs`, `src/claims.rs`,
  `src/cdgc.rs` with unit tests; `src/lib.rs` is the gateway wiring.
- `demo/config.json` and `demo/env.local.sh` contain real credentials and are
  gitignored. Never commit them.
