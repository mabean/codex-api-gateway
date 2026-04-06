# SECURITY

## Security posture
This project is a local-first proxy for Codex/ChatGPT OAuth-backed access with:
- an OpenAI-compatible ingress surface
- an Anthropic-compatible ingress surface with streaming support

Current security goals:
- localhost-only bind by default
- no cloud relay
- no hidden telemetry
- no secret logging
- minimal outbound host set
- reproducible builds and pinned dependencies

## Sensitive assets
- OAuth access tokens
- account identifiers
- local auth files
- downstream prompts/responses

## Rules
- Never log raw bearer tokens or auth files
- Do not expose the proxy publicly by default
- Do not commit local auth files
- Treat reverse tunnels/public bind as advanced and unsafe until separately hardened
- Planning/scratch workflow files should remain local unless they become user-facing documentation

## Allowed outbound hosts (target policy)
- `chatgpt.com`
- auth-related OpenAI hosts only if explicitly needed by future refresh logic

## Current protocol/security notes
- OpenAI-compatible `/v1/chat/completions` is implemented
- Anthropic-compatible `/v1/messages` is implemented as a text/non-streaming baseline
- error responses are structured per API family
- usage accounting on compatibility surfaces is currently placeholder-level

## Reporting
If a change broadens token exposure, adds telemetry, introduces new outbound destinations, or turns local planning files into tracked repo artifacts, it should be treated as security-relevant and called out explicitly in review.
