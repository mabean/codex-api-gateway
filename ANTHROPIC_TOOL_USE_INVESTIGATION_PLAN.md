# Anthropic Tool Use Investigation Plan

## Context

Text completions through the gateway work for both OpenAI-compatible and Anthropic-compatible ingress, including Claude Code text-only flows. The remaining blocker is Claude Code file-edit/tool-use behavior: the model often returns prose and `stop_reason: end_turn`, while the file remains unchanged.

Raw upstream probing already proved that Codex can emit real tool calls:

- `response.output_item.added`
- `type: "function_call"`
- `response.function_call_arguments.delta`
- final `function_call` arguments for tools like `Edit`

Therefore the remaining investigation focuses on:

1. whether Anthropic `/v1/messages` requests preserve tools and tool history into the Codex upstream payload;
2. whether gateway Anthropic streaming/output matches what Claude Code / CCR expects for valid tool execution;
3. whether payload differences vs OpenClaw coding-agent are causing Codex to choose prose instead of `function_call`.

## Current High-Confidence Findings

### Finding A: Anthropic ingress was dropping tools (now patched for investigation)

Status: confirmed and patched locally.

Changes already made:

- added `tools: Option<Vec<Value>>` to `AnthropicMessagesRequest`;
- forwarded Anthropic `tools` through `convert_anthropic_to_chat()` into `ChatCompletionsRequest`;
- forwarded them onward into `ResponsesApiRequest.tools`;
- added temporary diagnostics for incoming Anthropic tools and final Responses payload tool list.

Current evidence:

- before the patch, Anthropic `/v1/messages` path could not preserve tools because request parsing did not model them at all;
- after the patch, gateway now logs incoming tool presence and outgoing normalized tool names/count.

Implication: the first major prose-vs-tool suspect has been confirmed as real.


In current `src/main.rs`:

- `AnthropicMessagesRequest` does not include a `tools` field.
- `convert_anthropic_to_chat()` always sets `tools: None`.
- `convert_chat_to_responses()` only forwards tools from `chat_req.tools`.

Implication:

- Claude Code may be sending Anthropic tools to `/v1/messages`,
- but gateway may be discarding them before building the Codex Responses payload,
- causing upstream to reasonably answer in prose instead of tool-calling.

This is the strongest immediate suspect and must be confirmed/fixed first.

### Finding B: Anthropic SSE rendering is still a minimal implementation

Current shared streaming layer already supports:

- Codex `function_call` parsing,
- Anthropic `tool_use` rendering,
- Anthropic `stop_reason: "tool_use"` heuristic,

but it is still much simpler than CCR’s Anthropic transformer and may not fully match Claude Code’s expectations in mixed or multi-step tool flows.

## Investigation Phases

## Phase 1 — Confirm tool preservation through Anthropic ingress

### Goal

Prove whether tools sent by Claude Code to `/v1/messages` make it into the upstream Codex Responses payload.

### Tasks

1. [done] Add temporary logging around Anthropic ingress for tools count/names.
2. [done] Extend `AnthropicMessagesRequest` to include `tools`.
3. [done] Pass Anthropic tools through `convert_anthropic_to_chat()`.
4. [done] Add temporary logging before upstream send for normalized tools/model/input summary.
5. [next] Re-run Claude Code acceptance prompt and inspect logs against real traffic.

### Success criteria

- [achieved] We proved by code inspection and local patching that tools were being dropped on the Anthropic ingress path.
- [next] Confirm with live request logs during Claude Code acceptance run.

## Phase 2 — Make Anthropic non-streaming and streaming tool contract less lossy

### Goal

Ensure that when upstream emits tool calls, gateway returns Anthropic output that Claude Code can act upon.

### Tasks

1. Audit canonical event model vs actual Codex SSE lifecycle.
2. Improve parser fidelity for:
   - `response.output_item.added`
   - `response.output_item.done`
   - `response.function_call_arguments.delta`
   - completed function-call argument assembly.
3. Improve Anthropic SSE renderer semantics:
   - block index management,
   - mixed text/tool block ordering,
   - final `message_delta.stop_reason` mapping,
   - avoid overly heuristic stop-reason derivation where possible.
4. Review non-streaming Anthropic response conversion for tool_use presence and stop_reason mapping.

### Success criteria

- If upstream emits `function_call`, rendered Anthropic output clearly contains:
  - `tool_use` block(s),
  - argument deltas,
  - correct `stop_reason: "tool_use"`.

## Phase 3 — Compare working OpenClaw coding payload vs gateway payload

### Goal

