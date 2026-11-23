# Claude Code contract inventory — 2026-11-20

P4 update, 2026-11-21: historical P2 Codex gap statements below remain baseline observations. Current translation supports effective low/medium/high/xhigh effort, correlated supported deferred schemas and validated tool selection. Temporary scope, mid-history system content, unsupported output/context controls and invalid references receive terminal structured refusals. [P4 verification](claude-code-p4-verification-2026-11-21.md) identifies original native ToolSearch, eligibility, history, validation and permission layers and the synthetic executor boundary. P5 streaming/usage gaps remain outside P4.

P4 repair round 1: original module 720 `_Bn` at raw bytes `[2087425,2087596)` builds the exact retain-all thinking setting accepted as a semantic no-op. Its pinned execution is part of legacy Read first/followup acceptance; any added field/edit or changed retention remains refused. Discovery authorization and Messages parallel control use typed internal state skipped by public JSON deserialization. Raw OpenAI flags cannot expand the four executor tool names, and public Chat parallel behavior stays unchanged. See the P4 report for discriminating R1/R2 regressions.

Status: P2 baseline diagnostics, prepared for review. P3–P5 criteria are mapped below and do not pass by virtue of this inventory. Source: local official Claude Code 2.1.329 macOS arm64, binary SHA-256 `8c1729568b19cdadedd0a978bf692f9f279ef554c8a333c4e132b94fd8725550`, gateway baseline `cd2276a`, unchanged production source in P1 `4b6ae67`.

P3 update, 2026-11-21: the descriptions below remain the frozen P2 baseline. The [separate P3 verification](claude-code-p3-verification-2026-11-21.md) records the direct Anthropic repairs and positive extracted tests for P3-AC1–3, including original SDK retry recursion, controlled context rejection/removal and countTokens. P2 known-gap assertions must run against the preserved baseline, not the repaired gateway. The [P3 range manifest](../scripts/fixtures/claude_contract/2.1.329-macos-arm64-p3.json) identifies its additional executed byte spans. This update makes no P4/P5 acceptance claim.

The [structured inventory](../scripts/fixtures/claude_contract/p2_inventory.json) uses stable `records[].id` and `spans[].id` anchors. The runner checks all span hashes directly against raw module bytes; positions are half-open byte ranges, relative to the module, with absolute binary starts recorded separately. No decoded character offsets or reformatted scratch sources are used. The full Bun table contains 3,657 records; the scan examines every JavaScript module for all listed base-URL, endpoint and transport markers. Marker hits are candidates, not proof of reachable invocation. The [unchanged dated comparison report](claude-code-protocol-2026-11-20.md) supplies the 2.1.289 comparison; that earlier CLI/mock evidence was not rerun as part of P2.

## Evidence levels and limits

`static` identifies explicit callers/returns and their predicates, `SDK runtime` executes the pinned Messages SDK, `original SXn` executes the main client turn shaper, and `original retry methods` executes the exact SDK decision and delay methods. `sdk_only`, `server_conditioned_and_unresolved_dynamic` and `unavailable` remain separate statuses. The tests do not execute native ToolSearch, the full CLI dialogue loop, automatic SDK transport retry, real OAuth, live providers or remote flag assignments. No CLI substitution or quota-consuming model call is used.

The full graph marker scan records endpoint declarations and fetch/baseURL references. Computed endpoint strings, dynamic imports and opaque fetch overrides remain unresolved; an SDK method without a traced CLI caller is not elevated into a client requirement. Telemetry and non-first-party providers are outside this contract.

## Traced contract surfaces

<a id="primary-messages"></a>
### primary-messages

