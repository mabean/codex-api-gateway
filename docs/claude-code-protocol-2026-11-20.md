# Claude Code 2.1.329 protocol comparison — 2026-11-20

The Mac mini's global npm installation was updated from `2.1.289` to `2.1.329`. The executable and `npm ls -g` agree. npm reported `2.1.329` as `latest` and `2.1.311` as `stable`. The pinned installation used `--min-release-age=0` for that invocation. The preceding CLI was installed into a temporary directory for comparison. [Claude Code 2.1.329 release](https://github.com/anthropics/claude-code/releases/tag/v2.1.329)

## Local verification

Gateway revision `cd2276a` built with `cargo build --release --locked --offline`. The clients used isolated profiles, fixture credentials, and a local HTTP mock. The gateway's two upstream URLs both pointed to the mock. The `Read` check required the fixture path in the tool call, the fixture contents in a successful tool result with the matching call ID, and the final streamed marker in a successful CLI result. Each passing case made two mock inference requests.

| Route | CLI | Result |
| --- | --- | --- |
| Direct to the Messages mock, `--bare` with `Read` | `2.1.289`, `2.1.329` | Pass for both |
| Direct to the Messages mock, default tool set | `2.1.289`, `2.1.329` | Pass for both |
| Gateway to the Messages mock, API key and OAuth separately | `2.1.329` | Pass for both with `--bare` and `Read` |
| Gateway to the Codex mock, restricted and default tool sets separately | `2.1.329` | Pass for both |

These checks used synthetic streams and credentials. Live provider schema validation, real OAuth, and model behavior remain unverified. The direct client comparison used `claude-sonnet-5-5`. The Codex route used `gpt-6.2-sol` to select the adapter.

## Request differences

In the comparable Sonnet 5.5 captures, both versions posted to `/v1/messages?beta=true` with API version `2023-06-01` and SDK version `0.128.0`. Both used the same top-level body fields, including `max_tokens: 128000`, `thinking: {"type":"adaptive","display":"updates"}`, and `output_config.effort: low`.

| Surface | `2.1.289` | `2.1.329` |
| --- | --- | --- |
| Default tool set | 20 definitions with schemas in `tools` | 12 definitions including `ToolSearch` and `DeferredToolPlaceholder`. A system reminder lists deferred tools such as `NotebookEdit` and `Monitor`. |
| Default beta flags | Included `prompt-caching-scope-2026-01-05` | Omitted that flag and added `advanced-tool-use-2025-11-20`. The placeholder carries `defer_loading: true`. |
| Tool inputs | `Read` had no `allow_large`. `Agent` had no `effort`. | Adds `Read.allow_large` and `Agent.effort`. The gateway's Codex translation preserved the `Read` property. |

## Confirmed gateway gaps

