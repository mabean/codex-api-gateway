# Native Responses JSON and HTTP errors — 2026-11-23

Status: observed offline implementation evidence for C2-AC1 and C2-AC2, updated for review round 1 on 2026-11-24; implementation success is separate from orchestrator acceptance. The current repaired suite passes 66 HTTP cases and 89 Rust tests. Sources: [C2 runner](../scripts/codex_responses_c2.py), [66 mandatory cases](../scripts/fixtures/codex_responses/c2_cases.json), [Rust regressions](../src/tests.rs), [harness controls](../scripts/test_codex_responses_c2.py) and [reproduction commands](../BUILD.md#offline-native-responses-json-and-http-errors-c2). No real credentials or live provider requests were used.

## Snapshot and reproduction

The immutable original executable is accepted C1 revision `95b35c79933e268d2333e2d24b6215a940c93978`. The repaired executable was built from that revision plus the uncommitted C2 implementation. Both versions received identical request and upstream fixture bytes; `--expect c1|c2` changes assertions only. Fixture requests use synthetic auth and explicit loopback origins. C2 explicitly reuses C1/P1's OS sandbox, minimal environment, denied external-connect and credential-canary probes, process ownership and cleanup checks. The manifest fingerprints both the C2 runner and shared C1 lifecycle.

| Artifact | SHA-256 |
| --- | --- |
| Immutable accepted-C1 gateway | `977d88f2a156d1e123b14bf792a4fbc8f1cb0e2e08965a88c6f97d951845b394` |
| C2 release before final HTTP-date syntax tightening | `7b6e2fc0518f1283d50c6c9ac1f6d6b7a9154763118a9ea460f9706e2afd9bb8` |
| Initial C2 release reviewed before round 1 | `3af9c4be54c26138494341c60075e15a37a8d70c18467008002f91a1ed730f96` |
| Initial C2 runner | `aa4c69af817f73c0a631574d111b9c8b31ad99f6342e1524d3994ad73e1023c7` |
| Initial shared C1 lifecycle | `7e71c81869503d36d608806d27f5143b69375d50f44db9d1500c7dc1f4ea351b` |
| Original 56 C2 fixtures | `bf4f3e043ad0dd71307f0aba975d4c9f4e6c7133334ef09910e6b089bf6cc3fc` |
| Round-1 repaired C2 release | `6bf34b3dbb179cb359127689d73951339586ad189ad25ec86a60df5f43bd33c4` |
| Round-1 C2 runner | `3bd45f7989e931298e92d43553e5a033a445692b9758d1853062dc6f856f0422` |
| Round-1 shared C1 lifecycle | `9cbf8dcce3f465ad4038b88b0cac5ebb04bb9a8320d2fb5f8e04701392190a0e` |
| Expanded 66 C2 fixtures | `9a7f46a49e203649a599e0bf0f634ac11dc74cd11d4de6b8b80d9570cbca85a7` |

The before-final release passed all 56 C2 cases, intentional-failure cleanup, unchanged 24-case C1 and unchanged 26-case P5 suites. The final refinement only restricts retry dates to exact HTTP date spellings after calendar parsing; Rust tests additionally reject a long weekday in IMF form, a two-digit IMF year, UTC instead of GMT, leading whitespace and an impossible date.

The initial C2 release also passed all 56 original C2 HTTP cases with the same runner, lifecycle and fixture hashes as the accepted-C1 comparison. Its isolation, ordinary-log secret exclusion, owned process cleanup, owned-port closure and unrelated-listener survival receipts were all true. The review counterexamples and round-1 evidence below extend that original coverage.

## Observed differences

| Criterion | Accepted C1 original | C2 repaired behavior |
| --- | --- | --- |
| C2-AC1 native completed/function-call JSON | Reconstructed response ID/model/output and discarded opaque output and usage details; reasoning usage became zero. | Preserves native response/model/item/call IDs, output order and contents, incomplete details and all available usage, including nonzero cached/reasoning details. Missing usage remains absent. |
| C2-AC1 text and sparse compatibility | Canonical reconstruction retained streamed text; stale text could reject the native terminal. | Native top-level text is authoritative. Otherwise ordered output-text blocks supply text, including an authoritative empty block. Sparse terminal output falls back to deltas; missing object becomes `response`. Stale deltas do not replace terminal output. |
| C2-AC1 terminal outcomes | Incomplete JSON became completed; JSON `response.done` failed as a missing terminal. Native failed/error diagnostics disclosed the synthetic secret. SSE event/type mismatch was ignored in the canonical JSON path. | Native incomplete remains incomplete with partial output and reason. Legacy completed terminal and EOF-delimited terminal are accepted through C1 framing. Failed/error/missing/contradictory/mismatched/malformed/oversized outcomes return safe structured protocol errors. |
| C2-AC1 terminal completion | JSON held until fixture EOF, 804 ms in the comparison. | JSON completed in 1 ms while upstream remained open. |
| C2-AC2 upstream HTTP failures, both modes | 400/401/403/404/429 became 502 with legacy codes; no retry hint. | Original 400/401/403/404/429 survives with fixed safe messages and distinct upstream codes; local validation remains 400 `validation_error` without an upstream request. |
| C2-AC2 transport/deadline, both modes | Send/read timeouts and transport failures returned 502. | Typed send/read timeouts return 504 `upstream_timeout`; transport returns 502 `upstream_unavailable`. Four timeout fixtures observe two attempts across the configured five-second per-attempt deadline, approximately 10 seconds total each. |
| C2-AC2 safe retry hint | No retry hint. | Final 429 forwards one decimal or valid IMF/RFC850/asctime HTTP-date hint. Invalid, negative, combined, duplicate and `Connection`-nominated hints are omitted. Other statuses never forward it. |

The same cases assert actual upstream attempt counts: 400/401/403/404 and truncated Content-Length take one attempt; 429/503, transport and timeouts take the configured two attempts. Truncated bodies remain HTTP 502 `upstream_protocol_error`, preserving C1-R1. Ordinary logs and unauthorized response headers are secret-free on both releases. Baseline-only native failed/error disclosure is explicitly asserted as the original defect, and absent on C2. No existing C1/P5 fixture expectations were weakened or removed.

## Accepted review repairs — round 1, 2026-11-24

Reviewed snapshot: `c887e4eeb1b3b6ab4e51913c836d8dd101476d1af777302028ef442eceb1187a`. Plan SHA-256 remains `4f62e90d8e1df0e9cf4e698c4a9ae441246f4152a6cc648e3fe59469133b549e`. Only accepted findings C2-R1 and C2-R2 were repaired. The initial C2 executable and repaired release received the same ten appended fixture inputs, using identical current runner, lifecycle and expanded fixture hashes above. Re-serializing the first 56 fixture records reproduces their original hash exactly; their expectations are unchanged.

| Finding | Initial C2 observation | Repaired observation |
| --- | --- | --- |
| C2-R1 finalized message/function items | Sparse terminal omitted output entirely; finalized message text was empty and native function item was lost. | Recovers native `msg_final` or `fc_final` with all supplied fields when the terminal omits output; message text becomes `final snapshot`. No IDs or usage are synthesized. |
| C2-R1 standalone text and duplicate lifecycle | Standalone text-done became empty text; delta plus done retained only `final `. | Standalone done returns `final snapshot` without fabricating an output item. Delta/done/item snapshots yield the final text once and one native message item. |
| C2-R1 order and terminal authority | Mixed final events lost their content; explicit nonempty terminal output was already authoritative. | Native items retain output-index order `msg_first`, `msg_second`, `fc_final`; text is `first second tail`, including standalone finalized text. Explicit terminal output stays intact, including `[]`; explicit terminal text, including an empty string, stays authoritative. |
| C2-R2 missing nested status | Both completed and incomplete outcomes omitted status. | Defaults status from the validated native event kind to `completed` or `incomplete`, preserving incomplete details. Explicit contradictory/null/failed statuses still fail; native SSE bytes are unchanged. |

The original binary with the repaired expectation exits 1 at `r1_message`, reproducing the loss before repair. With `--expect initial-c2 --repair-only`, all ten original outcomes, including the unchanged terminal-authority control, match their explicitly defective expectations. The repaired `--expect c2 --repair-only` passes all ten. The repaired default C2 run passes all 66 cases. Each repair case makes exactly one upstream attempt; successful before/after runs and the expected regression failure all retain true isolation, secret-free ordinary logs, owned-process cleanup, closed-port and unrelated-listener receipts.

The implementation reuses existing text identity matching for JSON-only per-item/per-part fallback state. Finalized text replaces matching deltas and finalized native items are retained without reconstruction. It recovers native output items only when the terminal output key is absent. Default streaming mode never allocates this collector state. C1 lifecycle changes only forward the optional bounded-case switch to its worker; default C1 behavior and fixture expectations are unchanged.

## Checks and boundaries

Round-1 implementation checks actually run: `cargo fmt --check`; `cargo test --all-targets --locked --offline` (89 passing); `cargo clippy --all-targets --all-features --locked --offline -- -D warnings`; `cargo build --release --locked --offline`; focused C1/C2 Python controls (5 passing); ten-case initial/repaired comparison and expected initial regression failure; repaired full C2 comparison (66 passing); relative documentation links and `git diff --check`. Receipts produced before the interruption were inspected on 2026-11-24; no extra protocol run was needed to finish this report. The full acceptance matrix remains separate orchestrator work.

Before review round 1, implementation checks also included the original 87 Rust tests, 4 Python controls, immutable-C1 comparison (56 passing), initial C2 comparison (56 passing), intentional-failure cleanup, unchanged C1 (24 passing) and unchanged P5 (26 passing). Those historical outcomes do not substitute for acceptance of the repaired snapshot. Full Python discovery, dependency-age lookup and P1/P3/P4 are not claimed as worker-run checks here.

Normal and intentional-failure runs verified owned process termination, owned-port closure and unrelated-listener survival. OS loopback isolation and protected-file denial were verified before any request. Temporary logs and individual receipts remain scratch artifacts; this report preserves the meaningful observations and reproducible hashes.

Only Responses ingress selects the new error envelope and terminal JSON projection. Typed timeouts retain the existing Chat/Messages status, code and diagnostic message mapping. Ordinary Codex send/read/retry logging now records classes/codes instead of raw transport diagnostics. Native streaming error events remain native user data; generated postcommit protocol errors use a fixed safe message and never replay. Credential selection, request conversion, upstream destinations and listener exposure are unchanged. See [security guidance](../SECURITY.md) and [threat assessment](../THREAT_MODEL.md).

The JSON collector uses C1's byte framing, event/type agreement, EOF normalization and 1 MiB frame bound, with a 16 MiB total input bound. It captures terminal fields only in JSON mode; streaming does not accumulate text or response snapshots. Native terminal output remains authoritative, and processing ends at the first accepted terminal without validating trailing transport bytes. Canonical Chat/Anthropic parsing and rendering are unchanged. Live provider entitlement, model-specific support, OAuth refresh and Codex application acceptance remain unverified.
