# Build and verification

## Requirements

- Current stable Rust toolchain with `rustfmt` and `clippy`.
- Python 3 for the dependency-age check.

## Build

```bash
cargo build --release
```

`Cargo.lock` is committed. Use `--locked` when a build must fail instead of updating the lockfile:

```bash
cargo build --release --locked
```

## Local verification

Run the same checks enforced by CI:

```bash
cargo fmt --check
cargo test --all-targets
python3 scripts/check_crate_age.py 7
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

CI runs this baseline on Linux and macOS.

## Live verification

Unit tests do not prove account entitlements or upstream availability. With valid local credentials, run:

```bash
scripts/live_mixed_model_smoke.sh
scripts/live_claude_code_acceptance.sh
```

The live scripts start a temporary localhost gateway, make real upstream calls, and write diagnostic artifacts under `/tmp`. They consume account quota and should not run in ordinary CI.
