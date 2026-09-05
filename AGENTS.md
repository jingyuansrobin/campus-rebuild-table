# AGENTS.md

## Read order

Before changing product or architecture behavior, read:

1. `README.md`
2. `docs/architecture/v3-architecture-draft.md`
3. the current vertical-slice document under `docs/implementation/`
4. `docs/migration/v2-to-v3.md`

For campus search work, also read `docs/implementation/v3-v0.2-campus-search.md`.

## Product authority

- Product owner decides user-visible behavior, product scope, privacy/publication semantics, and other irreversible product choices.
- Implementation agents decide code structure, types, tests, refactors, and dependency details unless they materially change those product decisions.

## V3 rules

1. This repository is a clean V3 line, not a continuation of V2 architecture by default.
2. Do not bulk-copy modules from `campus-reconstruction-tool`.
3. Before reusing a V2 capability, audit both its product value and implementation quality. Reuse experience or tests when useful; rewrite or drop poor implementations.
4. Keep core use cases headless and testable without the desktop GUI.
5. Desktop UI must remain a thin shell over application use cases.
6. Arnis is the V3.0 base generation engine; do not rebuild a parallel generic generation engine.
7. Do not add AI/Agent infrastructure in the current V3 scope.
8. Do not add cloud/auth/marketplace complexity before the corresponding milestone requires it.
9. Prefer a few cohesive crates over speculative micro-crate decomposition.
10. Add architecture enforcement tooling only after a recurring failure demonstrates the need.
11. External-provider types must stop at adapter boundaries; `campus-core` owns provider-neutral domain types.
12. Never commit API keys, access tokens, security codes, or other secrets. Runtime credentials come from environment/config outside the repository.

## Development workflow

For a completed vertical slice or non-trivial refactor:

1. branch from a green `main`;
2. iterate on the feature branch with narrow checks as needed;
3. run the full validation set before merge;
4. open a PR and keep `main` green;
5. merge only after CI passes.

Small documentation-only typo fixes may go directly to `main`, but intermediate broken implementation states must not.

## Validation

For Rust changes, normally run:

```text
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Use narrower checks while iterating; use the full set before merging a completed vertical slice.

Live provider calls are not required in normal CI. Provider parsing/request construction must be covered with deterministic tests; manual smoke tests may use runtime credentials outside GitHub Actions.
