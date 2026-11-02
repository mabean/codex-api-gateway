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
python3 -B -m unittest discover -s scripts -p 'test_*.py'
python3 scripts/check_crate_age.py 7
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

CI runs this baseline on Linux and macOS.

The live-runner evidence regressions require no credentials or quota. They reject wrong-file, stale, failed, missing, and prematurely issued Claude tool read-backs, and invalid Chat SSE terminal reasons.

The dependency-age check rejects crates published less than seven days ago when their metadata is available. It is best effort: failed crates.io lookups or missing publication dates produce warnings and skip those packages, so a successful exit does not prove that every dependency was checked.

For documentation-only changes, verify repository-relative file links and run `git diff --check`.

## Live verification

Unit tests do not prove account entitlements or upstream availability. Live suites require Bash, curl, Python 3, and credentials for the selected upstreams. The Claude Code acceptance suite also requires the `claude` executable in `PATH`.

Both scripts use [live_gateway_checks.py](scripts/live_gateway_checks.py), build the release binary, start an owned temporary localhost gateway, and continue after individual failures. They default to nine advertised IDs covering the current GPT-6 and Claude families, including GPT-6.1 Terra and Claude Haiku 5.5. The mixed suite checks JSON and SSE text plus complete tool-use round trips on `/v1/messages`; GPT models also run these checks on `/v1/responses` and `/v1/chat/completions`. It also probes default aliases and environment overrides. The Claude Code suite checks an actual `Read` → `Edit` → `Read` sequence and a `Bash` round trip, including successful tool results bound to their call IDs and a successful terminal response.

```bash
scripts/live_mixed_model_smoke.sh
scripts/live_claude_code_acceptance.sh
# Select a subset or adjust time limits (seconds):
MODELS="gpt-6.1-sol claude-sonnet-5-5" REQUEST_TIMEOUT=90 CLAUDE_TIMEOUT=180 scripts/live_claude_code_acceptance.sh
```

Gateway wire diagnostics are disabled by default. Set `CODEX_PROXY_VERBOSE=1` only when necessary; this can capture prompts, responses, and tool data. Even with diagnostics disabled, the suites save request/response artifacts and Claude Code transcripts in a unique temporary directory. The printed JSON rows and `summary.json` contain per-scenario outcomes; any failure or blocked scenario makes the suite exit nonzero. Authentication failures block dependent upstream scenarios, and model availability failures block later scenarios for that model. Inspect and redact artifacts before sharing them. `OUT_DIR` can select an artifact directory, and `PORT` can select a dedicated free port (otherwise the suite chooses a free localhost port). The suite never removes an existing directory or terminates unrelated services.

Live suites consume account quota. Unit tests and `/health` do not establish upstream access. Preserve a dated, redacted report for meaningful acceptance runs. The [2026-10-05 model verification report](docs/model-verification-2026-10-05.md) records earlier acceptance scope and limitations; the [2026-11-02 catalog verification](docs/model-verification-2026-11-02.md) records the latest catalog refresh and local checks.

The Claude Code acceptance suite runs with `--dangerously-skip-permissions` inside temporary workspaces and a bounded timeout/turn count. This bypasses Claude Code permission prompts; run the suite only in a trusted test environment.