- Activation: First-party provider with configured ANTHROPIC_BASE_URL; main turn.
- Request/events: POST /v1/messages?beta=true; model/messages/system/tools/thinking/output_config and conditional fields.
- Caller → transport → consumer: aL baseURL -> main Hw request -> mg.beta.messages.create -> zi.create -> SDK post -> zt SSE -> main event switch.
- Next state/result: Accumulates assistant blocks/usage; tool_use returns to client tool loop and next messages request.
- Anthropic: Pass-through SSE; typed request reconstruction and system hoisting.
- Codex: Anthropic->Chat->Responses normalization and buffered SSE rendering.
- Proof: static caller + extracted SDK runtime. Source span IDs: `base-url-auth`, `main-request-response`, `sdk-resource`, `sdk-transport`, `sdk-consumer`.
- Assigned criteria: P3-AC1, P3-AC2, P4-AC3, P5-AC1, P5-AC2. Remaining condition: None for the bounded local observation.

<a id="auxiliary-messages"></a>
### auxiliary-messages

- Activation: Side query model helper; token estimation sampling; compaction/other query sources.
- Request/events: Nonstreaming Messages; max_tokens and query-source-specific system/tools/effort.
- Caller → transport → consumer: side yi.beta.messages.create; count NLr -> beta.messages.create; auxiliary R.beta.messages.create -> SDK post.
- Next state/result: Returns JSON content or usage; nonstreaming fallback parses text JSON if content type is wrong.
- Anthropic: Shared ingress reconstruction; no special auxiliary endpoint.
- Codex: Shared normalization; model selects adapter.
- Proof: static reachable callers; SDK-only runtime does not execute complete helper. Source span IDs: `main-side-query`, `main-count`, `main-aux-fallback`, `sdk-resource`.
- Assigned criteria: P3-AC1, P4-AC3. Remaining condition: Helper-specific features/server flags and model/provider predicates not exercised end to end.

<a id="retry-fallback"></a>
### retry-fallback

- Activation: HTTP errors; context_management/effort/turn-shape rejection predicates; streamed request fallback.
- Request/events: 429 header-controlled retry; nonstreaming stream:false fallback; rejected beta/field removal branches.
- Caller → transport → consumer: SDK makeRequest -> shouldRetry -> retryRequest -> makeRequest; main retry wrapper uses rejection predicates; Q.beta.messages.create stream:false.
- Next state/result: SDK delay priority is retry-after-ms then Retry-After then exponential jitter; high-level fallback can issue changed request.
- Anthropic: 429 status/body retained; retry metadata lost.
- Codex: Codex upstream has own bounded retries; SDK sees gateway response.
- Proof: original SDK predicates/delay executed on direct and gateway 429; high-level fallback static only. Source span IDs: `sdk-transport`, `main-rejection-predicates`, `main-aux-fallback`, `main-request-response`.
- Assigned criteria: P3-AC2. Remaining condition: Automatic SDK transport retry and main context-management removal are future P3 execution checks; current delay test uses counting makeRequest adapter.

<a id="count-tokens"></a>
### count-tokens

- Activation: Not disabled, supported provider/model; p8t token estimator.
- Request/events: POST /v1/messages/count_tokens?beta=true; token-counting beta plus model/messages/tools/thinking.
- Caller → transport → consumer: p8t -> aL -> beta.messages.countTokens -> zi.countTokens -> SDK post.
- Next state/result: Numeric input_tokens returned; exceptions become null; configured gateway 501 can trigger NLr sample then local estimator.
- Anthropic: No count_tokens route: dated report 404.
- Codex: No count_tokens route; client estimation fallback required.
- Proof: static real caller + dated 2026-11-20 report. Source span IDs: `main-count`, `sdk-resource`.
- Assigned criteria: P3-AC3. Remaining condition: P3 must execute extracted countTokens route and verify query/auth/alias. P2 does not claim sample inference is quota free against live providers.

<a id="model-discovery"></a>
### model-discovery

- Activation: CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY, firstParty, ANTHROPIC_BASE_URL, not excluded auth mode.
- Request/events: GET /v1/models?limit=1000; redirect:error; bounded timeout.
- Caller → transport → consumer: jp gating -> ayl -> fetch -> validated data -> cache keyed by base URL -> picker.
- Next state/result: Filters claude/anthropic models and caches picker entries.
- Anthropic: Explicit local models handler.
- Codex: Same shared local catalog handler.
- Proof: static concrete gateway fetch caller. Source span IDs: `gateway-discovery`.
- Assigned criteria: P3-AC3. Remaining condition: Model cache, auth and flag predicates require client-specific isolated execution beyond this P2 SDK diagnostic.

