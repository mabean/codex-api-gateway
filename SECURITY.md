# Security

Codex API Gateway is an experimental, local credential-bearing proxy. It has not been designed or audited for public, hosted, or multi-tenant deployment.

## Safe use

- Run it only on a trusted machine.
- Keep the listener on `127.0.0.1`.
- Do not expose it through a tunnel, reverse proxy, shared LAN address, or public hostname.
- Protect `~/.codex/auth.json`, Claude credentials, OAuth tokens, and API keys as secrets.
- Review custom upstream URLs before use; they receive credentials and request content.
- Keep dependencies and the committed `Cargo.lock` current.

## Sensitive data

The gateway can process:

- OAuth and API credentials;
- account identifiers;
- prompts, responses, and conversation history;
- tool names, arguments, and results.

Normal logs must not contain raw credentials. Detailed wire diagnostics are disabled unless `CODEX_PROXY_VERBOSE=1` is set.

Verbose mode is intended only for short-lived local debugging. It may include prompts, responses, tool arguments, and upstream SSE fragments. Never attach verbose logs to a public issue without inspecting and redacting them first.

## Network boundary

The process listens on localhost and connects to the configured Codex/ChatGPT and Anthropic upstreams. Changing either upstream base URL changes the trust boundary: the new destination may receive authentication headers and user content.

## Security-relevant changes

Treat a change as security-sensitive if it:

- changes listener exposure or downstream authentication;
- adds an outbound destination;
- changes credential loading, forwarding, or refresh behavior;
- logs request or response bodies;
- adds telemetry or persistent storage;
- broadens the supported deployment model.

Such changes should include an updated threat assessment and tests for secret handling and failure behavior.
