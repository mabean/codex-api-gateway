# C3 native Responses request verification — 2026-11-24

Status: local synthetic implementation evidence; provider/model capability and live account access are unverified. Source: accepted C3-AC1–AC4 request slice, pinned OpenClaw `59b0aec`, and the release HTTP [runner](../scripts/codex_responses_c3.py) with its [fixtures](../scripts/fixtures/codex_responses/c3_cases.json). C2 baseline revision: `6720020f42c34dc41ab90e2f4bcd9282380a8fbf`.

## Reproduction and isolation

Use the three C3 commands in [BUILD.md](../BUILD.md), passing an immutable prebuilt C2 executable for `--expect c2`. The runner uses C1's existing sandbox lifecycle and adds only optional synthetic auth/configured path arguments to that lifecycle. It probes OS denial of external egress and protected credential-canary reads, uses synthetic OAuth bearer/account, records actual upstream requests, fingerprints inputs, enforces process deadlines, closes owned listeners and checks an unrelated sentinel survives. The intentional post-start failure takes the same cleanup path. No live upstream requests, SDK acceptance, credential refresh or deployment occurred.

Both releases receive identical fixtures. The final inventory has 55 ingress cases plus eight additional URL runs (63 observed cases across nine configurations). The two replay cases use real JSON/SSE first-turn responses to construct second-turn history, extracting the returned function `call_id` and retaining the returned opaque reasoning item. The fixture receives that exact history plus its correlated `function_call_output`; the second ingress response contains `Berlin: 18°C`.

## Observed contract

| Criterion | Discriminating observation |
| --- | --- |
| C3-AC1 | Each accepted Responses request sends `store:false`, `stream:true`, array input and instructions. String, object, scalar compatibility and exact array history are retained. Store true and any supplied previous response ID fail before upstream access. |
| C3-AC2 | Caller-defined `weather_lookup_42`, its nested JSON schema, description, explicit strict true/false, omitted strict false, flat/nested choice and parallel false survive. Unsupported types, malformed declarations and undeclared choices return field-specific 400 with zero attempts. Chat and translated Messages still remove the arbitrary function. Native caller-defined ToolSearch is ordinary data. |
| C3-AC3 | Explicit priority tier, JSON schema text format/verbosity, low reasoning with `summary:null`, include and cache key match the observed upstream payload. Absent cache key stays absent. Unsupported field presence, including null, and invalid option shapes receive safe 400 and `error.param`. |
| C3-AC4 | Root loopback, base `/backend-api`, `/backend-api/codex`, complete endpoint, trailing slashes and `/tenant` prefix select one configured endpoint. Caller body/header destination and credential probes cannot change configured bearer/account or URL. |

The C2 comparison records the former forwarding of unsupported controls, dropped arbitrary functions, generated cache keys and duplicated endpoint suffixes. C3 rejects all 44 negative cases locally; accepted cases, including two second turns and eight extra URL checks, make 21 upstream requests. C2 retains its historical shape errors and forwards other cases, making 59 requests. Both identical final fixture runs passed all 63 cases and their cleanup/sentinel checks. The final deliberate-failure cleanup run also passed, preserving its expected intentional failure outcome.

## Temperature provenance and limits

The request policy is pinned to OpenClaw `59b0aec`, `packages/ai/src/providers/openai-reasoning-effort.ts` (`isOpenAIGpt6Model`, `supportsOpenAITemperature`) and `extensions/openai/openclaw.plugin.json` explicit false compatibility entries. After gateway model normalization, GPT-5.6 and its hyphenated family, named GPT-6 Astra/Sol/Luna, GPT-6.1 Sol, GPT-6.2 Astra/Sol and Daybreak Blue/Red reject temperature. Unknown model IDs retain finite values with capability unresolved. A fixture accepting `.75` proves forwarding only; this is not a provider entitlement/support claim.

## Checks and fingerprints

Implementation checks run: all 92 Rust tests; all 40 Python tests; focused C3 inventory tests; `cargo fmt --check`; clippy with all targets/features and warnings denied; offline locked release build; relative documentation links/whitespace; and `git diff --check`. The orchestrator owns final CI and C1/C2/P1/P3/P4/P5 regression acceptance on the supplied snapshot. Implementation success is not acceptance.