<a id="headers-betas-attribution"></a>
### headers-betas-attribution

- Activation: API key or OAuth and enabled conditional client features.
- Request/events: anthropic-version, anthropic-beta; client attribution system block; cache_control and metadata.
- Caller → transport → consumer: aL credentials/default headers -> request betas -> zi header builder -> transport.
- Next state/result: Response headers feed rate-limit and retry state.
- Anthropic: Version/betas forwarded; OAuth prepends old 2.1.119 attribution before existing block.
- Codex: Headers used at ingress; Codex upstream uses distinct headers.
- Proof: SDK runtime version/beta control; OAuth behavior in dated report/static gateway. Source span IDs: `base-url-auth`, `sdk-resource`, `main-request-response`.
- Assigned criteria: P3-AC1, P3-AC2. Remaining condition: P3 must verify API key and synthetic OAuth first/unique original attribution; no real OAuth claim.

<a id="turn-controls"></a>
### turn-controls

- Activation: Scoped system/statement maps after selected conversation index.
- Request/events: role:system content plus clear_at:next_user_message; effort-only system entry content:[] output_config.
- Caller → transport → consumer: main SXn wire shaper -> main request -> zi -> gateway ingress.
- Next state/result: Chronology maintained by client; next user clears scoped content at server; last applicable effort should override top-level.
- Anthropic: Hoists content; loses clear_at and nested effort.
- Codex: Hoists instructions; reasoning:null; effort-only can return structured 400.
- Proof: original SXn and SDK runtime, both routes and follow-up chronology. Source span IDs: `main-turn-shaper`, `main-request-response`.
- Assigned criteria: P3-AC1, P4-AC1. Remaining condition: None for the bounded local observation.

<a id="tools-discovery"></a>
### tools-discovery

- Activation: Deferred tool selection; advanced tool use enabled.
- Request/events: ToolSearch tool_use -> matching tool_result tool_reference/tool_name; deferred tools schema.
- Caller → transport → consumer: main tool result normalization/rewrite -> next Messages request; extracted SDK serializes synthetic follow-up.
- Next state/result: Found schema available on next turn; permitted client tool executor owns execution.
- Anthropic: Preserves references and Read/NotebookEdit deferred schemas.
- Codex: Reference becomes empty function_call_output; Read schema survives; NotebookEdit filtered.
- Proof: SDK runtime synthetic references; native ToolSearch not executed. Source span IDs: `main-tools-rewrite`, `main-request-response`, `sdk-resource`.
- Assigned criteria: P4-AC2. Remaining condition: Future P4 needs native ToolSearch -> supported tool execution and matched result. NotebookEdit remains outside Bash/Edit/Read/Write boundary.

<a id="extra-codex-fields"></a>
### extra-codex-fields

- Activation: Main conditional output formatting, context management or tool_choice.
- Request/events: output_config.format, thinking, context_management, tool_choice, cache_control.
- Caller → transport → consumer: main request conditional spreads -> SDK passthrough -> typed ingress -> Chat translation.
- Next state/result: Some fields are retained at ingress but absent from Codex representation.
- Anthropic: Typed ingress preserves supported top-level extras; verify exact combinations later.
- Codex: No declared support for untested format/context/tool_choice combinations.
- Proof: static reachable field branches; no new support claim. Source span IDs: `main-request-response`, `sdk-resource`.
- Assigned criteria: P4-AC3. Remaining condition: P4 must execute every newly claimed combination or document structured refusal/product boundary.

<a id="sse-first-fragment"></a>
### sse-first-fragment

