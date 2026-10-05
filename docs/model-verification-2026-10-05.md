# Model update verification — 2026-10-05

Status: latest implementation approved by independent local review; latest-patch live verification is unconfirmed, and Anthropic live acceptance is blocked by unavailable credentials. Source: working-tree changes based on Git revision `2190e7a`, local Rust/Python checks, and earlier bounded gateway and Claude Code runs on macOS. The local and live snapshots below are distinct. This is a dated observation, not a claim of universal account availability.

## Implementation and source contracts

The catalog adds `gpt-6.1-sol`, `gpt-6-astra`, `gpt-6-luna`, `gpt-6-sol`, `claude-fable-5-1`, `claude-opus-5-5`, and `claude-sonnet-5-5`. Default aliases move `sonnet` and `opus` to 5.5; `haiku` and `gpt-5.6` retain their previous targets. No `gpt-6` or `gpt-6.1` alias is introduced. `claude-mythos-5-1` is an official September 1, 2026 release but is intentionally excluded from the general supported catalog: the [official Mythos documentation](https://platform.claude.com/docs/en/models/mythos-5-1/overview) limits access to invited Project Glasswing participants (checked 2026-10-05).

Release-date metadata uses the documented dates at midnight UTC: GPT-6.1 Sol September 29, Astra September 3, Sol/Luna September 22; Claude Fable 5.1 September 1, Opus 5.5 September 22, Sonnet 5.5 September 28. Sources accessed 2026-10-05: [OpenAI changelog](https://developers.openai.com/api/docs/changelog) and [Anthropic release notes](https://platform.claude.com/docs/en/release-notes/overview). Knowledge cutoffs are not used as creation dates.

[OpenAI's GPT-6 guide](https://developers.openai.com/api/docs/guides/latest-model) requires Responses for Astra/6.1 Sol tools and excludes `none`/`minimal` effort for those models. Sol/Luna support `none`; reasoning with tools uses Responses. The gateway uses Responses for all GPT ingress endpoints. [Anthropic thinking documentation](https://platform.claude.com/docs/en/build-with-claude/thinking) excludes forced `any`/`tool` choices on the three new Claude models. Fable 5.1/Opus 5.5 use always-on adaptive thinking; Sonnet 5.5 also accepts `between_tools` at `high` effort or below. These restrictions are forwarded to the upstream rather than silently rewritten.

Concrete compatibility repairs preserve Anthropic top-level parameters, JSON tool calls and their IDs, standard Chat tool-result history, and GPT stream argument deltas whose upstream item ID differs from the tool call ID. Responses JSON emits a valid `fc_` item ID separately from `call_id`. Chat SSE assigns stable distinct indices to parallel calls. Empty upstream successes remain errors.

## Local checks

| Check | Result |
| --- | --- |
| `cargo fmt --check` | PASS |
| `cargo test --all-targets` | PASS, 68 tests on the latest parser patch |
| `python3 -B -m unittest discover -s scripts -p 'test_*.py'` | PASS, 8 tests (including adversarial subcases), also enforced in CI |
| `python3 scripts/check_crate_age.py 7` | PASS in the preceding repair stage: 227 registry crates checked, no age violations; dependencies and lockfile remain unchanged |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| Shell/Python syntax, repository-relative documentation links, `git diff --check` | PASS |

No dependency versions or lockfile entries changed. Tests use synthetic credentials and cover routing, aliases, preserved controls, JSON tools, call/result binding, stream item-ID mapping, and parallel-call indices.

## Latest parser patch and independent review

The last repair tracks emitted text separately by output item and content part. Regression coverage checks multiple final messages, mixed delta/fallback items, multiple parts (including identical text), done/item-done deduplication, and missing suffix recovery through production JSON/SSE renderers. Independent read-only review approved this implementation with 68 Rust tests and eight Python tests passing. Formatting, clippy, release build, and diff checks also passed on the latest source.

The executor hit a Codex usage limit before its final handoff. No post-patch live result is accepted as evidence here. A bounded representative GPT-6.1 Sol pass had been requested, with no further Astra or other expensive-model runs; its outcome is unconfirmed. The previous four-model acceptance below was completed before this last parser patch and must not be read as a rerun of the latest binary.

## Live results before the last parser patch

The protocol suite used 60-second request limits; Claude Code used a 120-second process limit and eight-turn cap. Runtime: macOS, Rust/Cargo 1.94.1, Python 3.14.3, Claude Code 2.1.87. Dedicated localhost gateways and unique temporary workspaces were used. Gateway wire diagnostics stayed disabled. All owned gateway/Claude child processes were stopped by the suite; raw artifacts from the repair rerun were removed after recording the redacted outcomes.

| Model | Protocol matrix | Claude Code Edit/read-back + Bash |
| --- | --- | --- |
| `gpt-6.1-sol` | PASS, 12/12 | PASS, 2/2 |
| `gpt-6-astra` | PASS, 12/12 | PASS, 2/2 |
| `gpt-6-luna` | PASS, 12/12 | PASS, 2/2 |
| `gpt-6-sol` | PASS, 12/12 | PASS, 2/2 |
| `claude-fable-5-1` | 4/4 BLOCKED, auth prerequisite not retried | Both BLOCKED, auth prerequisite not retried |
| `claude-opus-5-5` | 4/4 BLOCKED by the shared auth prerequisite | Both BLOCKED |
| `claude-sonnet-5-5` | 4/4 BLOCKED by the shared auth prerequisite | Both BLOCKED |

For each GPT model, the 12 protocol scenarios are text and tool round trips, both JSON and SSE, on `/v1/messages`, `/v1/responses`, and `/v1/chat/completions`. Each tool test requires a real `Read` call with valid arguments and a nonempty ID, sends the result with that exact ID, and requires a newly generated result nonce in the final response. The nonce is generated after receiving the call. Claude Code checks require actual `Read` → `Edit` → `Read` calls targeting the temporary `note.txt`, a successful matching initial Read result, a matching successful Edit result, then a new Read issued after that Edit result whose matching successful text result contains the nonce. The final file and terminal response must also contain the marker. Bash requires a successful matching tool result with the command-output marker and terminal success. Chat SSE tests require `tool_calls` as the tool terminal reason and `stop` for text, in addition to `[DONE]`.

Default `sonnet`, `opus`, and `haiku` live probes were BLOCKED by Anthropic auth. Environment overrides `sonnet` → `gpt-6.1-sol` and `haiku` → `gpt-6-luna` both PASSED with the effective response model checked. The `opus` → `claude-fable-5-1` override remained BLOCKED. Local regression tests verify all three defaults and cross-upstream overrides.

Previous repair-stage rerun totals: protocol **50 PASS, 0 FAIL, 16 BLOCKED** (66 scenarios); Claude Code **8 PASS, 0 FAIL, 6 BLOCKED** (14 scenarios). All 48 GPT protocol scenarios, eight GPT Claude Code scenarios, and two GPT alias overrides were executed again after the first three review fixes, before the last per-item/content parser patch. These results do not establish live acceptance of the latest binary. The runner’s existing Anthropic blocker was preseeded from the confirmed missing-auth prerequisite, so no Anthropic calls or login attempts were repeated. Both suites intentionally exited nonzero because blocked checks remain incomplete. The earlier pre-review run had protocol 50 PASS / 1 FAIL / 15 BLOCKED and Claude Code 8 PASS / 1 FAIL / 6 BLOCKED, including failed auth prerequisite probes.

### Anthropic blocker

The pre-review auth probes returned the structured gateway error `HTTP 400: no Anthropic upstream auth found`. With inherited gateway placeholder variables removed, the supported command `env -u ANTHROPIC_API_KEY -u ANTHROPIC_BASE_URL claude auth status` returned `loggedIn: false`, `authMethod: none`. The inherited `ANTHROPIC_API_KEY` was the local gateway placeholder, and no configured upstream key/OAuth token or usable Claude local credential was found. `claude auth login --help` confirms supported subscription/Console/SSO login options, but no authenticated session or device authorization was available to complete login autonomously. No local desktop login, password in chat, token copying, or credential-store edits were requested or performed.

An initial exploratory probe using a placeholder bearer received `401 Invalid bearer token`. The suites use placeholder-aware `x-api-key` ingress to expose the actual missing-auth prerequisite and skip its dependent scenarios. Anthropic model entitlements and thinking/tool contracts therefore remain unverified live, while parameter-preservation and routing regressions pass locally.

### Defects reproduced and repaired

The first exploratory GPT-6.1 run reproduced a Responses JSON tool replay rejection (its `id` incorrectly used `call_id`) and duplicated/misbound Chat SSE arguments (`item_id` differs from `call_id`). The preceding live suites passed both repaired paths for all four GPT models. Independent review then reproduced three further defects, all repaired before the repeat acceptance: final `response.output_item.done` message text was lost by the production canonical parser when deltas were absent; Chat SSE used the Anthropic `tool_use` terminal reason; and the Claude acceptance runner could accept tools operating on another file or stale Read results. Production parser/renderer regressions cover final-item and output-text-done fallback without duplicate deltas. Adversarial Python traces now reject wrong paths, stale results, missing/failed/mismatched IDs, Read calls issued before Edit succeeds, and incorrect replacement arguments; these tests run in both CI platforms. The runner also handles upstream Responses SSE whose completed event omits output items, assembling the actual `response.output_item.done` events. Initial upstream failures describe real exploratory calls; the independent-review regressions use synthetic streams and adversarial traces. Only the repeated live outcomes are counted in the repair totals.

## Reproduction and limits

Use the [live verification procedures](../BUILD.md) and the two entrypoint scripts; both use [live_gateway_checks.py](../scripts/live_gateway_checks.py). They continue per scenario, record PASS/FAIL/BLOCKED in a temporary `summary.json`, stop repeated upstream authentication failures, and return nonzero if verification remains incomplete.

These checks establish short text and core tool flows for the tested account, not image, audio, built-in hosted tools, long-context behavior, performance guarantees, or advanced Claude beta features. Anthropic capabilities and entitlements remain unverified until usable upstream credentials are available. No commit or push was performed.
