# AGENTS.md

## Read order

Before changing product or architecture behavior, read:

1. `README.md`
2. `docs/architecture/v3-architecture-draft.md`
3. `docs/implementation/v3-v0.1-vertical-slice.md`
4. `docs/migration/v2-to-v3.md`

## Product authority

- Product owner decides user-visible behavior, product scope, privacy/publication semantics, and other irreversible product choices.
- Implementation agents decide code structure, types, tests, refactors, and dependency details unless they materially change those product decisions.

## V3 rules

1. This repository is a clean V3 line, not a continuation of V2 architecture by default.
2. Do not bulk-copy modules from `campus-reconstruction-tool`.
3. Reuse V2 code only after proving it directly supports the current V3 vertical slice.
4. Keep core use cases headless and testable without the desktop GUI.
5. Desktop UI must remain a thin shell over application use cases.
6. Arnis is the V3.0 base generation engine; do not rebuild a parallel generic generation engine.
7. Do not add AI/Agent infrastructure in the current V3 scope.
8. Do not add cloud/auth/marketplace complexity before the corresponding milestone requires it.
9. Prefer a few cohesive crates over speculative micro-crate decomposition.
10. Add architecture enforcement tooling only after a recurring failure demonstrates the need.

## Validation

For Rust changes, normally run:

```text
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Use narrower checks while iterating; use the full set before merging a completed vertical slice.
