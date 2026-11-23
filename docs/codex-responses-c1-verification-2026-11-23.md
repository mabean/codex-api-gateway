# Native Responses streaming verification — 2026-11-23

Status: observed local implementation evidence for C1-AC1 through C1-AC3. This is offline release-binary HTTP evidence with synthetic credentials, not live provider acceptance. Sources: [C1 runner](../scripts/codex_responses_c1.py), [mandatory cases](../scripts/fixtures/codex_responses/c1_cases.json), Rust regression tests, and the commands in [BUILD.md](../BUILD.md#offline-native-responses-streaming-contract-c1).

## Snapshot and reproduction

The immutable baseline was built from `0995e0811933a7e883730a8a95a27d33c28a56ee`. The repaired binary was built from that revision plus the uncommitted C1 implementation. No credentials, upstream inference or external service calls were used. The runner reused P1's `sandbox_policy` and `minimal_env`, proved external egress and credential-canary reads were OS-denied, and checked only owned services were terminated.

| Artifact | SHA-256 |
| --- | --- |
| Immutable baseline gateway | `00472792d7461e1627ad6edb9ce2db2e4d311ec7404b61dbded8db866ef215cf` |
| Initial C1 release before review repairs | `3e9c8799e60b722bc94f4b2b42c65c96d5295f8d527a37fe1d83475bc73bcf4f` |
| Repaired release gateway, review round 1 | `977d88f2a156d1e123b14bf792a4fbc8f1cb0e2e08965a88c6f97d951845b394` |
| C1 runner, review round 1 | `94edc8ce8503535b445184fd262ed7833aae2ae4f06405f6856f026c2e134fec` |
| C1 cases, review round 1 | `6de500ce5219c805cbb4d65909bda1006da29b4e011ea856c7f8a9f42b6b80bc` |

Run `--expect baseline` against the immutable binary, then `--expect c1` against the repaired release, as documented in BUILD. The identical barriers decode JSON deltas while the upstream terminal remains blocked; the runner also requires the baseline to complete correctly after release. Baseline mode runs the two barrier controls and the truncated-length precommit policy control.

## Observed outcomes

| Criterion | Local evidence |
| --- | --- |
| C1-AC1 | Baseline text and tool deltas stayed buffered through the 800 ms barrier window, then completed after release (806/813 ms total). Repaired text and tool deltas were decoded before release (2/1 ms total). UTF-8, CRLF and multiline data were written in single-byte fragments; ordinary SSE wire bytes, opaque fields and tool `call_id` were preserved. Rust checks also fed one-byte, seven-byte and whole-stream partitions. |
| C1-AC2 | Downstream disconnect closed upstream within 17 ms total. Committed EOF, malformed data, frame overflow and timeout produced explicit `error` with one attempt and no completed event. A precommit 503 retried to success in two attempts; repeated 503 exhausted the configured two attempts with an HTTP error. A truncated Content-Length returned HTTP 502 `upstream_protocol_error` with one attempt on both immutable baseline and repaired release. |
| C1-AC3 | All three legacy `response.done` statuses normalized to corresponding event/type with payload retained. Native completed/incomplete/failed/error stayed distinct. Contradictory terminal status, event/type mismatch, invalid legacy status and malformed JSON failed explicitly. The completed terminal ended downstream and closed an otherwise held-open upstream in 1 ms total. The 1 MiB frame bound failed explicitly. The configured 5-second deadline fired at 5004 ms, before the fixture's 8-second EOF. |

All 24 mandatory C1 cases passed after review round 1. Valid EOF terminals with and without a final newline decoded as completed with only the missing boundary appended. EOF tails with missing or malformed terminals remained explicit errors. The initial implementation's deliberate post-start failure was separately checked: the wrapper accepted only the expected intentional error, and owned process cleanup, owned-port closure and unrelated-listener survival were all true. The round-1 baseline and repaired runs also had all three cleanup receipts true.

Implementation checks actually run: `cargo fmt --check`; `cargo test --all-targets --locked --offline` (85 passing tests after round 1); `cargo clippy --all-targets --all-features --locked --offline -- -D warnings`; `cargo build --release --locked --offline`; focused Python harness controls (2 passing tests); and `git diff --check`.

## Accepted review repairs — round 1

The reviewed snapshot was `c0554330b1ad9d79aefe6933aab79635711097d5dadd0e0202bc4db7b0f8a671`. Both counterexamples were repeated against the frozen initial C1 executable and repaired release with the same synthetic loopback fixture under P1's OS sandbox.

- **C1-R1:** The initial implementation classified every precommit body-read failure as unavailable, broadening retries. The shared body-error classifier now retains the previous timeout/non-timeout distinction, error messages, logs and retry predicate for both buffered and native paths. Before repair, truncated Content-Length produced HTTP 200 after two upstream attempts; after repair it produced HTTP 502 `upstream_protocol_error` after one attempt. The strengthened mandatory harness fails against the initial C1 binary and passes against both the immutable original baseline and repaired release for this case. The case keeps its historical `precommit_retry` ID, with its incorrect expectation corrected.
- **C1-R2:** The initial native parser did not dispatch accumulated EOF data. Its `finish` now consumes a pending final line, validates and dispatches the terminal, and appends only the missing SSE boundary. Before repair, a valid completed event with one trailing newline became a missing-terminal error; after repair it remained a completed event with its input bytes preserved and one newline appended. Release fixtures also cover no final newline and invalid EOF tails. Rust coverage checks native and legacy completed/incomplete/failed terminals, extension payloads, one-byte fragmentation and malformed/contradictory/nonterminal EOF data.

## Scope and remaining acceptance

Only public `/v1/responses` with `stream:true` selects the new pull-based native stream. Other routes and nonstream Responses retain their existing implementations. The shared canonical parser change only extracts its existing complete-byte-line operation; P5 source, fixtures and expectations were not changed. P1/P3/P4/P5, the full Python suite and dependency-age validation remain separate orchestrator acceptance checks; they are not claimed as implementation-run results here.

The native route preserves ordinary complete frames byte-for-byte. Legacy normalization changes terminal JSON `type` and the SSE event field, preserving response and extension values. EOF-delimited terminals retain their input bytes with the missing boundary appended. Generated errors are additional explicit frames. Processing stops at the first accepted terminal; trailing transport bytes are not consumed or validated. Native frames have a 1 MiB bound and remain subject to the existing total upstream request deadline. The canonical translated routes retain their existing limits and EOF behavior. Credential resolution, destination configuration, listener exposure and logging policy did not change, so no trust-boundary document change was needed.

Live account access, model support, OAuth refresh and Codex application behavior are unverified by C1. Later compatibility slices are outside this change.
