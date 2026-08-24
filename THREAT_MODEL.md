# Threat model

## Scope

The supported deployment is one gateway process on the same trusted machine as one user. The gateway reads local credentials, accepts requests from local clients, and forwards them to configured upstream services.

Public hosting, shared credentials, multi-user isolation, and untrusted local clients are outside the supported scope.

## Assets

- Codex and Claude credentials;
- account identifiers;
- prompts, responses, and tool data;
- integrity of routed requests and returned responses.

## Trust boundaries

1. Local client to gateway.
2. Gateway to local credential stores.
3. Gateway to the configured Codex/ChatGPT upstream.
4. Gateway to the configured Anthropic upstream.
5. Build process to third-party Rust dependencies.

## Main risks and mitigations

| Risk | Current mitigation |
| --- | --- |
| Public or LAN exposure leaks subscription access | Listener is bound to `127.0.0.1`; public deployment is unsupported |
| Credentials leak through logs | Raw credentials are not logged; detailed body tracing is opt-in |
| Malicious custom upstream captures data | Upstream URLs are explicit configuration and should be treated as trust-boundary changes |
| Protocol drift causes silent or misleading success | Structured upstream errors, live acceptance scripts, and contract-focused tests |
| Tool-call translation changes behavior | Tool history and streaming paths have regression and live acceptance coverage |
| Dependency compromise | Committed lockfile, CI checks, and a minimum crate-age policy |
| Stale local credentials are forwarded | Credential parsing validates usable material; upstream authorization failures remain visible |

## Residual risks

- Local processes can call the gateway because it does not provide strong downstream authentication.
- Upstream private contracts may change independently of this project.
- Verbose diagnostics can expose sensitive content.
- Compatibility translation is intentionally incomplete and may lose unsupported fields.
- A compromised trusted machine can read the same credentials as the gateway.

Any proposal to expose the service beyond localhost requires a new threat review, downstream authentication, authorization, rate limiting, and secret-isolation design.
