# Codex API Gateway

A local gateway that lets OpenAI-compatible and Anthropic-compatible clients use Codex/ChatGPT and Claude upstreams through one local base URL.

> **Experimental:** the gateway handles local OAuth credentials and is intended for single-user development on `127.0.0.1`. Do not expose it to a LAN or the public internet.

## Origin and evolution

This project originally started from [Securiteru/codex-openai-proxy](https://github.com/Securiteru/codex-openai-proxy) and has since evolved into an independently maintained gateway with Anthropic compatibility, multi-upstream routing, Claude Code tool-use support, per-agent model routing, and a local-first security model.

## What it provides

| Endpoint | Purpose |
| --- | --- |
| `POST /v1/responses` | OpenAI Responses-compatible requests through the Codex/ChatGPT backend |
| `POST /v1/chat/completions` | OpenAI Chat Completions-compatible requests through the Codex/ChatGPT backend |
| `POST /v1/messages` | Anthropic Messages-compatible requests, routed by model family |
| `POST /v1/messages/count_tokens` | Direct Anthropic token counts; Codex returns a structured 404 for client estimation |
| `GET /v1/models` | Models advertised by the gateway |
| `GET /health` | Local process health |

The gateway supports JSON and streaming responses, Claude Code text flows, and the core `Edit`, `Read`, `Write`, and `Bash` tool loop.

For Codex-bound Anthropic Messages, `ToolSearch` is a client-side discovery mechanism for that same tool set. Deferred declarations become available after a preceding declared ToolSearch call and correlated result references a supported declared tool; references become schema-bearing text outputs. The gateway executes no tools. Unsupported, undeclared or unrelated references receive a structured 400.

Codex Messages translate `output_config.effort` values `low`, `medium`, `high` and `xhigh`, including the last system effort statement for the turn and empty effort-only content. Absence retains existing defaults. Non-temporary system text within conversation history retains its position and system role in Codex input, including separate text blocks and subsequent tool turns. Top-level and leading system prompts retain existing global instruction normalization. Temporary system scope (`clear_at`) and system content other than supported text receive a terminal structured 400. Tool selection supports omitted/`auto`, `none`, `any`, or a named active tool, with boolean `disable_parallel_tool_use` inverted for Codex. The client's exact `context_management: {"edits":[{"type":"clear_thinking_20251015","keep":"all"}]}` is accepted as a retain-all no-op. Other context settings, unsupported effort, output formats and unknown output controls receive explicit refusals. These discovery and parallel controls apply to Anthropic Messages; public Chat retains its existing tool allowlist and parallel behavior; native Responses accepts client-declared functions. Cache markers have no Codex cache guarantee; Anthropic signed thinking cannot be synthesized. See [the local P4 contract and limits](docs/claude-code-p4-verification-2026-11-21.md) and [standard-mode verification](docs/claude-code-standard-mode-verification-2026-11-25.md).

Requests to `/v1/messages` are routed as follows:

- `claude-*` models go to the Anthropic upstream.
- GPT and other Codex-style models go to the Codex/ChatGPT upstream.

## Requirements

- A current stable Rust toolchain.
- A local Codex auth file, normally `~/.codex/auth.json`.
- A ChatGPT account with access to the Codex backend path.
- For Claude models: a Claude Code login, `ANTHROPIC_OAUTH_TOKEN`, or `ANTHROPIC_UPSTREAM_API_KEY`.

An OpenAI API key alone does not grant access to the ChatGPT/Codex backend.

## Quick start

```bash
git clone https://github.com/mabean/codex-api-gateway.git
cd codex-api-gateway
cargo build --release

./target/release/codex-api-gateway \
  --port 8080 \
  --auth-path ~/.codex/auth.json
```

Confirm that the local process is running:

```bash
curl http://127.0.0.1:8080/health
```

Send an OpenAI Responses request:

```bash
curl http://127.0.0.1:8080/v1/responses \
  -H 'content-type: application/json' \
  -d '{
    "model": "gpt-5.6-sol",
    "input": "Reply with exactly: OK"
  }'
```

## Claude Code

Point Claude Code at the local Anthropic-compatible endpoint:

```bash
export ANTHROPIC_BASE_URL="http://127.0.0.1:8080"
export ANTHROPIC_API_KEY="local-proxy"
export CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1
```

`local-proxy` is only a local placeholder. Upstream authentication is resolved separately by the gateway.

You can now use either upstream through the same client:

```bash
claude -p --model gpt-5.6-sol "Reply with exactly: CODEX_OK"
claude -p --model claude-sonnet-5-5 "Reply with exactly: CLAUDE_OK"
```

## Models and aliases

Query the gateway's advertised catalog instead of hard-coding the full list. It is a static list, not a check of upstream availability or account entitlements:

```bash
curl http://127.0.0.1:8080/v1/models
```

The main model families currently include (checked 2026-11-18 against the [OpenAI catalog](https://developers.openai.com/api/docs/models) and [Claude catalog](https://platform.claude.com/docs/en/models/overview)):

- GPT-6: `gpt-6.2-astra`, `gpt-6.2-sol`, `gpt-6.1-terra`, `gpt-6.1-sol`, `gpt-6-astra`, `gpt-6-luna`, and `gpt-6-sol`.
- GPT-5.6: `gpt-5.6-sol`, `gpt-5.6-terra`, and `gpt-5.6-luna`.
- `gpt-5.6`, normalized by the gateway to `gpt-5.6-sol`.
- Latest Claude: `claude-fable-5-2`, `claude-opus-5-5`, `claude-sonnet-5-5`, and `claude-haiku-5-5`.
- Earlier Claude 5: `claude-fable-5-1`, `claude-fable-5`, `claude-opus-5`, and `claude-sonnet-5`.
- Older GPT and Claude catalog entries returned by `/v1/models`.

The catalog covers general-purpose text and tool models. Anthropic limits Mythos 5.2 access to verified organizations. See [Anthropic's November 9 release notes](https://platform.claude.com/docs/en/release-notes/overview#november-9-2026).

Use the full model IDs for GPT-6 releases. Catalog release dates come from the [OpenAI changelog](https://developers.openai.com/api/docs/changelog) and [Anthropic release notes](https://platform.claude.com/docs/en/release-notes/overview), with dates represented at midnight UTC.

GPT ingress on all three endpoints uses the upstream Responses protocol, including tools. `/v1/chat/completions` accepts standard assistant `tool_calls`, tool messages with `tool_call_id`, and `reasoning_effort`; JSON and SSE responses preserve tool call IDs. Chat SSE terminates tool calls with `finish_reason: "tool_calls"`. Text present only in a final upstream message item is preserved without duplicating streamed deltas. Chat and translated Messages keep the `Edit`, `Read`, `Write`, and `Bash` executor allowlist. Native Responses accepts arbitrary valid function names.

Streaming `/v1/responses` forwards native SSE frames incrementally, retaining event order, tool call IDs and opaque data. Cancellation closes the upstream body. Retries use the existing configured policy before the first complete frame: timeouts remain retryable, while other body-read failures retain their protocol classification and existing retry predicate. Subsequent failures emit an SSE `error` without replay or successful completion. Native `response.completed`, `response.incomplete`, `response.failed` and `error` remain distinct. Legacy `response.done` becomes the corresponding terminal event according to `response.status`, preserving its payload. The first valid terminal ends the stream without waiting for upstream EOF. A complete terminal JSON event at EOF is also accepted when its last newline or blank delimiter is missing; the gateway appends the boundary needed by SSE consumers. Contradictory statuses, malformed frames and missing terminals fail explicitly. Frames are limited to 1 MiB and the configured total request deadline still applies. See [local C1 verification](docs/codex-responses-c1-verification-2026-11-23.md).

Nonstream `/v1/responses` returns the native completed or incomplete terminal response, preserving IDs, model, ordered output, incomplete details and available usage details. Native `output_text` is authoritative; when absent, the gateway joins ordered output-text blocks, falling back to streamed text for sparse terminals. A missing `object` becomes `response`; usage counts are never invented. Failed, malformed, contradictory or missing terminals return a structured error. JSON responses share native framing and EOF normalization, end at the accepted terminal without waiting for EOF, and have a 16 MiB response bound.

Before Responses output begins, upstream HTTP 400, 401, 403, 404 and 429 retain their status with fixed safe OpenAI error messages and distinct error codes; upstream 400 differs from local `validation_error`. Transport failures return 502 and typed timeouts return 504. Only a single valid `Retry-After` on the final 429 is relayed, excluding `Connection` nominations. Upstream error bodies and other headers are discarded. Chat and Messages keep their existing error mappings. See [local C2 verification](docs/codex-responses-c2-verification-2026-11-23.md).

For GPT-6.2 Astra, GPT-6.2 Sol, GPT-6 Astra, and GPT-6.1 Sol, use `low` or higher reasoning effort. GPT-6.1 Terra, GPT-6 Sol, and GPT-6 Luna accept `none`. See the [official GPT-6 guide](https://developers.openai.com/api/docs/guides/latest-model).

Anthropic requests preserve top-level fields such as `thinking`, `tool_choice`, and `output_config`. For Fable 5.2, Fable 5.1, Opus 5.5, and Sonnet 5.5, use `tool_choice: {"type":"auto"}` or `{"type":"none"}`. Fable 5.2, Fable 5.1, and Opus 5.5 require adaptive thinking. Sonnet 5.5 also accepts `between_tools` at `high` effort or below. See [Anthropic thinking documentation](https://platform.claude.com/docs/en/build-with-claude/thinking) and the [Fable 5.2 migration guide](https://platform.claude.com/docs/en/models/fable-5-2/migration-guide).

Codex-backed streaming `/v1/messages` responses deliver text and tool argument fragments incrementally. The gateway owns the upstream body through the downstream response: cancellation drops that connection. Retries follow the selected request policy until the first emitted Anthropic frame; subsequent errors become SSE errors without replay or a successful stop. The canonical parser rejects missing/contradictory terminal events, malformed data and inconsistent final snapshots. Available input/output token counters arrive in the terminal message delta; `max_output_tokens` incompletion maps to `max_tokens`. Frames are limited to 1 MiB and each canonical response to 16 MiB. Completion is confirmed at upstream EOF within the existing total request deadline. Codex reasoning summaries, annotations and opaque compaction data are retained on raw Responses SSE, but do not supply the provider-specific fields needed for Anthropic thinking/signature, citations or compacted-content blocks. See [P5 verification](docs/claude-code-p5-verification-2026-11-23.md).

The direct Anthropic route also preserves nested message fields, scoped system entries, `clear_at`, cache markers and tool references in their original array order. It forwards `?beta=true`, the client version and beta flags, status, response bytes and streamed bodies. Response metadata is limited to content type, retry headers, request ID and `anthropic-ratelimit-unified-*`, excluding headers nominated by `Connection`. Existing model aliases, `[gateway-model: ...]` removal, Agent routing instructions and credential precedence still apply. OAuth Messages retain client-supplied attribution; the legacy prefix is used only when attribution is absent. OAuth token-count requests receive no Messages prompt prefix. These are local forwarding guarantees; upstream support for individual beta features remains provider-dependent. See [P3 verification](docs/claude-code-p3-verification-2026-11-21.md).

Claude aliases default to:

```text
sonnet -> claude-sonnet-5-5
opus   -> claude-opus-5-5
haiku  -> claude-haiku-5-5
```

The `haiku` default moved from 4.5 to 5.5. Haiku 5.5 enables adaptive thinking by default; omit `temperature`, `top_p`, `top_k`, manual `thinking.budget_tokens`, and assistant prefill. See the [Haiku 5.5 release notes](https://platform.claude.com/docs/en/release-notes/overview#october-7-2026). To keep an existing Haiku 4.5 integration, request `claude-haiku-4-5-20251001` explicitly or set `CODEX_GATEWAY_ALIAS_HAIKU` to that ID. The gateway preserves request fields and upstream validation errors.

The [2026-11-18 catalog verification](docs/model-verification-2026-11-18.md) records the additions, release-date sources, and validation scope.

Override them when needed:

```bash
export CODEX_GATEWAY_ALIAS_HAIKU="gpt-5.6-luna"
```

Claude Code restricts `Agent.model` to `sonnet`, `opus`, or `haiku`. To route an individual subagent to any gateway model, set `Agent.model` to `haiku` and start its prompt with:

```text
[gateway-model: gpt-5.6-terra]
```

The gateway removes this directive before forwarding the prompt.

## Authentication

Codex credentials are read once at startup from the operator-selected `--auth-path`. Use the native Codex file-backed ChatGPT login: configure Codex `cli_auth_credentials_store = "file"`, let Codex own login and refresh, and pass its `$CODEX_HOME/auth.json` (normally `~/.codex/auth.json`). See [official Codex authentication](https://learn.chatgpt.com/docs/auth?surface=cli). Keyring-only or ephemeral login does not provide this file contract.

Supported native JSON has `auth_mode:"chatgpt"` (or historical omitted mode), `tokens.access_token`, and `tokens.account_id`. Unrelated owner fields such as `id_token`, `refresh_token` and `last_refresh` are tolerated; the gateway never uses them to refresh. A missing account may come from the access-token claim `https://api.openai.com/auth` → `chatgpt_account_id`. Conflicting, empty or invalid accounts fail startup. JWT claims are decoded only for consistency and known `exp` checks; they do not verify signatures, ownership or Codex route provenance. Opaque access tokens with explicit accounts have unknown expiry. Platform API keys and explicitly different auth modes are rejected.

Legacy OpenClaw JSON is limited to `profiles` entries with `type:"oauth"`, `provider:"openai-codex"`, `access` and `accountId` (or the same claim fallback). `lastGood["openai-codex"]`, when present, must point to a fully valid matching profile; invalid or dangling references fail closed. Otherwise exactly one matching profile is required. Known legacy `expires` milliseconds and JWT `exp` seconds must be in the future. Modern `provider:"openai"`, SIWC, API-only and foreign profiles are unsupported: JSON labels and unverified claims cannot establish which credential flow owns them. There is no managed OpenClaw SQLite integration or private app-server handoff; see [C4 sources and support boundary](docs/codex-responses-c4-verification-2026-11-24.md).

After the login owner updates the file, stop and restart the gateway through its existing process manager with the same explicit path. A running process retains its original token/account snapshot. Startup diagnostics use fixed categories without credential values. Responses 401/403 preserve the status with safe messages: check the selected owner login, account and access, update through Codex, then restart. The gateway does not attempt refresh. Inbound credentials cannot replace configured Codex bearer/account headers.

Anthropic authentication is resolved in this order:

1. `--anthropic-auth-token` or `ANTHROPIC_UPSTREAM_API_KEY`.
2. `ANTHROPIC_OAUTH_TOKEN`.
3. Local Claude Code credentials from the macOS Keychain or `~/.claude/.credentials.json`.
4. A real inbound `Authorization` or `x-api-key` header.

Never commit or share these credentials.

## Configuration

Run `codex-api-gateway --help` for the complete CLI reference. The most useful options are:

```text
--port <PORT>                              default: 8080
--auth-path <PATH>                         default: ~/.codex/auth.json
--upstream-base-url <URL>                  default: https://chatgpt.com/backend-api
--anthropic-upstream-base-url <URL>        default: https://api.anthropic.com
--anthropic-auth-token <TOKEN>
--log-path <PATH>
--log-max-size-mib <MIB>                   default: 10
--log-retained-files <COUNT>               default: 5
```

Logs rotate automatically. The default path is `~/Library/Logs/codex-api-gateway/gateway.log` on macOS and `$XDG_STATE_HOME/codex-api-gateway/gateway.log` or `~/.local/state/codex-api-gateway/gateway.log` on Linux.

Timeouts and retry counts can be adjusted with:

```text
CODEX_GATEWAY_CODEX_TIMEOUT_SECS                    default: 30, range: 5..300
CODEX_GATEWAY_CODEX_MAX_ATTEMPTS                    default: 1,  range: 1..5
CODEX_GATEWAY_CODEX_AGENT_TIMEOUT_SECS              default: 12, range: 5..300
CODEX_GATEWAY_CODEX_AGENT_MAX_ATTEMPTS              default: 1,  range: 1..5
CODEX_GATEWAY_ANTHROPIC_FIRST_CHUNK_TIMEOUT_SECS    default: 12, range: 3..120
```

## Verification

Run the local checks described in [BUILD.md](BUILD.md). Two optional live suites verify real credentials and upstream behavior:

```bash
scripts/live_mixed_model_smoke.sh
scripts/live_claude_code_acceptance.sh
```

Live suites consume upstream quota and require valid local Codex and Claude credentials.

## Limitations

- This is not a production or multi-tenant gateway.
- Anthropic compatibility covers the subset needed by the supported local workflows, not the entire Anthropic API.
- Codex-bound tools are limited to `Edit`, `Read`, `Write`, and `Bash`; other Claude Code tools, including `Agent`, are filtered out.
- Compatibility-surface usage accounting is approximate.
- Model availability depends on the current account and upstream entitlements.
- Upstream private contracts can change without notice.
- Verbose diagnostics may contain prompts, responses, and tool arguments.

See [SECURITY.md](SECURITY.md) before running the gateway and [THREAT_MODEL.md](THREAT_MODEL.md) before changing its trust boundary.

## Development

See [BUILD.md](BUILD.md) for build and verification commands, [PROJECT_MAP.md](PROJECT_MAP.md) for the repository layout, and [AGENTS.md](AGENTS.md) for coding-agent guidance.

## License

[MIT](LICENSE)

Native Responses requests use `store:false` and upstream `stream:true` in both client modes. `store:true`, any supplied `previous_response_id`, and presence of `metadata`, `truncation`, `top_p` or `user` (including null) receive local HTTP 400 with `error.param` before upstream access. Replay conversation history in `input`; array items, opaque reasoning and correlated function outputs are retained. Flat or nested function declarations retain schema, description and boolean `strict` (default false); a named choice must reference a declared function. Parallel tool calls remain explicitly configurable.

Validated `service_tier`, `text` format/verbosity, `reasoning` (including `summary:null`), `include` and explicit `prompt_cache_key` are forwarded. An omitted cache key remains absent. The pinned OpenClaw `59b0aec` policy rejects temperature for GPT-5.6 families, named GPT-6 Astra/Sol/Luna routes and the two Daybreak aliases; other finite values are forwarded with model capability unresolved. Fixture forwarding does not establish real model support. Configured Codex URLs ending in `/backend-api`, `/backend-api/codex` or `/backend-api/codex/responses`, with trailing slashes, select one Responses endpoint and preserve custom prefixes and root localhost overrides. See [C3 offline verification](docs/codex-responses-c3-verification-2026-11-24.md).