- Activation: stream:true with upstream first text before terminal completion.
- Request/events: message_start/content_block_delta then terminal barrier; Codex response.output_text.delta.
- Caller → transport → consumer: zt/cc -> ff event listener and finalMessage; gateway Codex reads full response before canonical rendering.
- Next state/result: Forwarding first delta arrives before release; baseline Codex first delta only after release.
- Anthropic: Positive control streams before closed terminal barrier.
- Codex: Known buffering gap; eventual text confirmed after release.
- Proof: extracted SDK event consumer + explicit closed barrier. Source span IDs: `sdk-consumer`, `main-request-response`.
- Assigned criteria: P5-AC1. Remaining condition: P5 adds cancellation, disconnect, error after first fragment and duplicate prevention; P2 bounded absence is a baseline observation.

<a id="usage-terminal"></a>
### usage-terminal

- Activation: Nonzero input/output/cache counters supplied by fixture.
- Request/events: message_start usage, message_delta usage, stop reason, message_stop; Codex response.completed usage.
- Caller → transport → consumer: zt/cc -> ff full terminal message usage; main parser updates running usage.
- Next state/result: Final message must contain terminal text and expected counters; HTTP 200 insufficient.
- Anthropic: 17 input,11 output,5 cache-read,3 cache-created preserved.
- Codex: Known zero input/output despite 17/11 upstream counters.
- Proof: extracted terminal SDK accumulator runtime. Source span IDs: `sdk-consumer`, `main-request-response`.
- Assigned criteria: P5-AC2. Remaining condition: P5 extends terminal reasons/tool/max_tokens/incomplete/error cases; current synthetic fixture does not prove provider availability of every counter.

<a id="extended-events"></a>
### extended-events

- Activation: Upstream supplies thinking/signatures/citations/compaction/server metadata.
- Request/events: thinking_delta estimated_tokens, signature_delta, citations_delta, compaction_delta and stop metadata.
- Caller → transport → consumer: main event switch validates delta/block type and accumulates citations/thinking; SDK accumulator has own supported subset.
- Next state/result: Main parser preserves supported event fields; source availability limits translated output.
- Anthropic: Raw SSE forwards extended events.
- Codex: Canonical stream lacks corresponding thinking/citation/compaction data; no Anthropic signature synthesis.
- Proof: static main parser and canonical source inspection; runtime unavailable for absent Codex source data. Source span IDs: `main-request-response`, `sdk-consumer`.
- Assigned criteria: P5-AC3. Remaining condition: unavailable: real Codex provider evidence for representable fields; P5 must test available events and document source absences, never fabricate signatures.

<a id="sdk-only-resources"></a>
### sdk-only-resources

- Activation: No concrete CLI caller found during this bounded full-graph endpoint scan.
- Request/events: SDK /v1/models retrieve/list and other administration/resource endpoints.
- Caller → transport → consumer: SDK resource declarations -> get/post; no resolved base-URL CLI activation chain.
- Next state/result: Do not promote SDK method existence into a CLI requirement.
- Anthropic: No support commitment from SDK declaration alone.
- Codex: No support commitment from SDK declaration alone.
- Proof: sdk_only. Source span IDs: `sdk-model-resources`, `sdk-transport`.
- Assigned criteria: P3-AC3. Remaining condition: New concrete callers require inventory extension; full graph marker scan does not resolve all computed/dynamic endpoint strings.

<a id="server-conditioned-dynamic"></a>
### server-conditioned-dynamic

- Activation: Remote flags, alternate provider, socket mode, dynamic imports or computed dispatch.
- Request/events: Conditional additional beta fields and transports; imported fetch overrides.
- Caller → transport → consumer: Main feature predicates and aL provider selection; dynamic dispatch beyond explicit traced chains remains unresolved.
- Next state/result: Record candidate paths without claiming exhaustive dynamic reachability.
- Anthropic: Conditional; no live server assignment evidence.
- Codex: Conditional; no source/provider evidence for new protocol support.
- Proof: server_conditioned_and_unresolved_dynamic. Source span IDs: `base-url-auth`, `main-request-response`, `gateway-discovery`.
- Assigned criteria: P4-AC3, P5-AC3. Remaining condition: Separate provider/account/remote-flag evidence and product decision required. Non-firstParty providers/telemetry are outside compatibility scope.

