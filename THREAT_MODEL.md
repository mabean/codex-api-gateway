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
6. Browser origin to the localhost gateway. A page from a remote origin may be able to send requests, subject to the browser's local-network restrictions.

## Main risks and mitigations

| Risk | Current mitigation |
| --- | --- |
| Public or LAN exposure leaks subscription access | Listener is bound to `127.0.0.1`; public deployment is unsupported |
| Credentials leak through logs | Raw credentials are not logged; detailed body tracing is opt-in |
| Malicious custom upstream captures data | Upstream URLs are explicit configuration and should be treated as trust-boundary changes |
| Protocol drift causes silent or misleading success | Structured upstream errors, live acceptance scripts, and contract-focused tests |
| Tool-call translation changes behavior | Tool history and streaming paths have regression and live acceptance coverage |
| Dependency compromise | Committed lockfile, CI checks, and a best-effort minimum crate-age check; metadata lookup failures are skipped with warnings |
| Stale Codex credentials are forwarded | Auth parsing checks field presence, not token expiry or nonempty values; upstream authorization failures remain visible |

## Residual risks

- Local processes can call the gateway because it does not provide strong downstream authentication.
- CORS permits any origin and the gateway does not authenticate downstream callers. A remote web page may be able to call the localhost listener where browser restrictions permit it; loopback binding alone does not establish caller trust.
- Codex credentials are loaded at startup without local expiry validation or automatic reload. Updating the auth file requires restarting the gateway; token validity is determined by the upstream.
- Upstream private contracts may change independently of this project.
- Verbose diagnostics can expose sensitive content.
- Compatibility translation is intentionally incomplete and may lose unsupported fields.
- A compromised trusted machine can read the same credentials as the gateway.

Any proposal to expose the service beyond localhost requires a new threat review, downstream authentication, authorization, rate limiting, and secret-isolation design.

## P1 extracted SDK verification boundary (2026-11-20)

The optional offline P1 runner executes a bounded unchanged SDK source closure from one SHA-256-pinned official `2.1.329` macOS arm64 binary. Extraction validates Mach-O/Bun bounds, module identity and per-range hashes before Node loads it. Unknown binaries fail closed. Explicit loading shims provide imported error classes, abort classification, one stream-helper constant and throwing sentinels for unused SDK imports; transport is a Node fetch adapter. The SDK constructs the Messages request and parses/accumulates SSE. No claim is made about the main Claude Code parser or its retry/dialogue decisions.

The worker process tree runs under a verified macOS loopback-only network policy with redirected HOME, minimal environment, synthetic auth and OS denials for real credential/keychain paths and access. Positive loopback identity, negative numeric TEST-NET, protected-file and external-redirect probes are mandatory. Explicit local upstreams and redirect rejection limit accidental external calls. The test process owns and terminates its children; it verifies its ports close while an unrelated listener survives. Timeout cleanup kills only the created process group. This boundary is intended for trusted pinned source, not hostile arbitrary Node programs. [Evidence and limits](docs/claude-code-p1-verification-2026-11-20.md).
