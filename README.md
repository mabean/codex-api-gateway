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

The main model families currently include:

- GPT-6: `gpt-6.1-sol`, `gpt-6-astra`, `gpt-6-luna`, and `gpt-6-sol`.
- GPT-5.6: `gpt-5.6-sol`, `gpt-5.6-terra`, and `gpt-5.6-luna`.
- `gpt-5.6`, normalized by the gateway to `gpt-5.6-sol`.
- Latest Claude: `claude-fable-5-1`, `claude-opus-5-5`, and `claude-sonnet-5-5`.
- Earlier Claude 5: `claude-fable-5`, `claude-opus-5`, and `claude-sonnet-5`.
- Older GPT and Claude catalog entries returned by `/v1/models`.

`claude-mythos-5-1` was officially released on September 1, 2026, but is excluded from the general supported catalog because access is invitation-only through Project Glasswing. See the [official Mythos 5.1 availability documentation](https://platform.claude.com/docs/en/models/mythos-5-1/overview) (checked 2026-10-05).

The gateway does not define `gpt-6` or `gpt-6.1` aliases. The new catalog entries use documented release dates from the [OpenAI changelog](https://developers.openai.com/api/docs/changelog) and [Anthropic release notes](https://platform.claude.com/docs/en/release-notes/overview), represented at midnight UTC; these are not knowledge cutoff dates.

GPT ingress on all three endpoints uses the upstream Responses protocol, including tools. `/v1/chat/completions` accepts standard assistant `tool_calls`, tool messages with `tool_call_id`, and `reasoning_effort`; JSON and SSE responses preserve tool call IDs. Chat SSE terminates tool calls with `finish_reason: "tool_calls"`. Text present only in a final upstream message item is preserved without duplicating streamed deltas. The supported tool names remain `Edit`, `Read`, `Write`, and `Bash`.

For GPT-6 Astra and GPT-6.1 Sol, use `low` or higher reasoning effort; `none` and `minimal` are unsupported. GPT-6 Sol and GPT-6 Luna accept `none`. These upstream restrictions come from the [official GPT-6 guide](https://developers.openai.com/api/docs/guides/latest-model); catalog inclusion does not prove account access.

Anthropic requests preserve top-level fields such as `thinking`, `tool_choice`, and `output_config`. For the latest three Claude models, use `tool_choice: {"type":"auto"}` or `{"type":"none"}`: forced `any`/`tool` choices are rejected upstream. Fable 5.1 and Opus 5.5 require adaptive thinking; Sonnet 5.5 also accepts `between_tools` at `high` effort or below, while `disabled` and manual `enabled` thinking are rejected. See [Anthropic thinking documentation](https://platform.claude.com/docs/en/build-with-claude/thinking). The gateway forwards these controls and upstream errors rather than silently adjusting them.

Claude aliases default to:

```text
sonnet -> claude-sonnet-5-5
opus   -> claude-opus-5-5
haiku  -> claude-haiku-4-5-20251001
```

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