## Raw-source provenance

All module and span SHA-256 values are in the structured inventory. Executable P2 ranges have their own [pinned profile](../scripts/fixtures/claude_contract/2.1.329-macos-arm64-p2.json); the SDK ranges retain the P1 profile.

| Span ID | Module ID | Module byte range | Absolute start |
| --- | --- | --- | --- |
| `sdk-resource` | 404 | [120899, 123169) | 292768392 |
| `sdk-transport` | 404 | [166200, 180000) | 292813693 |
| `sdk-consumer` | 404 | [98122, 108838) | 292745615 |
| `base-url-auth` | 404 | [330000, 335900) | 292977493 |
| `main-count` | 720 | [1379000, 1381100) | 298773279 |
| `main-aux-fallback` | 720 | [3123900, 3131500) | 300518179 |
| `main-request-response` | 720 | [3199000, 3274000) | 300593279 |
| `main-side-query` | 720 | [3301000, 3314500) | 300695279 |
| `main-turn-shaper` | 720 | [3079373, 3079721) | 300473652 |
| `main-rejection-predicates` | 720 | [2235200, 2245500) | 299629479 |
| `main-tools-rewrite` | 720 | [2062400, 2063800) | 299456679 |
| `gateway-discovery` | 221 | [372900, 376900) | 289889563 |

| `sdk-model-resources` | 404 | [55400, 56100) | 292702893 |

## Offline diagnostics and interpretation

