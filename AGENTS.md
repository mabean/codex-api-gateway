# Repository guidance

## Project overview

Codex API Gateway is an experimental Rust gateway for OpenAI-compatible and Anthropic-compatible clients. It routes requests to Codex/ChatGPT and Anthropic upstreams and supports JSON responses, streaming, and tool-use translation. The supported deployment is a single user on localhost.

Read [PROJECT_MAP.md](PROJECT_MAP.md) for the repository layout and documentation entrypoints, then open only files relevant to the task.

## Development setup

- Use stable Rust with `rustfmt` and `clippy`, plus Python 3 for the dependency-age check.
- Build with `cargo build --release`. See [BUILD.md](BUILD.md) for lockfile handling and verification procedures.
- Use the configuration and client examples in [README.md](README.md). Credentials are needed for live upstream calls, not unit tests.

## Testing and verification

For code changes, run the CI baseline documented in [BUILD.md](BUILD.md):

```bash
cargo fmt --check
cargo test --all-targets
python3 -B -m unittest discover -s scripts -p 'test_*.py'
python3 scripts/check_crate_age.py 7
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

- Add regression coverage for changed routing, validation, credential handling, protocol conversion, and streaming behavior. Use synthetic credentials and temporary files in tests.
- Tests that change gateway environment variables must follow the existing `ALIAS_ENV_LOCK` and cleanup pattern in `src/tests.rs`.
- For documentation-only changes, verify relative file links and run `git diff --check`.
- Live smoke and Claude Code acceptance suites are optional, require real credentials, and consume quota. Run them when verifying an upstream-dependent change; report their actual scope and outcome separately from unit tests.
- Inspect diagnostic scripts before running them, especially their process termination and cleanup behavior. Use the live suites listed in [BUILD.md](BUILD.md) as the acceptance entrypoints.

## Code conventions

- Follow the existing Rust formatting, naming, and error-handling patterns; keep changes focused on the requested behavior.
- Keep ingress conversion and model routing explicit. Use the canonical stream events in `src/streaming.rs` for Codex SSE translation rather than duplicating parser or renderer logic.
- Preserve structured errors for each API family. Do not turn upstream failures into successful responses or treat `/health` as proof of upstream access.
- Keep `Cargo.lock` committed and include dependency changes deliberately. CI checks a seven-day minimum crate age on a best-effort basis; crates.io lookup failures are reported as warnings and skipped. See [BUILD.md](BUILD.md).

## Documentation

- Update user-facing behavior in [README.md](README.md), verification procedures in [BUILD.md](BUILD.md), and trust-boundary changes in [SECURITY.md](SECURITY.md) and [THREAT_MODEL.md](THREAT_MODEL.md).
- Keep durable decisions, necessary evidence, and versioned deliverables in the repository with a reachable link from [PROJECT_MAP.md](PROJECT_MAP.md). Update the map when adding, moving, or removing documentation.
- Separate supported behavior from dated observations and proposals. Give observations and proposals a date, status, and source; report only checks actually performed.
- Keep scratch output and raw diagnostics in temporary directories. Preserve only useful, redacted evidence; avoid empty documentation scaffolding and per-dump READMEs.
- Use repository-relative links and portable examples in committed documentation.

## Security

- Keep the listener on `127.0.0.1`; public and multi-tenant deployments are outside the supported scope.
- Never commit credentials, auth-store contents, or unredacted wire logs. `CODEX_PROXY_VERBOSE=1` can expose prompts, responses, and tool data.
- Treat changes to credential resolution, outbound destinations, logging, or listener exposure as security-sensitive. Update the threat assessment and relevant tests.
