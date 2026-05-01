# Anthropic-Compatible Gateway Notes

Date: 2026-05-02
Status: streaming/tool-use hardening in progress

## Why this matters

Some tools expect Anthropic-style endpoints, especially `/v1/messages`, rather than OpenAI-style endpoints. Supporting Anthropic-compatible ingress lets clients such as Claude Code target the local Codex-backed gateway.

## Current baseline

Implemented:
- `POST /v1/messages`
- Anthropic request → internal chat conversion
- Anthropic `tools` preservation into the Codex Responses payload
- Anthropic assistant `tool_use` and user `tool_result` history conversion
- Structured Anthropic error envelopes
- Request validation for key Anthropic fields
- Shared upstream transport and SSE text extraction logic
- Streaming response rendering for text and `tool_use` blocks

Current constraints:
- usage accounting is still placeholder-level on compatibility surfaces
- multimodal Anthropic content is not a supported goal yet
- tool-use behavior still needs live Claude Code acceptance verification after each contract change
- full Anthropic protocol parity remains out of scope for the current phase

## Design direction

- Keep one internal canonical request model.
- Build thin ingress adapters:
  - OpenAI Chat Completions / Responses
  - Anthropic Messages
- Reuse one Codex transport layer.
- Keep response/error translation explicit per API family.
- Keep sensitive wire diagnostics behind `CODEX_PROXY_VERBOSE=1`.

## Error translation baseline

Anthropic responses return structured envelopes like:
- `invalid_request_error`
- `authentication_error`
- `api_error`

Validation is explicit for:
- missing `model`
- empty `messages`
- missing/zero `max_tokens`
- invalid Anthropic message roles

## Remaining future work

1. Add HTTP-level integration tests for `/v1/messages`.
2. Improve token/usage accounting when upstream contract is clearer.
3. Expand live Claude Code acceptance tests around multi-step tool loops.
4. Document the supported Anthropic subset in README.
