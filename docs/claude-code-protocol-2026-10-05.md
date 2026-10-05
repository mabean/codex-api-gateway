# Claude Code update and protocol comparison — 2026-10-05

Status: confirmed local CLI update and mock compatibility checks; real upstream acceptance remains unverified. Sources: installed package metadata, executable version output, a local Python HTTP mock, gateway revision `1e00eea`, and the official pages linked below. No real model calls were made.

## Update

Claude Code was updated from `2.1.87` to `2.1.289`, the [official latest release](https://github.com/anthropics/claude-code/releases/tag/v2.1.289) published on October 3, 2026. Both the executable and npm package metadata report `2.1.289`.

The built-in updater initially announced `2.1.289` but installed `2.1.283`. The environment's npm resolution cutoff was seven days before the current date. A pinned install of `@anthropic-ai/claude-code@2.1.289` from `https://registry.npmjs.org`, with `--min-release-age=0` for that command only, completed the requested update. The default npm cutoff remained in place. The installation is still managed through the global npm package; the newer package exposes a native executable instead of the old JavaScript entrypoint.

## Method and results

The old installed package was temporarily preserved for comparison. Both clients used an isolated configuration directory, fake API credentials, localhost endpoints, low effort, a three-turn cap, and a 45-second process timeout. Customizations and MCP servers were disabled. The mock returned a streamed `Read` tool call, accepted its matching result containing a fixture marker, then returned a streamed final marker. Success required the actual file content in the follow-up request and a successful CLI terminal result. An offline check of all seven captures also verified the exact matching `tool_use_id` or `call_id` and successful tool-result status.

| Check | Result |
| --- | --- |
| CLI `2.1.87` directly against the Anthropic-shaped mock, bare mode | PASS |
| CLI `2.1.289` directly against the same mock, bare mode | PASS |
| CLI `2.1.289` through gateway to a Codex-shaped mock, bare mode | PASS |
| CLI `2.1.289` through gateway to an Anthropic-shaped mock, fake API-key auth | PASS |
| CLI `2.1.289` through gateway to an Anthropic-shaped mock, fake OAuth auth | PASS |
| CLI `2.1.289` directly against the mock, built-in tools in safe mode | PASS |
| CLI `2.1.289` through gateway to a Codex-shaped mock, built-in tools in safe mode | PASS |

Each check made two mock upstream requests. The model names selected routing only: no GPT, Astra, or Claude inference occurred. Fake Codex credentials were used by the gateway as well. These checks validate request construction, translation, streamed tool/result replay, and client parsing; the mock does not enforce the real provider's model, schema, thinking, or authentication policies.

## Observed request differences

The comparable bare-mode captures used the same custom model ID and flags in both versions.

| Surface | `2.1.87` | `2.1.289` |
| --- | --- | --- |
| SDK version header | `0.74.0` | `0.128.0` |
| Client User-Agent version | `2.1.87` | `2.1.289` |
| `thinking` | `type: adaptive` | Adds `display: updates` |
| Additional beta flags | Absent | `thinking-token-count-2026-05-13`, `mid-conversation-system-2026-04-07`, `thinking-display-updates-2026-08-18` |
| Bash input schema | Includes `run_in_background` | Omits `run_in_background` |

In these captures, the Messages path remained `/v1/messages?beta=true`, the API version remained `2023-06-01`, and the top-level body keys were unchanged: `context_management`, `max_tokens`, `messages`, `metadata`, `model`, `output_config`, `stream`, `system`, `thinking`, and `tools`. Low effort, streaming, a 32,000 output-token limit, and the observed ephemeral cache-control shapes were unchanged. Bare-mode tool names were unchanged; this is not a comparison of every optional tool or plugin.

The gateway preserved the new `thinking.display` value and merged the new beta flags on the Anthropic route. [Anthropic's Sonnet 5.5 guide](https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/prompting-claude-sonnet-5-5) documents this display mode for progress updates. The mock did not generate progress-update thinking blocks, so their rendering and replay are outside these results.

## Observed Codex tool limitation

In safe mode, the new client advertised 20 built-in tools, including `Agent`, `ListAgents`, `SendMessage`, `Workflow`, and scheduling tools. The gateway's Codex request contained four: `Bash`, `Edit`, `Read`, and `Write`. `normalize_tools_for_codex` explicitly filters by these four names; the same whitelist existed in revision `2190e7a` before this model-update work.

The successful mock round trip exercised `Read`. It does not establish availability of the other client tools through Codex. In particular, the presence of an `Agent` definition in Claude Code does not make it available to a GPT model through the current adapter. This limitation matters when assessing the newer client's broader workflows alongside the core file-tool subset.

## Observed OAuth metadata mismatch

The [current OAuth adapter](../src/main.rs) still synthesizes `claude-cli/2.1.119` and a billing system block with `cc_version=2.1.119.unknown`. The new client already supplies its own billing block with version `2.1.289`. The mock captured both blocks in the gateway's OAuth request, whereas the API-key route retained only the client billing block.

This mismatch and duplication are confirmed locally. Whether the real Anthropic OAuth service rejects them is unverified; a permissive mock cannot answer that. Changing or removing these identity blocks requires a separate compatibility decision, rather than treating the local PASS as proof of real OAuth acceptance. No gateway identity-header change was made in this investigation.

## Limits

The historical [model verification](model-verification-2026-10-05.md) used Claude Code `2.1.87`; those live outcomes were not relabeled as results for `2.1.289`. This investigation does not establish real account entitlements, model behavior, extended thinking/progress replay, dynamic MCP tooling, mods, or all permission modes. Raw request bodies, isolated profiles, the old package copy, and test logs were temporary; only this redacted observation is retained.
