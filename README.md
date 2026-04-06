# Codex API Gateway

> **Status:** prototype under hardening
>
> **Security posture:** local-only by default • no cloud relay • explicit trust boundary • security docs in repo
>
> **Current reality:** this fork is under active audit/hardening and should not yet be treated as a production-grade token-handling proxy.

A local-first API gateway that exposes OpenAI-compatible and Anthropic-compatible clients over the ChatGPT/Codex upstream, suitable for ChatGPT Plus and higher plans that support the Codex path.

## Trust signals

- **Status:** prototype under hardening
- **Default exposure:** localhost-only
- **Cloud relay:** none intended
- **Telemetry:** none intended
- **Security docs:** `SECURITY.md`, `THREAT_MODEL.md`, `BUILD.md`
- **Verification baseline:** `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo build`
- **Ingress surfaces:** OpenAI-compatible `/v1/chat/completions`, Anthropic-compatible `/v1/messages`, both with streaming support
- **Secrets posture:** local auth material only; auth files must not be committed or publicly exposed

This is a trust-boundary and auditability signal, not a claim of production readiness.

## Overview

This proxy bridges the gap between:
- **Input**: OpenAI Chat Completions or Anthropic Messages API format
- **Core transport**: ChatGPT/Codex-backed upstream request path
- **Output**: OpenAI-compatible or Anthropic-compatible response envelopes

## Trust signals / security notes

- Local-first design is the target default
- No cloud relay is intended
- Public exposure is unsafe-by-default
- Secrets must never be logged
- This repo is being hardened for reproducible builds and explicit auditability
- Security docs in this repo:
- `SECURITY.md`
- `THREAT_MODEL.md`
- `BUILD.md`
- `HARDENING_PLAN.md`

## Current status

Subscription note:
- this gateway is intended for ChatGPT plans that can access the Codex/ChatGPT upstream path, such as ChatGPT Plus and above
- it does not convert a basic API key into Codex access if the upstream account itself does not have that path available

This fork is currently in **audit and hardening mode**.

Important:
- the codebase is still being aligned with the real upstream contract
- the primary request path now uses the real backend path instead of a fake success stub
- health/readiness reporting is more honest about config vs upstream verification
- security-sensitive deployment patterns must be treated as advanced/unsafe until explicitly documented
- maturity claims should be interpreted conservatively until the hardening checklist is complete

## Features (current / target split)

Current:
- OpenAI-compatible `/v1/chat/completions` ingress
- Anthropic-compatible `/v1/messages` ingress with streaming support
- Structured validation and error envelopes for both API families
- Local auth file loading
- Message/content conversion baseline
- Direct Codex backend request path in active development

Target:
- Fully validated ChatGPT/Codex-backed upstream transport
- Verified streaming support for both compatibility surfaces
- Hardened auth handling
- Auditable local-only deployment defaults

## Quick Start

### 1. Build and Run (local-only)

```bash
git clone https://github.com/mabean/codex-api-gateway.git
cd codex-api-gateway
cargo build --release
./target/release/codex-api-gateway --port 8080 --auth-path ~/.codex/auth.json
```

Optional:

```bash
./target/release/codex-api-gateway \
  --port 8080 \
  --auth-path ~/.codex/auth.json \
  --upstream-base-url https://chatgpt.com/backend-api
```

### 2. Local-only default

This proxy should be treated as a **localhost-only service**.

Unsafe-by-default until hardening is complete:
- ngrok exposure
- reverse tunnels
- public bind addresses
- shared LAN exposure

If remote exposure is ever needed, it must be documented as an advanced deployment mode with downstream authentication and a separate threat review.

### 3. Test locally

```bash
# Health check
curl http://127.0.0.1:8080/health

# Test completion (OpenAI-compatible ingress)
curl -X POST http://127.0.0.1:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer local-test-key" \
  -d '{
    "model": "gpt-5",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# Test completion (Anthropic-compatible ingress)
curl -X POST http://127.0.0.1:8080/v1/messages \
  -H "Content-Type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -d '{
    "model": "claude-sonnet-4-5",
    "max_tokens": 128,
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### 4. Client integration

Any downstream client should initially be configured against a **localhost** base URL only.
Do not expose the service publicly during the hardening phase.

### 5. Claude Code setup

Claude Code can be pointed at this gateway through Anthropic-compatible env vars.

Recommended global shell configuration:

```bash
export ANTHROPIC_BASE_URL="http://127.0.0.1:8080"
export ANTHROPIC_API_KEY="local-proxy"
export CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1
```

Why this works:
- Claude Code talks to the Anthropic-compatible `/v1/messages` surface
- this proxy translates that request into the working Codex/ChatGPT upstream path
- Anthropic-facing model names from Claude Code are normalized onto the supported Codex model path

Minimal verification:

```bash
claude -p --bare --model claude-sonnet-4-5 --output-format text \
  "Reply with exactly: CLAUDE_STREAM_OK"
