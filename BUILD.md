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

The dependency-age check rejects crates published less than seven days ago when their metadata is available. It is best effort: failed crates.io lookups or missing publication dates produce warnings and skip those packages, so a successful exit does not prove that every dependency was checked.

For documentation-only changes, verify repository-relative file links and run `git diff --check`.

## Live verification

Unit tests do not prove account entitlements or upstream availability. Live suites require Bash, curl, Python 3, and credentials for the selected upstreams. The Claude Code acceptance suite also requires the `claude` executable in `PATH`.

Both scripts enable gateway wire diagnostics by default (`CODEX_PROXY_VERBOSE=1`), which may capture prompts, responses, and tool data. To disable those diagnostics, run:

```bash
CODEX_PROXY_VERBOSE=0 scripts/live_mixed_model_smoke.sh
CODEX_PROXY_VERBOSE=0 scripts/live_claude_code_acceptance.sh
```

The live scripts start a temporary localhost gateway, make real upstream calls, and write diagnostic artifacts under `/tmp`. They consume account quota and should not run in ordinary CI.

The Claude Code acceptance suite runs tool-use scenarios with `--dangerously-skip-permissions` inside temporary workspaces. This bypasses Claude Code permission prompts; run the suite only in a trusted test environment. Disabling gateway wire diagnostics does not disable the suite's Claude Code transcripts or test artifacts. Inspect and redact artifacts before sharing them.
