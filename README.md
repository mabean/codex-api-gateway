# Codex OpenAI Proxy

> **Status:** prototype under hardening
>
> **Security posture:** local-only by default • no cloud relay • explicit trust boundary • security docs in repo
>
> **Current reality:** this fork is under active audit/hardening and should not yet be treated as a production-grade token-handling proxy.

A proxy server that allows OpenAI-compatible clients to use ChatGPT/Codex authentication instead of requiring separate OpenAI API keys.

## Overview

This proxy bridges the gap between:
- **Input**: Standard OpenAI Chat Completions API format (what CLINE expects)
- **Output**: ChatGPT Responses API format (what ChatGPT backend uses)

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
- Local auth file loading
- Message/content conversion baseline
- Direct Codex backend request path in active development

Target:
- Fully validated ChatGPT/Codex-backed upstream transport
- Verified streaming support
- Hardened auth handling
- Auditable local-only deployment defaults

## Quick Start

### 1. Build and Run (local-only)

```bash
git clone https://github.com/mabean/codex-openai-proxy.git
cd codex-openai-proxy
cargo build --release
./target/release/codex-openai-proxy --port 8080 --auth-path ~/.codex/auth.json
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
```

### 4. Client integration

Any downstream client should initially be configured against a **localhost** base URL only.
Do not expose the service publicly during the hardening phase.

## How It Works

### Request Flow

1. **CLINE** → Chat Completions format → **Proxy**
2. **Proxy** → Converts to Responses API → **ChatGPT Backend**
3. **ChatGPT Backend** → Responses API format → **Proxy**
4. **Proxy** → Converts to Chat Completions → **CLINE**

### Format Conversion

**Chat Completions Request:**
```json
{
  "model": "gpt-5",
  "messages": [
    {"role": "user", "content": "Hello!"}
  ]
}
```

**Responses API Request:**
```json
{
  "model": "gpt-5", 
  "instructions": "You are a helpful AI assistant.",
  "input": [
    {
      "type": "message",
      "role": "user", 
      "content": [{"type": "input_text", "text": "Hello!"}]
    }
  ],
  "tools": [],
  "tool_choice": "auto",
  "store": false,
  "stream": false
}
```

## Configuration

### Command Line Options

```bash
codex-openai-proxy [OPTIONS]

Options:
  -p, --port <PORT>          Port to listen on [default: 8080]
      --auth-path <PATH>     Path to Codex auth.json [default: ~/.codex/auth.json]
  -h, --help                 Print help
  -v, --version              Print version
```

### Authentication

The proxy reads authentication from a local auth file. Treat that file as highly sensitive.

Security guidance:
- do not copy the auth file around casually
- do not commit it
- do not expose the proxy publicly while it can use those credentials
- do not enable verbose logging without checking redaction behavior first

The proxy automatically reads authentication from your Codex `auth.json` file:

```json
{
  "access_token": "eyJ...",
  "account_id": "db1fc050-5df3-42c1-be65-9463d9d23f0b",
  "api_key": "sk-proj-..."
}
```

**Priority**: Uses `access_token` + `account_id` for ChatGPT Plus accounts, falls back to `api_key` for standard OpenAI accounts.

## API Endpoints

### Health Check
- **GET** `/health`
- Returns service status

### Chat Completions
- **POST** `/v1/chat/completions`
- OpenAI-compatible chat completions endpoint
- Supports: messages, model, temperature, max_tokens, stream, tools

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

## Troubleshooting

### Common Issues

**Connection Refused:**
```bash
# Check if proxy is running
curl http://localhost:8080/health
```

**Authentication Errors:**
```bash
# Verify auth.json exists and has valid tokens
cat ~/.codex/auth.json | jq .
```

**Backend Errors:**
```bash
# Check proxy logs for detailed error messages
RUST_LOG=debug cargo run
```

### Debug Mode

```bash
# Run with debug logging
RUST_LOG=debug cargo run -- --port 8080

# Test with verbose curl
curl -v -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-5", "messages": [{"role": "user", "content": "Test"}]}'
```

## Known limitations

- This fork is still under hardening and protocol verification.
- OAuth/Codex backend behavior is still being validated against the real upstream contract.
- Anthropic-compatible ingress is planned, but not implemented yet.
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