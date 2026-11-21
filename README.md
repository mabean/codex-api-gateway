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

GPT ingress on all three endpoints uses the upstream Responses protocol, including tools. `/v1/chat/completions` accepts standard assistant `tool_calls`, tool messages with `tool_call_id`, and `reasoning_effort`; JSON and SSE responses preserve tool call IDs. Chat SSE terminates tool calls with `finish_reason: "tool_calls"`. Text present only in a final upstream message item is preserved without duplicating streamed deltas. The supported tool names remain `Edit`, `Read`, `Write`, and `Bash`.

For GPT-6.2 Astra, GPT-6.2 Sol, GPT-6 Astra, and GPT-6.1 Sol, use `low` or higher reasoning effort. GPT-6.1 Terra, GPT-6 Sol, and GPT-6 Luna accept `none`. See the [official GPT-6 guide](https://developers.openai.com/api/docs/guides/latest-model).

Anthropic requests preserve top-level fields such as `thinking`, `tool_choice`, and `output_config`. For Fable 5.2, Fable 5.1, Opus 5.5, and Sonnet 5.5, use `tool_choice: {"type":"auto"}` or `{"type":"none"}`. Fable 5.2, Fable 5.1, and Opus 5.5 require adaptive thinking. Sonnet 5.5 also accepts `between_tools` at `high` effort or below. See [Anthropic thinking documentation](https://platform.claude.com/docs/en/build-with-claude/thinking) and the [Fable 5.2 migration guide](https://platform.claude.com/docs/en/models/fable-5-2/migration-guide).

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

Codex credentials are read from `--auth-path`. Legacy Codex auth files and OpenClaw-style auth profile files are supported.

The gateway reads Codex credentials at startup and does not validate token expiry locally. Restart it after updating the auth file; upstream authorization errors indicate credentials that are rejected.

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
