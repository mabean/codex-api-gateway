# Security

Codex API Gateway is an experimental, local credential-bearing proxy. It has not been designed or audited for public, hosted, or multi-tenant deployment.

## Safe use

- Run it only on a trusted machine.
- Keep the listener on `127.0.0.1`.
- Loopback binding does not authenticate callers. CORS permits any origin, so remote web pages may be able to call the gateway where browser local-network restrictions permit it; see [THREAT_MODEL.md](THREAT_MODEL.md).
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

Live verification scripts enable gateway verbose mode by default. See [BUILD.md](BUILD.md) for disabling it and for the acceptance suite's permission-bypass behavior.

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

## Offline extracted SDK harness

The P1 harness reads a fingerprint-pinned local Claude Code binary without invoking it, extracts only declared SDK source ranges into temporary storage, and executes those ranges in Node. This is local verification of the SDK layer, not a general-purpose sandbox for unknown executables. It requires macOS `sandbox-exec` and refuses an unconfined fallback. The worker, gateway, Node transport and fixture inherit an OS loopback-only network policy. Both gateway upstreams are explicit local fixture URLs. Node rejects redirects and destinations outside its exact local gateway origin.

Children receive a minimal environment, temporary HOME/config/auth/log paths, and synthetic credentials. The policy denies the actual user's credential directories, keychain files, `/usr/bin/security`, and the securityd lookup. A protected synthetic file proves file-read denial without opening a real credential. A numeric TEST-NET connect must fail with OS `EPERM`; timeout/refusal does not count as isolation. Temporary source and diagnostic logs stay local. See [P1 verification](docs/claude-code-p1-verification-2026-11-20.md).
