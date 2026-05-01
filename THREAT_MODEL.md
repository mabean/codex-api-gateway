# THREAT MODEL

## Trust boundary
The proxy is designed to run on the same machine as the user and consume local auth material. The primary trust boundary is:

- trusted local process
- trusted local auth store
- trusted upstream Codex/ChatGPT backend

## Main threats
1. Secret leakage through logs
2. Secret leakage through accidental public exposure
3. Supply-chain compromise through dependencies
4. Hidden or unexpected outbound network traffic
5. Fake-success behavior that hides real upstream/auth failures
6. Protocol drift between documented API surfaces and actual behavior
7. Local planning artifacts accidentally committed as if they were product documentation

## Non-goals for current phase
- multi-tenant deployment
- public internet deployment
- centralized auth distribution
- production SaaS hosting model
- full Anthropic protocol parity

## Mitigations currently present
- localhost bind
- auth validation for legacy Codex auth and OpenClaw auth profile stores
- honest error responses
- removal of fake response paths
- transport/header minimization
- reproducible build path
- explicit OpenAI/Anthropic baseline documentation
- sensitive wire/body diagnostics are opt-in behind `CODEX_PROXY_VERBOSE=1`