| Artifact | SHA-256 |
| --- | --- |
| Frozen C2 release | `6bf34b3dbb179cb359127689d73951339586ad189ad25ec86a60df5f43bd33c4` |
| Initial C3 release | `103ce641922f13e6ca678be67615ac850c3db028833759f1635fa18d895e869e` |
| Initial C3 runner | `ef2718a8c0cf929e005829fdd498744b5df86edbb0fe19f918f1cb7163fd310c` |
| Shared C1 lifecycle | `9d4afaf2b70e741282f628a7c339036e39f5b18c357adba3580d34071bcf042a` |
| Initial C3 cases | `28d1203ab46e6eaa44f6d621fb87cdc8e2711e1043e9cdee30892660a9b2a459` |

Raw request observations and cleanup reports remain temporary runner output; the orchestrator retains phase evidence. No real credential contents or live wire logs are included here.

## Repair round 1 — C3-R1/R2/R3

Status: scoped local repair evidence, 2026-11-24; final acceptance remains with the orchestrator. Reviewed initial snapshot: `b77dac08a1f57bd3b4aa1d75366912b2080aa82b57f6fa77a78459668e3ff0dc`.

- C3-R1 hypothesis: strict option validation rejected valid nullable/default shapes and supported reasoning controls. The repair treats supported optional top-level null controls as omitted; model and forbidden-field presence remain invalid. Reasoning context/mode and nullable inner controls are preserved after shape validation. Summary `off`/`on` remain verbatim per the pinned Codex builder. Public shape evidence: [Responses create reference](https://developers.openai.com/api/reference/resources/responses/methods/create); pinned evidence: [OpenClaw request builder](https://github.com/openclaw/openclaw/blob/59b0aec099f5751ab221d724e339402eb111e828/packages/ai/src/providers/openai-chatgpt-responses-request.ts#L91-L101). Model/account support remains unverified.
- C3-R2 hypothesis: native nullable declarations were confused with malformed schemas and nested defaults were lost. Flat nullable description/parameters now remain native nulls. Nested nullable/missing parameters regain the established `{"type":"object","properties":{}}` default. Supplied nonnull schemas remain unchanged; flat supplied strict still requires boolean.
- C3-R3 hypothesis: literal identity comparison missed known models with case/whitespace/date variations. Temperature comparison now trims/folds identity and removes only a syntactic `-YYYY-MM-DD` suffix. Existing outbound model serialization remains unchanged; unknown and malformed-date identities retain unresolved capability and finite-value forwarding. Source: [pinned identity normalization](https://github.com/openclaw/openclaw/blob/59b0aec099f5751ab221d724e339402eb111e828/packages/ai/src/providers/openai-reasoning-effort.ts#L41-L47).

The same 52 targeted HTTP cases in both ingress modes pass their explicit initial/repaired expectations: initial C3 makes 12 upstream attempts, while the repaired candidate makes 46. This demonstrates the initial local rejections, restored compatible payloads, restored nested defaults and newly local temperature refusals. The unchanged original 63 observations also pass with the repair cases in a 115-observation combined run (67 attempts). That combined run used candidate SHA `9fd8c328c83c56ad62b34fcee2d243674b9e09e2ff617e3ad3716af07b163aa1`, before a clippy-only redundant-unit cleanup. Final focused HTTP verification uses the final release below.

Repair checks: nine existing Responses Rust tests, three new issue-specific Rust regressions, two Python fixture-inventory tests, clippy with warnings denied, locked offline release build, formatting and documentation/diff checks. No full final acceptance or live upstream checks were run by the executor in this round.

| Repair artifact | SHA-256 |
| --- | --- |
| Final repair release | `d313f333b55782705e64fa7031371510a917cf4a2e34f7616130ee6e7a29c1b6` |
| Repair runner | `df8dea40a104c44528cce06e191356fa3b58c6c0b1a03fbb853d302d044e4a90` |
| Extended cases | `c48ec75dda2de277e8f03bfdf0162f76a60a75fd3a92452aecfe2821c29109e9` |
