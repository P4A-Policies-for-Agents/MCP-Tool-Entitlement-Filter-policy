# field-level-entitlement-filter-flex — policy implementation

The Rust → `wasm32-wasip1` **implementation** half of the split-model
[Field-Level Entitlement Filter](../README.md) policy.

```
src/lib.rs          # CDGC auth (Login→JWT) + ccgf-searchv2 sensitivity derivation,
                    # cache-aside DataStorage + single-flight refresh, request-leg
                    # caller-claim capture, response-leg per-record field projection
src/entitlement.rs  # PURE: caller entitlement decision + mask/nullify/drop — 11 unit tests
src/cdgc.rs         # PURE: JWT nonce + cached field-map types
src/generated/      # config.rs generated from the published definition (do not edit)
```

```bash
make build-asset-files                       # fetch definition by GAV + gen config.rs
cargo build --target wasm32-wasip1 --release # build the wasm
cargo test --lib                             # 11 pure unit tests
make release                                 # publish the wasm implementation to Exchange
```

Requires **PDK 1.10** and the `wasm32-wasip1` target. Reuses the CDGC
`ccgf-searchv2` derivation engine from the sibling Conformance Guard policy; this
policy adds the caller-aware, fail-closed field projection.
