# Project map

Use this index to find documentation and implementation entrypoints. Open only the files relevant to the task.

## Documentation

| Task | Entry point |
| --- | --- |
| Understand the product, run it, configure clients, models, and authentication | [README.md](README.md) |
| Build, choose verification checks, and review live-suite prerequisites and execution modes | [BUILD.md](BUILD.md) |
| Handle credentials, logs, outbound destinations, and security-sensitive changes | [SECURITY.md](SECURITY.md) |
| Assess deployment scope, browser access, credential limitations, and other trust boundaries | [THREAT_MODEL.md](THREAT_MODEL.md) |
| Review the current model catalog, release-date sources, and refresh validation | [2026-11-18 catalog verification](docs/model-verification-2026-11-18.md) |
| Review the Terra and Haiku catalog update | [2026-11-02 catalog verification](docs/model-verification-2026-11-02.md) |
| Review earlier local approval, live results, and remaining blockers | [2026-10-05 verification](docs/model-verification-2026-10-05.md) |
| Review Claude Code 2.1.289 request changes and mock compatibility | [2026-10-05 protocol comparison](docs/claude-code-protocol-2026-10-05.md) |
| Run the offline extracted Claude Code SDK contract and inspect its limits | [P1 verification](docs/claude-code-p1-verification-2026-11-20.md), [runner](scripts/claude_contract_p1.py) |
| Verify native Responses incremental SSE, terminal outcomes, cancellation and retry offline | [C1 verification](docs/codex-responses-c1-verification-2026-11-23.md), [runner](scripts/codex_responses_c1.py), [cases](scripts/fixtures/codex_responses/c1_cases.json) |
| Verify native Responses request validation, function replay and configured URL normalization offline | [C3 verification](docs/codex-responses-c3-verification-2026-11-24.md), [runner](scripts/codex_responses_c3.py), [cases](scripts/fixtures/codex_responses/c3_cases.json) |
| Verify native Responses terminal JSON and safe precommit HTTP errors offline | [C2 verification](docs/codex-responses-c2-verification-2026-11-23.md), [runner](scripts/codex_responses_c2.py), [cases](scripts/fixtures/codex_responses/c2_cases.json) |
| Verify incremental Codex SSE, actual cancellation, terminal usage and extended-event boundaries offline | [P5 verification](docs/claude-code-p5-verification-2026-11-23.md), [runner](scripts/claude_contract_p5.py), [cases](scripts/fixtures/claude_contract/p5_cases.json) |
| Verify Codex effort, terminal refusals and native deferred tool discovery offline | [P4 verification](docs/claude-code-p4-verification-2026-11-21.md), [runner](scripts/claude_contract_p4.py), [cases](scripts/fixtures/claude_contract/p4_cases.json) |
| Verify direct Anthropic forwarding, original SDK retries and count-tokens offline | [P3 verification](docs/claude-code-p3-verification-2026-11-21.md), [runner](scripts/claude_contract_p3.py), [positive cases](scripts/fixtures/claude_contract/p3_cases.json) |
| Inspect the pinned Claude Code contract and reproduce six baseline diagnostic families | [P2 inventory](docs/claude-code-contract-inventory-2026-11-20.md), [structured records and byte spans](scripts/fixtures/claude_contract/p2_inventory.json), [runner](scripts/claude_contract_p2.py), [dated comparison](docs/claude-code-protocol-2026-11-20.md) |
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
