# Project map

Use this index to find documentation and implementation entrypoints. Open only the files relevant to the task.

## Documentation

| Task | Entry point |
| --- | --- |
| Understand the product, run it, configure clients, models, and authentication | [README.md](README.md) |
| Build, choose verification checks, and review live-suite prerequisites and execution modes | [BUILD.md](BUILD.md) |
| Handle credentials, logs, outbound destinations, and security-sensitive changes | [SECURITY.md](SECURITY.md) |
| Assess deployment scope, browser access, credential limitations, and other trust boundaries | [THREAT_MODEL.md](THREAT_MODEL.md) |
| Review latest local approval, earlier live results, and remaining blockers | [2026-10-05 verification](docs/model-verification-2026-10-05.md) |
| Contribute changes with a coding agent | [AGENTS.md](AGENTS.md) |

## Source and tooling

| Area | Pointer |
| --- | --- |
| CLI, HTTP handlers, model routing, credential resolution, and request conversion | [src/main.rs](src/main.rs) |
| Canonical stream events, upstream SSE parsing, and response rendering | [src/streaming.rs](src/streaming.rs) |
| Local regression tests | [src/tests.rs](src/tests.rs), [live-runner evidence tests](scripts/test_live_gateway_checks.py) |
| Package and pinned dependency graph | [Cargo.toml](Cargo.toml), [Cargo.lock](Cargo.lock) |
| Linux/macOS verification baseline | [.github/workflows/ci.yml](.github/workflows/ci.yml) |
| Dependency age policy | [scripts/check_crate_age.py](scripts/check_crate_age.py) |
| Shared bounded protocol and Claude Code acceptance runner | [scripts/live_gateway_checks.py](scripts/live_gateway_checks.py) |
| Live mixed-model smoke suite | [scripts/live_mixed_model_smoke.sh](scripts/live_mixed_model_smoke.sh) |
| Live Claude Code acceptance suite | [scripts/live_claude_code_acceptance.sh](scripts/live_claude_code_acceptance.sh) |
| Legacy diagnostics; inspect process termination and defaults before use | [scripts/debug_claude_tool_use.sh](scripts/debug_claude_tool_use.sh) |
