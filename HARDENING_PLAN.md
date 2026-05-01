# HARDENING PLAN

Status: active hardening checklist for the prototype gateway.

## Current priorities

1. Keep the default deployment localhost-only.
2. Keep auth parsing compatible with supported local Codex/OpenClaw auth stores.
3. Avoid logging prompts, completions, tool arguments, bearer tokens, or raw auth data by default.
4. Preserve honest upstream errors; do not reintroduce fake-success responses.
5. Maintain a green local verification baseline:
   - `cargo fmt --check`
   - `cargo test`
   - `python3 scripts/check_crate_age.py 7`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo build`
6. Keep OpenAI-compatible and Anthropic-compatible contract changes covered by unit tests.

## Verbose diagnostics

Sensitive wire/body diagnostics are opt-in only:

```bash
CODEX_PROXY_VERBOSE=1 ./target/release/codex-api-gateway ...
```

Verbose diagnostics may include prompts, responses, tool names, tool arguments, and upstream SSE fragments. Do not enable them in shared logs or public bug reports.

## Not yet production-grade

This project is still a local development gateway. Public exposure, multi-tenant use, reverse tunnels, and hosted deployments remain out of scope until a separate threat review is completed.