Run the command in [BUILD.md](../BUILD.md#offline-claude-code-baseline-diagnostics-p2). The [case fixture](../scripts/fixtures/claude_contract/p2_cases.json) defines exactly twenty ordered cases and each expected observable outcome. `known_gap` means the expected baseline defect was reproduced; `correct_behavior` means the expected control survived; `unavailable` is a failed mandatory observation with nonzero exit. Unexpected improvements also fail the pinned baseline and require deliberate fixture review. HTTP 200 alone never passes a case.

| Family | Executed client layer | Required observation |
| --- | --- | --- |
| tool_reference | Original SDK serialization and terminal accumulator | Matched ToolSearch use/result IDs, schema ordering and deferred Read/NotebookEdit boundaries on both routes. Synthetic follow-up only; native ToolSearch discovery/execution is a P4 requirement. |
| Nested effort | Original main-client SXn + SDK | Top-level low, nested effective high, both routes and effort-only control; observe nested loss, Codex reasoning:null or structured 400. |
| clear_at | Original SXn + SDK | System control appears after assistant, then optional next user; both routes hoist its text and lose scope. |
| Retry metadata | Original shouldRetry/retryRequest methods | Consume actual direct/gateway 429 bodies/headers; direct deny makes zero adapter calls, allow selects 125ms ahead of Retry-After:9; gateway header loss chooses status/default jitter. Wait and makeRequest adapters are bounded; this is not automatic retry. |
| First SSE fragment | Original SDK event consumer | Fixture flushes text and holds terminal events behind a closed barrier; Anthropic forwarding is a positive control, Codex has no first delta during bounded wait; both eventually yield exact terminal text. |
| Nonzero usage | Original SDK full terminal message | Anthropic final usage is 17 input/11 output and cache 5/3; Codex reports known zeros despite upstream 17/11. Required stop reason and terminal text are checked. |

Transport adapters restrict origins to the two owned numeric loopback origins and reject redirects. The worker uses the P1 sandbox policy, synthetic keys and isolated HOME/config; OS-denied external-network and credential canary probes are mandatory. Cleanup checks owned process and port closure plus a separate listener surviving. Extracted sources, request traces and process logs stay in temporary evidence directories.

## P3–P5 handoff (all pending acceptance)

| Criterion | Baseline finding or control | Remaining proof |
| --- | --- | --- |
| P3-AC1 | [turn-controls](#turn-controls), [headers-betas-attribution](#headers-betas-attribution), [tools-discovery](#tools-discovery) | Preserve system chronology/scope, nested config, cache markers and tool blocks across turns; original unique-first attribution on API key and synthetic OAuth. |
| P3-AC2 | [retry-fallback](#retry-fallback), [sse-first-fragment](#sse-first-fragment) | Header allowlist; original client automatic retry/count/delay precedence; context_management 400 removal and beta removal; raw streaming. |
| P3-AC3 | [count-tokens](#count-tokens), [model-discovery](#model-discovery) | Extracted countTokens request through normalized/authenticated Anthropic route/query, Codex estimation fallback, catalog and JSON/SSE controls. |
| P4-AC1 | [turn-controls](#turn-controls) | Effective last applicable effort, effort-only request and scoped content or structured unsupported refusal. |
| P4-AC2 | [tools-discovery](#tools-discovery) | Native extracted ToolSearch → supported deferred tool → matched result → terminal response; NotebookEdit explicit refusal/fallback; old direct Read control. |
| P4-AC3 | [extra-codex-fields](#extra-codex-fields), [server-conditioned-dynamic](#server-conditioned-dynamic) | Combination tests for every claimed field; product/provider condition for new capabilities. |
| P5-AC1 | [sse-first-fragment](#sse-first-fragment) | First text/tool delta while barrier remains closed; cancellation/disconnect/error and no duplicate fragment after failure. |
| P5-AC2 | [usage-terminal](#usage-terminal) | Real available counters, tool/limit/partial termination and errors, full message_delta/message_stop; no success after upstream error. |
| P5-AC3 | [extended-events](#extended-events) | Identify actual Codex source representations before support claims; test available thinking/citation/compaction; document unavailable data and never synthesize signatures. |

Current baseline normalization resolves aliases, reconstructs typed Messages and filters Codex tools. System hoisting, nested-field loss, OAuth duplicate attribution and buffering are recorded defects, not permission to retain incorrect semantics. Upstream schema acceptance and event availability need separate evidence.

## Verification receipt

Observed locally on 2026-11-20 with the pinned binary above and gateway release SHA-256 `d20f0404ecf5ca0266b60b751314b42ccf6f078cc36f3e9a03e006e7a3c519d5`:

- P2 command: 20 ordered cases passed; 14 `known_gap`, 6 `correct_behavior`, zero `unavailable`. All 3,657 graph records scanned and 13 raw-byte spans verified. Nineteen actual upstream requests were recorded locally; the effort-only Codex control returned the expected structured 400 without an upstream request.
- Direct retry deny selected no retry; direct allow selected 125 ms ahead of Retry-After: 9 seconds. Header-stripped gateway responses selected retry and the original 375–500 ms jitter range. These are decision/method observations using bounded adapters, not automatic transport retries.
- The Anthropic positive control delivered a decoded text delta while the terminal barrier remained closed. Codex did not deliver a delta during the bounded closed-barrier wait. Both completed with exact terminal text after release. Terminal usage was 17/11 with cache counters 5/3 on Anthropic and known zeros on Codex.
- P2 external-network and protected credential probes passed; owned gateway/ports closed and the independent listener survived. P1 also passed its SDK positive/malformed-order cases, external redirect rejection, isolation and owned cleanup.
- `cargo fmt --check`, 68 Rust tests, 23 Python tests (including eight P2 control tests), strict clippy, release build, relative file links and `git diff --check` passed. The seven-day crate-age check passed for 227 registry crates with metadata access; an earlier restricted attempt checked zero crates because DNS was unavailable.

Only hashes, synthetic fixtures and this summary are versioned. Extracted modules, raw requests and detailed result manifests remain in the runner's temporary evidence directory. This baseline does not establish P3–P5 acceptance, native tool discovery/execution, automatic client retry or live upstream compatibility.
