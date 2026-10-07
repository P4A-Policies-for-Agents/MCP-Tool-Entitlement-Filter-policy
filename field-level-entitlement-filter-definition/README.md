# field-level-entitlement-filter — policy definition

The Exchange **policy definition** (schema) half of the split-model
[Field-Level Entitlement Filter](../README.md) policy: `gcl.yaml` (config schema,
category `Security`, `assetTypes: mcp,rest,http`), `exchange.json` (GAV), and the
publish `Makefile`.

```bash
make release        # publish definition to Exchange (current org)
make publish        # publish a -DEV definition
```

The Rust implementation fetches this definition by GAV (`make build-asset-files` in
`../field-level-entitlement-filter-flex`) and generates `src/generated/config.rs`
from it. Bump `version` in `exchange.json` (and the matching
`definition_asset_id.version` in the flex `Cargo.toml`) to re-release.