```

Expected output:

```text
CLAUDE_STREAM_OK
```

Notes:
- keep the proxy running locally on port `8080`
- `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` reduces unrelated first-party side traffic during proxy usage
- the gateway is currently intended for local development use, not public exposure

## How It Works

### Request Flow

OpenAI-compatible path:
1. **Client** → Chat Completions format → **Proxy**
2. **Proxy** → Converts to internal Responses-style upstream request → **ChatGPT/Codex backend**
3. **Backend** → SSE/text result → **Proxy**
4. **Proxy** → Converts to Chat Completions response → **Client**

Anthropic-compatible path:
1. **Client** → Messages format → **Proxy**
2. **Proxy** → Converts to internal chat shape → upstream request
3. **Backend** → SSE/text result → **Proxy**
4. **Proxy** → Converts to Anthropic message response → **Client**

## Configuration

### Command Line Options

```bash
codex-api-gateway [OPTIONS]

Options:
  -p, --port <PORT>                    Port to listen on [default: 8080]
      --auth-path <PATH>               Path to Codex auth.json [default: ~/.codex/auth.json]
      --upstream-base-url <URL>        Upstream ChatGPT/Codex base URL [default: https://chatgpt.com/backend-api]
  -h, --help                           Print help
  -v, --version                        Print version
```

### Authentication

The proxy reads authentication from a local auth file. Treat that file as highly sensitive.

Security guidance:
- do not copy the auth file around casually
- do not commit it
- do not expose the proxy publicly while it can use those credentials
- do not enable verbose logging without checking redaction behavior first

The proxy supports:
- legacy auth files with token fields
- OpenClaw-style auth profile files with `lastGood.openai-codex`

## API Endpoints

### Health Check
- **GET** `/health`
- Returns service status

### OpenAI-compatible
- **POST** `/v1/chat/completions`
- **GET** `/v1/models`

### Anthropic-compatible
- **POST** `/v1/messages`
- Current support: non-streaming, text-oriented baseline

## Error behavior

The proxy now distinguishes:
- request validation errors
- auth/config errors
- upstream authorization failures
- upstream availability/protocol failures

OpenAI-compatible errors are returned in an OpenAI-style `error` envelope.
Anthropic-compatible errors are returned in an Anthropic-style `{"type":"error","error":...}` envelope.

## Security roadmap

This fork is being hardened toward:
- localhost-only defaults
- explicit allowed outbound hosts
- secret-safe logging
- reproducible builds
- pinned dependency graph
- security documentation and review checklist

See:
- `HARDENING_PLAN.md`
- `SECURITY.md`
- `THREAT_MODEL.md`
- `BUILD.md`

## Security-sensitive caveats

- Treat OAuth/auth files as secrets.
- Do not expose the proxy publicly during the hardening phase.
- Do not assume successful health checks mean the upstream transport is fully validated.
- `config_ready` / `auth_material_present` are not the same thing as proven upstream auth validity.
- Prefer local testing until the hardening plan is complete.

## Known limitations

- This fork is still under hardening and protocol verification.
- OAuth/Codex backend behavior is still being validated against the real upstream contract.
- Anthropic-compatible ingress is implemented with streaming, but broader Claude/Anthropic protocol parity is still intentionally limited.
- Usage/token accounting is currently placeholder-level on the compatibility surface.
- Public deployment guidance is intentionally omitted for now.

## Development

### Building

```bash
cargo build
cargo test
cargo clippy
cargo fmt
```

### Adding Features

The proxy is designed to be extensible:

- **New endpoints**: Add routes in `main.rs`
- **Format conversion**: Modify conversion functions
- **Authentication**: Extend `AuthData` structure
- **Streaming**: Add SSE support for real-time responses

## License

This project is part of the Codex ecosystem and follows the same licensing as the main Codex repository.