**Deferred tool loading on the Codex route.** Replaying the same captured default-mode request through both routes showed that the Anthropic route forwarded `ToolSearch`, `DeferredToolPlaceholder`, `defer_loading`, and `advanced-tool-use-2025-11-20`. The Codex route forwarded only `Bash`, `Edit`, `Read`, and `Write`, using a filter that predates this update. A mock `ToolSearch` call with `query: "select:NotebookEdit"` returned successfully both directly and through the Anthropic route. The client sent a matching `tool_result` with `{"type":"tool_reference","tool_name":"NotebookEdit"}` and included the `NotebookEdit` schema in `tools` with `defer_loading: true`. Replaying that captured follow-up through Codex produced an empty `function_call_output` with the matching call ID and dropped the `NotebookEdit` schema. The exercise stopped after loading the schema. [Codex result normalization](../src/main.rs#L2585) · [Codex tool filter](../src/main.rs#L2754) · [Claude Code guidance on ToolSearch](https://code.claude.com/docs/en/llm-gateway-protocol#retry-without-tool-search)

**Per-message effort on both routes.** Both client versions sent a `role: "system"` entry with `output_config: {"effort":"low"}` inside `messages` for Sonnet 5.5. The capture used `per-turn-control-2026-07-01`. On the Anthropic route the gateway passed that beta value and the top-level effort, moved system text to the top-level `system`, and dropped the nested effort. On the Codex route the translated request had `reasoning: null`. A synthetic request with top-level `low`, nested `high`, and the documented `mid-conversation-output-config-2026-07-01` beta value showed the same loss. Omitting top-level `system` from that fixture made the Codex route return `400` because hoisting the effort-only entry produced an empty system content array. Per-message effort changes the level for later turns while reusing the cached prefix. [Request representation](../src/main.rs#L681) · [System message hoisting](../src/main.rs#L2167) · [Anthropic per-message effort](https://platform.claude.com/docs/en/build-with-claude/effort-per-message)

**Retry and rate-limit metadata on the Anthropic route.** An upstream `429` reached the caller with the original status and error body. The gateway dropped `Retry-After`, `x-should-retry`, `anthropic-ratelimit-unified-status`, and `request-id`. Claude Code uses the first three for retry and usage-limit decisions. In a separate fixture, a forwarded `400` naming `context_management` let the CLI retry successfully without that field and the corresponding beta flag. A `POST /v1/messages/count_tokens` probe returned `404`. The token-counting endpoint is optional and the client can estimate counts in its absence. [Gateway response assembly](../src/main.rs#L3254) · [Claude Code gateway compatibility guide](https://code.claude.com/docs/en/llm-gateway-protocol)

**OAuth attribution.** The gateway still sends `claude-cli/2.1.119` upstream and prepends its own attribution block alongside Claude Code's `2.1.329` block. The mock accepted the request. The official guide ties first-party attribution stripping to the original attribution block remaining first. [OAuth system transformation](../src/main.rs#L2408) · [Claude Code gateway compatibility guide](https://code.claude.com/docs/en/llm-gateway-protocol#system-prompt-attribution-block)

## Static inspection of the installed clients

The macOS arm64 executables contain a Bun module graph in the Mach-O `__BUN,__bun` section. The table lists embedded minified source for all 3,414 JavaScript modules in `2.1.329` and all 2,184 in `2.1.289`. Neither graph listed JavaScript source maps. The analysis extracted the relevant modules, parsed the main bundles as JavaScript, and followed the request builder, SDK calls, and response-parser branches. It used source text from the binaries without executing the extracted bundles. [Bun module graph format](https://github.com/oven-sh/bun/blob/main/src/standalone_graph/StandaloneModuleGraph.rs)

| Installed binary | SHA-256 | Main bundle inspected |
| --- | --- | --- |
| `2.1.329` macOS arm64 | `8c1729568b19cdadedd0a978bf692f9f279ef554c8a333c4e132b94fd8725550` | `chunk-hg1yqkjk.js`, main Messages request and SSE parser at line 2042 |
| `2.1.289` macOS arm64 | `03d66745e3bb69ec727d66023696f3820bc0a00a8a5ba725eb6706d0c67cbe69` | `chunk-x2pwb441.js`, corresponding code at line 1913 |

The findings below come from code inspection. The runtime checks earlier in this report cover a smaller set of behaviors.

| Contract surface | Client code | Gateway revision `cd2276a` |
| --- | --- | --- |
| SSE timing and usage | `2.1.329` consumes individual events and aggregates `usage` from the stream, including cache token counts when supplied. | The Codex route reads the entire upstream body before rendering. Its Anthropic SSE emits zero token counts in `message_start` and `message_delta` even when the upstream parser has captured usage. [Upstream read](../src/main.rs#L1539) · [SSE renderer](../src/streaming.rs#L540) |
| Citation and thinking deltas | In the main parser, `2.1.329` appends `citations_delta` to text blocks where `2.1.289` ignored the delta. `2.1.329` also adds `thinking_delta.estimated_tokens` to the running count. | The Anthropic upstream route streams the response body through. The Codex renderer has no matching canonical events for citations or thinking. [Canonical event types](../src/streaming.rs#L8) |
| Turn-scoped system messages | Both versions can serialize `role: "system"` with `clear_at: "next_user_message"` and have handling for API rejection of that shape. | The `AnthropicMessage` type retains `role` and `content`. Ingress drops `clear_at` and hoists the content into the top-level system prompt on both routes, losing the message's place and scope. [Message type](../src/main.rs#L697) · [System message hoisting](../src/main.rs#L2167) |
| SDK retry metadata | `2.1.329` reads `retry-after-ms` before `Retry-After`, and uses `x-should-retry` to override the status-based retry decision. The SDK code is in `chunk-76t3g1yn.js` at line 48. | The Anthropic route's response assembly forwards the content type, so `retry-after-ms` is also lost. [Gateway response assembly](../src/main.rs#L3254) |
| Token counting and model picker | `2.1.329` has an actual `beta.messages.countTokens` caller in the main bundle and a gateway-specific `GET /v1/models?limit=1000` caller in `chunk-aq3ms068.js`. SDK methods for other resources alone do not establish that the CLI calls them. | The token count request returns the `404` recorded above. `/v1/models` has an explicit handler. [Gateway routes](../src/main.rs#L3163) |

Static inspection can enumerate explicit client-side branches and their predicates for these binaries. Provider acceptance, remotely assigned flags, and account-specific availability require separate evidence. The extracted bundles and analysis scratch files are temporary. The repository retains the binary fingerprints, locations, and interface-level findings.
