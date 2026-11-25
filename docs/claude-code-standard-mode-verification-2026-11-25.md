# Claude Code standard-mode verification — 2026-11-25

Status: observed live success for four bounded scenarios, plus local synthetic regression evidence. Claude Code 2.1.329, without `--bare`, completed `Read → Edit → Read` and `Bash` through the real Codex backend on both `gpt-6.2-sol` and `gpt-6.2-astra`.

Sources: dated, redacted phase receipts `initial-standard-live.json` and `final-standard-live.json`, the 2026-11-24 base observation, and the local P4 comparison. Raw transcripts, OAuth contents and extracted client code remain private temporary artifacts. This record describes executed checks; independent review and phase approval are separate.

## Comparison

The immutable base revision `58cfe440ed4a0880534a5f44a94152c54e4ee2bc`, tested on 2026-11-24, rejected both ordinary-mode Sol scenarios with HTTP 400: `Codex translation cannot preserve system content inserted within conversation history (field: messages[].content)`. All 12 CLI requests in that run failed before Codex routing. The captured request shape included non-temporary system text after a user message.

On 2026-11-25 the repaired gateway accepted that ordered context. The initial Sol run reached Codex on all five Messages requests: Read and Edit succeeded, but the model skipped the requested second Read after Edit advised that rereading was unnecessary. The strict file scenario failed despite terminal `end_turn`; Bash passed. Explicitly strengthening the read-back prompt still produced Read and Edit without the second Read on Sol and Astra. These were fixture failures, not evidence of a completed three-tool sequence.

The final fixture makes read-back retrieve new information. It writes `before` on line one and an independent random challenge on line two. The first Read requests `offset=1, limit=1`; Edit changes only line one; a separate Read requests both lines. Validation requires that the challenge was absent from the first successful target Read, both expected values occur in the successfully correlated post-Edit Read, both lines match the file on disk, and the successful terminal answer equals those two lines after trimming. Target paths, tool call/result IDs, error flags and execution order remain checked. Failure messages do not disclose either random value.

## Final live result

| Model | Scenario | Time | CLI turns | Observed successful tools | Terminal |
| --- | --- | --- | --- | --- | --- |
| `gpt-6.2-sol` | File edit/read-back | 18.65 s | 4 | Read, Edit, Read | `end_turn` |
| `gpt-6.2-sol` | Bash | 4.76 s | 2 | Bash | `end_turn` |
| `gpt-6.2-astra` | File edit/read-back | 14.05 s | 4 | Read, Edit, Read | `end_turn` |
| `gpt-6.2-astra` | Bash | 6.15 s | 2 | Bash | `end_turn` |

All four terminal answers matched the expected output after trimming. In both file scenarios the challenge was absent from the first Read and Edit results and present in the final Read result. All 12 Messages requests selected Codex; none selected Anthropic. The owned gateway listener was confirmed closed after the run.

The gateway used an ephemeral `127.0.0.1` listener, existing file-backed Codex OAuth and `https://chatgpt.com/backend-api/codex/responses`. The actual CLI ran with an empty isolated HOME/profile, strict MCP configuration, disabled extra traffic, auto-memory and session persistence, and an explicit temporary directory. A native macOS sandbox limited CLI outbound access to localhost and writes to temporary files, excluding access to `~/.codex`. Request and CLI limits were 90 and 180 seconds respectively, with an eight-turn cap. Verbose gateway wire logging was off. See the [runner](../scripts/live_gateway_checks.py) and [verification procedure](../BUILD.md#live-verification).

## Local regression evidence and limits

The updated [P4 contract](claude-code-p4-verification-2026-11-21.md) passed 30/32 cases against the immutable base: exactly `mid_history` and `standard_read` failed with the original system-history refusal. The candidate passed all 32. Both synthetic runs verified isolation, owned-process cleanup, closed owned ports and survival of an unrelated listener. Four focused Rust `standard_` tests passed. After the challenge fixture repair, all 13 live-runner regression tests and all 47 Python tests passed, as did `git diff --check`.

The local tests establish ordered system representation and regression controls; the live run establishes these short real tool loops for the two listed models and pinned client. Live ToolSearch execution was not observed. Interactive permissions, long conversations, other client versions, other model IDs and direct Anthropic acceptance were not established by this run. Unsupported temporary system scope remains refused. No deployment occurred.

## Fingerprints

| Artifact | SHA-256 |
| --- | --- |
| Claude Code 2.1.329 macOS arm64 | `8c1729568b19cdadedd0a978bf692f9f279ef554c8a333c4e132b94fd8725550` |
| Immutable base gateway release | `a46640ee1521c4f463a8054bd6ff5908f8c723110750ec2aecc618add9b9dac8` |
| Candidate gateway release, initial and final live runs | `888a242e8c909e8039ed474e227be3820f6ece7c06c544521be557c6b45680a4` |
| Final live runner | `7d6dbed17abaf08820b8e294bd2ce332d1ae66f7e43feb6bb26fa2cb56eaa32c` |
