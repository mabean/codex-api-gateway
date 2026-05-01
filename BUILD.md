# BUILD

## Local build
```bash
cargo build --frozen
```

## Local tests
```bash
cargo test --frozen
```

## Optional lint
```bash
cargo clippy --all-targets --all-features -- -D warnings
```

## Current verification baseline
As of 2026-05-02, the repo has been checked locally with:
- `cargo fmt --check`
- `cargo test`
- `python3 scripts/check_crate_age.py 7`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo build`

## Notes
- `Cargo.lock` should be committed and kept current
- prefer reproducible builds (`--frozen`) in CI and local verification
- CI enforces a minimum dependency age policy of 7 days for crates pulled from crates.io
- if vendoring is introduced later, document the exact vendor workflow here
