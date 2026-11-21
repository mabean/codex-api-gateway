# Direct Anthropic forwarding verification — 2026-11-21

Status: implemented and locally verified; review/acceptance remains separate. Scope: P3-AC1–3, gateway base `f29e2dc`, pinned official Claude Code 2.1.329 macOS arm64 SHA-256 `8c1729568b19cdadedd0a978bf692f9f279ef554c8a333c4e132b94fd8725550`. No live provider, model, real OAuth or CLI session was called. [P2 inventory](claude-code-contract-inventory-2026-11-20.md) remains historical; [BUILD.md](../BUILD.md) supplies reproducible commands and cleanup verification.

## Changes and intentional mutations

Direct Anthropic requests keep generic JSON rather than serializing the narrower Codex ingress message type. System entries stay in the message array with their original chronology, `clear_at`, nested `output_config`, cache markers and tool references. Model aliases are canonicalized, the first applicable gateway model directive is removed, and the existing Agent model-routing instruction is appended when Agent is declared. These intentional mutations and existing auth precedence remain supported. Codex conversion and its historical P2 assertions are unchanged.

OAuth Messages preserve an incoming billing-attribution block and its position/cache metadata; only callers without attribution receive the legacy billing/CLI prefix. Token-count requests use the same auth/model route without Messages prompt augmentation. Codex token counts return the existing structured 404 with no upstream traffic. Only `beta=true` is forwarded from the query. Existing required API/OAuth betas are added alongside incoming flags; the context-management beta is never synthesized after the client removes it.

Upstream status, body bytes and streaming are preserved. Response headers are allowlisted to `content-type`, `Retry-After`, `retry-after-ms`, `x-should-retry`, `request-id` and `anthropic-ratelimit-unified-*`. `Connection`-nominated headers are excluded even if allowlisted; arbitrary headers/cookies and hop-by-hop headers are excluded.

## Executed code and bounded seams

The [P3 profile](../scripts/fixtures/claude_contract/2.1.329-macos-arm64-p3.json) records module hashes, raw-byte ranges, absolute binary positions and range hashes. The runner validates these plus the complete frozen P2 inventory before execution. Extracted proprietary source stays temporary. Each result names its executed layer and gateway path; only object-key order is stabilized in JSON comparisons, while array order, unknown fields, nested values and IDs remain visible.

| Original code | Module / half-open raw-byte range | Adapter boundary |
| --- | --- | --- |
| SDK `makeRequest` | 404 / 173473–176239 | Request preparation, local fetch, status-error objects and logging; original retry recursion executes |
| SDK retry decision/delay methods | 404 / 177495–178435 | `Ut` records bounded milliseconds without sleeping; repeated requests are actual HTTP |
| Main turn shaper `SXn` | 720 / 3079373–3079721 | Synthetic conversation/control maps |
| Main rejection predicates and regex | 720 / 2239399–2240158 and 2239321–2239398 | Synthetic error class and beta registry; unrelated auth/provider conditions disabled |
| Main removal closure `lUo` | 720 / 3223688–3224206 | Controlled closure bindings; telemetry and unrelated feature hooks disabled |
| Main context request-field gate | 720 / 3202055–3202104 | Evaluated with the actual remaining registry after removal |

The original `zi.create`, `zi.countTokens`, SSE decoder `zt/cc` and accumulator `ff` are already included in the fingerprinted P1 SDK closure. Context tests execute the original rejection/removal/gate pieces, not the entire main retry loop. Catalog verification is a local HTTP regression, not execution of the CLI discovery cache. Synthetic tool references prove forwarding, not native ToolSearch execution.

For Codex count fallback, the frozen `main-count` span in the [P2 structured inventory](../scripts/fixtures/claude_contract/p2_inventory.json) is module 720 bytes 1379000–1381100. Original `p8t` returns null for ordinary API errors; its gateway `501` branch can invoke `NLr` sampling through Messages. P3 therefore returns 404 and tests zero upstream traffic. The caller's later local estimate is static evidence, not executed full-client acceptance.

## Before and after evidence

The immutable pre-repair release binary SHA-256 is `d20f0404ecf5ca0266b60b751314b42ccf6f078cc36f3e9a03e006e7a3c519d5`. Final repaired release SHA-256 is `9bcc4283294a5f555551d1b1c52508355b544fb85671386cb157be6d5d8a99dc`. Temporary artifact basenames below identify local evidence under the runner's printed temporary root; raw source/logs are deliberately untracked.

| Check | Local evidence directory | Observation |
| --- | --- | --- |
| Immutable baseline, final suite | `claude-contract-p3-doev4ueo` | Expected nonzero positive-suite result: scoped controls hoisted/lost, old OAuth prefix duplicated, query/retry headers lost, Anthropic count missing |
| Repaired final suite | `claude-contract-p3-ecr0no8d` | All 15 positive cases required; per-case expected/actual, path, layer, wire and receipt in `result.json`; harness/client/gateway hashes in `manifest.json` |
| Intentional failure cleanup | `claude-contract-p3-3t38ewbd` | Child exit 1, evidence line first, stderr present, intentional error recorded; wrapper exit 0 |
| P1 regression | `claude-contract-p1-i_uts8r3` | Exact terminal text and malformed-event-order rejection pass |

Baseline and repaired runs both verify OS network denial, protected credential-file denial, redirect rejection and owned cleanup. All owned processes/ports close while the independent listener remains alive. The failure run also records all three cleanup booleans true. Profiles/auth/logs are temporary and synthetic; gateway keychain access is explicitly disabled in P3 in addition to OS denials.

## Criterion evidence and checks

- **P3-AC1:** Four SDK/SXn cases compare the complete upstream JSON for API-key and synthetic OAuth first/follow-up requests. Cache metadata, tool schemas/references/IDs, scoped controls, nested effort, output format and a single first client attribution survive. Focused Rust regressions also cover extension fields and preserved intentional Agent/directive behavior.
- **P3-AC2:** Original SDK orchestration denies a `429` retry in one HTTP attempt; allowed `429` cases each make exactly two attempts and select 125 ms over 9 seconds, then 275 ms over 1 second. Raw error bytes/status and header policy are checked. Original context rejection closure and gate retry once without field/beta while keeping the other beta; unrelated `400` makes one attempt. The SSE consumer receives text while the terminal barrier remains closed, then exact final text, stop reason and nonzero usage.
- **P3-AC3:** Original `zi.countTokens` returns exactly `{"input_tokens":37}` through API-key/OAuth aliases, exact beta query and matching auth, without prompt augmentation. Codex gives structured 404 with zero upstream requests. Catalog and ordinary Messages JSON/SSE controls pass.

Executed checks: `cargo fmt --check`; 71 Rust tests via `cargo test --all-targets --locked --offline`; 27 Python tests via unittest discovery; `cargo build --release --locked --offline`; `cargo clippy --all-targets --all-features --locked --offline -- -D warnings`; P3 suite; intentional-failure wrapper; P1 regression; relative documentation links and `git diff --check`. The separate crate-age check queried all 227 registry crates and found no seven-day-policy violations; its initial restricted-network attempt skipped metadata and was not used as age proof.

Remaining limits: macOS arm64/pinned binary only; no real provider beta support, account entitlement, real credential refresh, main CLI dialogue/cache execution or P4/P5 acceptance. Existing localhost/single-user deployment scope is unchanged.
