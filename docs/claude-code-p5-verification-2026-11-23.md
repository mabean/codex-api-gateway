# P5 incremental Codex SSE verification — 2026-11-23

Status: implementation and local verification; independent acceptance pending. Source: accepted P5-AC1–3 plan, pinned official Claude Code 2.1.329 macOS arm64 SDK, synthetic localhost upstream fixtures and the public provider schemas linked in the [source-to-target inventory](claude-code-contract-inventory-2026-11-20.md#p5-source-to-target-update-2026-11-23). No model requests, live-provider acceptance, deployment or credentials are involved.

## Reproduction and executed layers

```bash
cargo build --release --locked --offline
python3 -B scripts/claude_contract_p5.py --binary /opt/homebrew/bin/claude --gateway target/release/codex-api-gateway
python3 -B -m unittest discover -s scripts -p 'test_claude_contract_p5.py'
```

The runner checks the existing P1/P2 source inventory and the [P5 byte-range profile](../scripts/fixtures/claude_contract/2.1.329-macos-arm64-p5.json). It extracts original `zi.create`, `zt.fromSSEResponse`/`cc`, and `ff.createMessage`/`ff.finalMessage`/`ff.abort`. The original `ff` signal reaches the actual fetch; a fixture socket read verifies upstream EOF after abort and after downstream reader cancellation. Transport, response wrapping, capability hooks and loopback fixture control are adapters. P5 executes neither the CLI's full main loop nor SDK automatic retry; actual gateway retry counts come from upstream HTTP receipts. P3 separately covers original SDK retries.

Each run writes the official binary fingerprint, byte-range provenance, gateway/harness/fixture hashes, actual SDK events/final messages, upstream counts, isolation probes and cleanup observations to its printed temporary evidence directory. Extracted modules and raw traces remain local. The runner rejects unknown fingerprints, unavailable layers and missing/reordered/duplicated cases. External connections are OS-denied, the credential canary is unreadable, redirects fail closed, owned process groups have deadlines, and cleanup verifies closed owned ports plus a surviving unrelated listener. The intentional-failure interface is `--fail-after-start`; it must exit 1 with an intentional error and all three cleanup assertions true.

## Bounded behavior

`CodexSseParser` retains split UTF-8 bytes and arbitrary chunks, handles CRLF/multiline/final SSE frames, and applies a 1 MiB frame / 16 MiB response limit. The same canonical state machine serves buffered conversion. Text snapshots and open-tool snapshots append only the unseen suffix; conflicting text, tool names and arguments fail. Finalized tool arguments are immutable: even a whitespace suffix is rejected before emitting any further delta. A completed response must finalize every started tool after reconciling authoritative terminal output; max-token incompletion retains its partial-tool outcome. A tool block starts and ends once even when final item and terminal output repeat the snapshot.

The Anthropic route owns reqwest's body inside a pull-driven downstream response. There is no detached producer or unbounded channel. The existing request-specific timeout/retry policy, destination, authentication and headers are preserved. Retry is possible only before any Anthropic frame, including `message_start`. After emission, transport errors, explicit upstream failures and deadlines produce SSE errors without replay. Successful terminal frames wait for validated EOF, so duplicate terminals or contradictory trailing data cannot follow an already emitted successful stop.

Input/output usage arriving at completion is applied to `message_delta`, which the original accumulator merges into its final usage. The initial required usage fields are provisional zeros; no token estimator is added. Completed text maps to `end_turn`, tools to `tool_use`, and documented `response.incomplete` / `max_output_tokens` to `max_tokens`. Unknown incomplete reasons, malformed data, missing terminal, failed/error events, duplicate terminal and deadline reject `finalMessage` without a successful stop. Counter values beyond the existing unsigned 32-bit representation fail explicitly.

## Coverage and source boundaries

The [26 mandatory cases](../scripts/fixtures/claude_contract/p5_cases.json) cover decoded text and tool fragments behind a closed terminal barrier; post-emission text/tool errors and the start-only retry boundary; exact late 17/11 usage and terminal counts; max-token incompletion; EOF, upstream error/failed, malformed JSON, contradiction, duplicate terminal and deadline; original abort and downstream cancellation; real pre-emission 503 retry; full direct Anthropic extended events; byte-for-byte raw Responses SSE; ordinary Chat JSON/SSE and Responses JSON.

The public Responses schema documents reasoning-summary text, annotations/citations and a compaction notice without summary content. Their actual emission by the Codex account route remains unverified. These source fields do not provide Claude thinking signatures, citation provenance/target document coordinates, or Anthropic compacted content. P5 preserves them on raw Responses SSE and does not synthesize extended Anthropic blocks. Direct Anthropic synthetic thinking/signature, citation and compaction events are compared in full and accumulated by the original consumer. This proves forwarding and consumer behavior for the pinned client, not upstream entitlement or cross-provider semantic equivalence.

## Initial snapshot results

Before repair round 1, the identical 20-case P5 suite returned **20/20 passing** on the completed release, with 21 actual upstream HTTP requests (the pre-emission 503 case contributes the extra request). The immutable P4-base binary returned 1, with **13 failing and 7 passing** cases. Failures exposed missing early text/tool delivery, replay boundaries, incorrect limit/terminal handling, zero usage, duplicate success, cancellation and disconnect. The direct extended-field, raw Responses and ordinary OpenAI controls remain passing on both binaries.

| Check | Observed result |
| --- | --- |
| `cargo fmt --check` | Passed |
| `cargo test --all-targets --locked --offline` | 80 passed |
| Python unittest discovery (`test_*.py`) | 33 passed |
| P5 control unittest discovery | 3 passed; missing/reordered/duplicate cases, missing expectations and byte-span drift fail closed |
| `python3 scripts/check_crate_age.py 7` | 227 registry crates checked; no violations |
| `cargo clippy --all-targets --all-features --locked --offline -- -D warnings` | Passed |
| `cargo build --release --locked --offline` | Passed |
| P1 / P3 / P4 local extracted SDK regressions | Passed: 2 / 15 / 31 outcomes |
| P5 completed release | Passed: 20 outcomes; 21 upstream requests |
| P5 immutable base | Expected nonzero: 13 failures demonstrate repaired gaps |
| Intentional-failure cleanup wrapper | Passed: underlying exit 1, intentional error, owned process cleanup, owned ports closed, unrelated listener alive |
| Relative documentation links / `git diff --check` | Passed |

Official client SHA-256: `8c1729568b19cdadedd0a978bf692f9f279ef554c8a333c4e132b94fd8725550`. Immutable original gateway SHA-256: `c04b47ec27e81148b016af2727e5ac15efdeabe3a2bee229e9c73fe3bdf00df6` (base `da8ba48`). Pre-round release SHA-256: `1648f64134be3f9a455af635b9841d0a3dcc87008ae86eae152074354e942d65`.

Local evidence directory identifiers: P5 repaired `claude-contract-p5-1r23cgf5`, original `claude-contract-p5-m2rpilqf`, intentional failure `claude-contract-p5-89zie1gf`; P1 `claude-contract-p1-jqs2olja`, P3 `claude-contract-p3-9r1ha2gu`, P4 `claude-contract-p4-46oasf7r`. Each contains its source/gateway/harness manifest and structured results. Positive, original and intentional-failure runs all recorded true owned-process/port cleanup and unrelated-listener survival. Raw diagnostics remain temporary; this dated summary preserves the useful redacted results.

Remaining limits: independent acceptance is pending; live Codex event availability, provider credentials/entitlement, beta support, the complete CLI loop and other Claude Code builds are unverified. The 1 MiB/16 MiB limits and terminal EOF confirmation are deliberate bounded streaming behavior. No authored tool-execution gate, direct Anthropic behavior, OpenAI endpoint schema or transport policy was expanded.

## Repair round 1 — P5-R1 and P5-R2

Review source: snapshot `8be82530864e82f853e6ac850870a9c8f6ab7576edf7bc72a362cbd266a98e09`, accepted plan `e35ff8c852c699a1a5994510fe30114d1b51f5139cb20393523fa16bb0bf3dfd`. Status: repaired and locally verified; independent re-review pending.

- **P5-R1:** `response.completed` previously accepted a started tool without finalized arguments or an authoritative terminal output snapshot. The parser now reconciles terminal output first, then requires every started tool to be finalized before a completed response. Legitimate terminal-only snapshots still supply the suffix and close the tool. `max_output_tokens` incompletion retains its `max_tokens` outcome with partial tool input.
- **P5-R2:** a later final snapshot previously appended a suffix after `content_block_stop`. The parser now rejects any changed arguments for a finalized tool before producing a delta. Identical final snapshots remain deduplicated, and legitimate suffixes remain supported while the tool is open.

Six original extracted-SDK cases extend the suite to 26: unfinished completion; post-finalization whitespace; post-finalization second JSON value; simultaneously open parallel tools; authoritative terminal-only tool snapshot; partial-tool max-token incompletion. The consumer checks both rejected `ff.finalMessage` outcomes and the absence of deltas after block stop or successful message termination. The parallel control asserts that both tool blocks start before either stops.

The identical final suite on the preserved pre-round release returned **23 passing / 3 failing**. All three new failure assertions exposed actual successful SDK completion: `unfinished_tool`, `finalized_tool_whitespace`, and `finalized_tool_second_json`. The repaired candidate returned **26/26 passing**, with 27 real upstream requests. All original 20 cases and successful parallel/snapshot/max-token controls pass. Both runs recorded true owned-process cleanup, closed owned ports and unrelated-listener survival.

Pre-round binary SHA-256: `1648f64134be3f9a455af635b9841d0a3dcc87008ae86eae152074354e942d65`. Repair candidate SHA-256: `8aeabfbb22481b674f2d531f8f68a3d1ba87f4ba876dae46e41698d0890b82b7`. Local structured evidence identifiers: before `claude-contract-p5-j3lbrxtb`, candidate `claude-contract-p5-qxw6rhx5`, P4 regression `claude-contract-p4-oo39xhyp`.

Round-1 checks: fmt, **82 Rust tests**, **33 Python tests**, three P5 controls, strict clippy, offline release build and P4's **31 outcomes** passed. Documentation links and whitespace checks passed. Dependency files and outbound/credential policies are unchanged; the initial 227-crate age result above remains applicable. Live-provider checks remain outside this repair.

The same updated 26-case suite also ran against the immutable original P4-base binary (`c04b47ec…`): expected exit 1, **19 failing / 7 passing**, with all cleanup assertions true. Local evidence identifier: `claude-contract-p5-hcomxs62`. Historical P2 expectations were not changed.