Determine why Codex chooses prose in gateway scenarios even though raw upstream probes can produce `function_call`.

### Compare

1. `instructions`
2. `input` history structure
3. `tools` array
4. tool descriptions
5. schema details:
   - `strict`
   - `additionalProperties`
   - nested schema shape
6. `tool_choice`
7. `parallel_tool_calls`
8. `text` config
9. `reasoning`
10. `include`

### Success criteria

- Produce a concrete payload diff showing the likely semantic reason for prose-vs-tool behavior.

## Phase 4 — Re-run acceptance ladder after each meaningful fix

### Ladder

1. raw incoming Anthropic request
2. raw upstream Codex payload
3. raw Codex SSE
4. raw Anthropic SSE returned by gateway
5. Claude Code acceptance test:
   - ask to use `Edit`
   - verify file mutation actually happened
   - verify follow-up file read matches actual contents

### Contract outcome

The acceptance test remains red until file editing is real. That is correct and should not be weakened.

## Expected First Code Changes

### Immediate patch targets

- `src/main.rs`
  - add `tools` to `AnthropicMessagesRequest`
  - forward tools in `convert_anthropic_to_chat()`
  - add temporary diagnostics around Anthropic ingress and upstream request construction

### Near-term patch targets

- `src/streaming.rs`
  - improve Codex function-call lifecycle parsing
  - improve Anthropic renderer fidelity for `tool_use`

## Notes

- Do not delete or weaken the red contract test.
- Remove or rewrite stale normalization-hypothesis tests if they conflict with the new tool-path understanding.
- The key question is no longer “can Codex do tools?” — it can.
- The key question is now “what exact request/response contract difference causes gateway-driven Claude Code to choose prose or fail to execute tools?”

## Progress Log

- Confirmed bug: Anthropic `/v1/messages` request struct did not model `tools`, so tools were silently lost before upstream transformation.
- Patched local code to preserve Anthropic tools into Codex Responses payload.
- Added temporary stderr diagnostics for incoming Anthropic tools and outgoing normalized Responses tools.
- `cargo test -q` now passes 21 tests; 2 remain red:
  - `anthropic_edit_prompt_stream_must_use_tool_use_contract` (intentionally red contract test; keep red until real tool path works end-to-end)
  - `normalize_tools_for_codex_sets_strict_and_additional_properties_false` (appears stale against current normalized tool shape; should be updated or replaced)

- Updated stale normalization test to match current flattened normalized tool shape (`name`, `parameters`, `strict` at top level).
- Phase 2 started: improved shared streaming model to carry `Completed { finish_reason }` instead of unit-only completion.
- Phase 2 started: improved Codex SSE parser so `function_call` items do not emit premature `ToolCallDone` just because arguments first appear; done is now tied to completed item / item.done semantics.
- Phase 2 started: Anthropic renderer refactored toward CCR-style block handling (explicit block indices, tool block map, stop_reason from finish_reason when available).
- Current test state: `cargo test -q` => 22 passing, 1 failing. Remaining red test is the intentional contract test `anthropic_edit_prompt_stream_must_use_tool_use_contract`.

- Live Claude Code acceptance run after Anthropic tools passthrough no longer failed due to missing tools; it now fails earlier with upstream schema validation.
- New confirmed blocker: Codex upstream returns `invalid_function_parameters` for tool schema, e.g. `Invalid schema for function 'Agent' ... Missing 'isolation'`.
- Interpretation: after preserving Anthropic tools, gateway is now sending tool definitions upstream, but current normalization is not producing Codex-valid strict schemas for at least some Claude Code tools.
- This moves the investigation from pure transport loss into payload/schema fidelity. The next sub-phase is schema normalization correctness, especially `required` coverage and nested property completeness for Anthropic tool definitions coming from Claude Code.

- Phase 2.6 started: replaced schema normalization with recursive object normalization.
- Recursive normalization now:
  - adds `additionalProperties: false` to object schemas;
  - synthesizes `required` from every key in `properties`;
  - recurses into nested `properties`, `items`, `anyOf`, `oneOf`, and `allOf`.
- Added regression test proving nested object schema with `isolation` now gets full `required` coverage.
- Current test state after schema fix: `cargo test -q` => 23 passing, 1 failing intentional contract test.

## Patch Plan — OpenClaw-like tool schema handling

### Goal
Reduce gateway tool-schema transformation so it matches OpenClaw/Codex behavior more closely and stops distorting Claude Code tool semantics.

### Patch steps

