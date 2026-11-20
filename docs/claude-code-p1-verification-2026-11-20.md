# Extracted Claude Code SDK P1 verification — 2026-11-20

Status: implemented and locally verified; phase acceptance remains separate. Source: the P1 assignment against the Claude Code compatibility specification, gateway baseline `cd2276af`. No production Rust changes, CLI invocation, model calls, updates or dependency installations were used.

## Reproduction and provenance

Run the command in [BUILD.md](../BUILD.md#offline-extracted-claude-code-sdk-contract-p1). It requires a prebuilt gateway, local Node and the official Claude Code `2.1.329` Darwin arm64 executable with SHA-256 `8c1729568b19cdadedd0a978bf692f9f279ef554c8a333c4e132b94fd8725550`.

The [profile](../scripts/fixtures/claude_contract/2.1.329-macos-arm64.json) pins the Mach-O/Bun graph module ID 404, `/$bunfs/root/chunk-76t3g1yn.js`, byte offset 292647493, length 349050, module hash and twelve unchanged source-range hashes. Offsets in each range are module-relative byte offsets. The local manifest records all ranges, symbols, binary path/hash, gateway executable hash and executed layer. Extracted source and traces remain in the printed temporary directory, never in Git.

Executed layer: embedded beta Messages SDK. The unchanged `zi.create` builds the request path/body/headers; `zt.fromSSEResponse` and `cc` decode SSE; `ff.createMessage`, accumulator `uf` and `ff.finalText` handle the stream and terminal text. The main CLI parser is a separate unexecuted layer. A Node fetch transport provides the real HTTP boundary with exact-origin loopback validation, redirect rejection and a deadline. Explicit load adapters provide imported error classes/abort classification, the stream-helper header constant and throwing sentinels for unused `nd`/`rb` dependencies. Supporting resource, header, text and private-field helpers execute from the original extracted source. No handwritten SSE parser replaces SDK behavior.

## Observations

| Criterion | Actual local evidence |
| --- | --- |
| P1-AC1 | Pinned installed binary extracted and loaded. Seven synthetic unit tests passed, including changed binary, missing required graph module, mismatched source range and invalid table bounds. Errors are explicit and nonzero. |
| P1-AC2 | Two original SDK `POST /v1/messages?beta=true` calls traversed the unchanged gateway to the Anthropic fixture (`/v1/messages`). Exact terminal text was `P1 terminal text`. Deliberately starting SSE with `content_block_delta` produced `Unexpected event order, got content_block_delta before "message_start"` from the original accumulator. Reports contain expected/actual outcomes and parser/branch names. |
| P1-AC3 | The sandbox worker's numeric `192.0.2.1:9` connect failed with OS `EPERM` (1), while local fixture identity passed. Protected-file read failed with permission denial and external HTTP redirect was rejected. Normal cleanup closed both owned ports/processes; an unrelated listener stayed alive. The intentional post-start failure probe also verified owned cleanup and unrelated-listener survival. |

The first protected-file probe exposed macOS `/var` versus `/private/var` path canonicalization; the harness now resolves paths before generating sandbox rules. The first redirect check expected a different Node error spelling; it now accepts the actual explicit redirect-policy errors, never a generic network failure. Both defects were discovered by mandatory probes before successful runs.

The orchestrator's clean baseline passed formatting, 68 Rust tests, 8 existing Python tests, 227 crate metadata checks, clippy and release build. Focused checks for the harness are recorded above; final whole-change checks are the orchestrator's responsibility. The dedicated fixture uses only synthetic API keys and temporary HOME/auth/log paths. Real credential content was never read by a probe.

## Limits

This is P1 SDK-layer verification for the Anthropic-shaped route. Both gateway upstream URLs point explicitly to the same controlled local fixture, but no Codex response scenario is claimed. Retry policy, main CLI parser behavior, tools, dialogue continuation, usage semantics and upstream entitlement remain for later phases or live acceptance. A trusted pinned source closure plus macOS policy is required; unsupported platforms fail closed. The VM is a loading boundary, not a security boundary for arbitrary hostile JavaScript.

A normal run exits zero only when every mandatory probe and SDK scenario passes. `--fail-after-start` intentionally exits nonzero after starting the gateway and fixture; inspect the cleanup fields. Timeouts kill only the owned process group. Raw temporary evidence is local and ephemeral; the structured observations here are the durable record.
