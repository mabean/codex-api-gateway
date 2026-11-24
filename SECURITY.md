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

Responses precommit errors use fixed safe messages and codes. Upstream HTTP error bodies are not read or reflected; only one valid `Retry-After` on final HTTP 429 can be forwarded, and `Connection` nominations exclude it. Arbitrary upstream headers, cookies and request IDs are excluded. Codex send/read logs record failure class and attempt information without raw transport diagnostics; retry logs use error codes. Native successful output and native streaming events remain user data, including upstream-provided error events after commitment. See [C2 offline verification](docs/codex-responses-c2-verification-2026-11-23.md).

Verbose mode is intended only for short-lived local debugging. It may include prompts, responses, tool arguments, and upstream SSE fragments. Never attach verbose logs to a public issue without inspecting and redacting them first.

Live verification scripts enable gateway verbose mode by default. See [BUILD.md](BUILD.md) for disabling it and for the acceptance suite's permission-bypass behavior.

## Network boundary

The process listens on localhost and connects to the configured Codex/ChatGPT and Anthropic upstreams. Changing either upstream base URL changes the trust boundary: the new destination may receive authentication headers and user content.

Codex ToolSearch translation accepts request-local discovery evidence only for declared Bash/Edit/Read/Write schemas. A matching tool-use ID activates a deferred declaration; arbitrary schema text does not. Discovery does not grant execution permission: the client owns validation, permission decisions and execution. The P4 harness exercises original client wrappers with a synthetic fixture executor, including denial with zero executions; it does not execute a production file or shell tool.

Discovery authorization is typed internal Messages state excluded from JSON deserialization. Public Chat/Responses tool objects cannot opt into discovery through `_codex_client_discovery` or other caller-supplied flags; Chat retains the four executor names. Native Responses accepts caller-declared function schemas without granting internal Messages discovery authorization.

The direct Anthropic Messages and count-tokens routes share credential resolution and the configured upstream origin. Only the supported `beta=true` query is forwarded. Upstream responses expose an allowlist: `content-type`, `Retry-After`, `retry-after-ms`, `x-should-retry`, `request-id` and `anthropic-ratelimit-unified-*`. `Connection`-nominated headers are excluded even when otherwise allowed. Cookies, arbitrary upstream headers and hop-by-hop headers are not relayed. Direct JSON fidelity preserves client fields without treating their content as gateway configuration. OAuth attribution is preserved when supplied; count-tokens does not receive a synthetic Messages prompt.

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

The [P3 runner](scripts/claude_contract_p3.py) extends that boundary to fingerprinted retry orchestration and bounded context-rejection seams. It additionally disables gateway keychain lookup using the supported environment control. All OAuth/API credentials are synthetic; neither real token refresh nor provider entitlement is tested. See [the P3 evidence and adapter limits](docs/claude-code-p3-verification-2026-11-21.md).

C3 request boundary (2026-11-24): native Responses rejects unsupported fields and malformed options with a fixed safe message and field-only `error.param`, before network access. Function names and schemas are caller data; the gateway does not execute them. URL suffix normalization preserves the configured destination and path prefix. Ingress body fields and authorization headers cannot replace the configured Codex bearer/account or destination. See [offline C3 evidence](docs/codex-responses-c3-verification-2026-11-24.md).