1. Change `normalize_tools_for_codex()` / schema handling to preserve Anthropic tool schemas mostly as-is:
   - map wrapper shape (`name`, `description`, `input_schema`/`parameters`) into Codex function tool shape;
   - stop synthesizing `required` from all `properties`;
   - stop recursive insertion of `additionalProperties: false`;
   - keep only minimal compatibility cleanup if strictly necessary.
2. Make `strict` configurable at normalization time and default first to OpenClaw-like behavior for investigation.
3. Add regression tests proving optional parameters remain optional for representative Claude Code tools.
4. Run `cargo test -q`.
5. Run live Claude Code acceptance against patched gateway and record whether:
   - upstream schema validation passes;
   - Codex returns `function_call` or prose;
   - Anthropic tool-use stream becomes valid.

### Immediate hypothesis
The current recursive strictification is the main incompatibility. Restoring near-pass-through schemas should move the system past upstream schema rejection and allow observation of real model behavior (`function_call` vs prose).

## Debug Harness Plan

To avoid guessing behind generic 502s, add a stable debug harness that surfaces exact upstream failures on every run:

1. Log upstream response status and full error body before classification.
2. Log compact per-tool summaries in the outgoing upstream request (name, strict, top-level required keys, schema source).
3. Add an opt-in env flag / debug mode so these logs stay available without permanently polluting normal output.
4. Add a repeatable local acceptance script that:
   - kills old listeners;
   - starts the patched gateway on a dedicated port;
   - runs the Claude Code acceptance prompt;
   - prints note.txt, Claude stdout, and filtered gateway debug logs.
5. After each patch, rerun the exact same harness and compare deltas.

Goal: every failing run should answer "which upstream request failed, with what exact status/body, and which tool/schema payload caused it".

## A/B Step 1 — Instructions isolation

Patch only the `instructions` construction while keeping tools, input history, parser, renderer, and stream settings unchanged.

Variant B1 goal:
- reduce the giant Claude SDK instruction blob impact;
- prepend/replace with a compact tool-compelling directive for file-edit tasks;
- explicitly forbid claiming edits without tool execution;
- explicitly prefer `Edit` for file modifications and `Read` for verification.

Success metric:
- compare `codex-events-summary` vs baseline and watch for `tool_call_starts > 0`.

## A/B Step 2 — Text verbosity isolation

Change only `text.verbosity` from `medium` to `low`, keeping tools, instructions patch, input history, parser, and renderer unchanged.

## A/B Step 3 — Tool set reduction experiment

After Step 2, test a reduced tool set for the acceptance scenario. Keep only the smallest tool subset needed for file edit verification (`Edit`, `Read`, optionally `Write`/`Bash`) and compare whether Codex starts choosing `function_call`.

## Verbose tracing patch

Add a minimal env-gated tracing mode (`CODEX_PROXY_VERBOSE=1`) with structured markers that distinguish:
- Codex no-tool-path vs Codex tool-path detected
- Anthropic tool-use rendered vs text/end_turn rendered
- raw Codex/Anthropic SSE only in verbose mode

This should make each run answer, without ambiguity: did Codex emit a tool call, and if yes, did gateway render a valid Anthropic `tool_use` wire sequence?

## Codex-side normalization parity patch #1

Goal: stop sending raw Claude SDK / Anthropic envelope boilerplate into Codex `instructions`. Keep only load-bearing system content so the request more closely resembles OpenClaw's normalized internal context -> Codex payload path.

Patch scope:
- sanitize / strip Claude-specific envelope lines from `instructions`;
- keep tools and current wire logging unchanged;
- run multiple controlled attempts and compare `codex_tool_path_detected` frequency.

## Codex-side normalization parity patch #2 (structure locked)

Decision: stop treating temperature as the next lever. Keep the current winning structural changes and continue with OpenClaw-like normalization improvements.

Locked-in structure so far:
- `tools` normalized in OpenClaw-like shape (`parameters` mostly as-is, `strict: false`);
- Anthropic SSE lifecycle fixed (`message_stop_seen: true`);
- `instructions` normalized to strip Claude/Anthropic envelope noise;
- prior assistant prose omitted from upstream Codex `input`.

Next improvements should stay structural and Codex-centric, not sampling-centric.

## Codex-side normalization parity patch #3 (core instructions cleanup)

Goal: drive prose false-positives toward zero by shrinking Codex `instructions` to the minimum load-bearing core. The current normalized prefix still starts with meta-reminder text, which likely continues to prime prose.

Patch scope:
- strip remaining meta-reminder / tag-explanation lines from `instructions`;
- keep tools, SSE wiring, and assistant-prose omission unchanged;
- run a multi-run sample and record tool-path vs prose-path counts.
