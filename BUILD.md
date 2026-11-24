# Build and verification

## Requirements

- Current stable Rust toolchain with `rustfmt` and `clippy`.
- Python 3 for the dependency-age check.

## Build

```bash
cargo build --release
```

`Cargo.lock` is committed. Use `--locked` when a build must fail instead of updating the lockfile:

```bash
cargo build --release --locked
```

## Local verification

Run the same checks enforced by CI:

```bash
cargo fmt --check
cargo test --all-targets
python3 -B -m unittest discover -s scripts -p 'test_*.py'
python3 scripts/check_crate_age.py 7
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

CI runs this baseline on Linux and macOS.

The live-runner evidence regressions require no credentials or quota. They reject wrong-file, stale, failed, missing, and prematurely issued Claude tool read-backs, and invalid Chat SSE terminal reasons.

The dependency-age check rejects crates published less than seven days ago when their metadata is available. It is best effort: failed crates.io lookups or missing publication dates produce warnings and skip those packages, so a successful exit does not prove that every dependency was checked.

For documentation-only changes, verify repository-relative file links and run `git diff --check`.

## Live verification

Unit tests do not prove account entitlements or upstream availability. Live suites require Bash, curl, Python 3, and credentials for the selected upstreams. The Claude Code acceptance suite also requires the `claude` executable in `PATH`.

Both scripts use [live_gateway_checks.py](scripts/live_gateway_checks.py), build the release binary, start an owned temporary localhost gateway, and continue after individual failures. Their nine default IDs include GPT-6.2 Astra, GPT-6.2 Sol, and Claude Fable 5.2. The mixed suite checks JSON and SSE text plus complete tool-use round trips on `/v1/messages`. For GPT models it also checks `/v1/responses` and `/v1/chat/completions`. It probes default aliases and environment overrides. The Claude Code suite checks an actual `Read` → `Edit` → `Read` sequence and a `Bash` round trip, including successful tool results bound to their call IDs and a successful terminal response.

```bash
scripts/live_mixed_model_smoke.sh
scripts/live_claude_code_acceptance.sh
# Select a subset or adjust time limits (seconds):
MODELS="gpt-6.2-sol claude-fable-5-2" REQUEST_TIMEOUT=90 CLAUDE_TIMEOUT=180 scripts/live_claude_code_acceptance.sh
```

Gateway wire diagnostics are disabled by default. Set `CODEX_PROXY_VERBOSE=1` only when necessary; this can capture prompts, responses, and tool data. Even with diagnostics disabled, the suites save request/response artifacts and Claude Code transcripts in a unique temporary directory. The printed JSON rows and `summary.json` contain per-scenario outcomes; any failure or blocked scenario makes the suite exit nonzero. Authentication failures block dependent upstream scenarios, and model availability failures block later scenarios for that model. Inspect and redact artifacts before sharing them. `OUT_DIR` can select an artifact directory, and `PORT` can select a dedicated free port (otherwise the suite chooses a free localhost port). The suite never removes an existing directory or terminates unrelated services.

Live suites consume account quota. Unit tests and `/health` do not establish upstream access. Preserve a dated, redacted report for meaningful acceptance runs. See the [2026-10-05 model verification report](docs/model-verification-2026-10-05.md) for earlier acceptance results and the [2026-11-18 catalog verification](docs/model-verification-2026-11-18.md) for the latest catalog refresh.

The Claude Code acceptance suite runs with `--dangerously-skip-permissions` inside temporary workspaces and a bounded timeout/turn count. This bypasses Claude Code permission prompts; run the suite only in a trusted test environment.

## Offline extracted Claude Code SDK contract (P1)

On macOS arm64, prebuild the gateway with `cargo build --release --locked --offline` and have Node available locally. Supply the official local Claude Code binary; the harness never invokes the CLI, downloads dependencies, or calls a model:

```bash
python3 -B scripts/claude_contract_p1.py --binary /opt/homebrew/bin/claude
```

Only the pinned `2.1.329` fingerprint is accepted. An unknown build, missing SDK module, extraction failure, unsupported OS, or failed mandatory probe exits nonzero with a reason. Python unit tests use synthetic Mach-O/Bun data and require neither the client binary nor network access. The separate crate-age check above may access crates.io metadata.

The runner prints a temporary evidence directory containing the binary/module/range manifest, unchanged extracted SDK source, sandbox policy, and structured result. Keep these local; do not commit extracted Anthropic code or raw logs. Results identify the SDK builder, SSE decoder, accumulator branch, gateway path, expected text/error and actual outcome. The main CLI parser, retries, tool/dialogue state and live upstream acceptance remain outside P1. See the [dated P1 verification](docs/claude-code-p1-verification-2026-11-20.md).

## Offline Claude Code baseline diagnostics (P2)

Use the same prebuilt gateway, local Node and pinned official binary as P1. No dependencies are downloaded and no live model is called:

```bash
python3 -B scripts/claude_contract_p2.py --binary /opt/homebrew/bin/claude --gateway target/release/codex-api-gateway
python3 -B -m unittest discover -s scripts -p 'test_claude_contract_p2.py'
```

The runner verifies full-graph marker hits and raw-byte source ranges before executing twenty ordered cases across tool references, nested effort, clear_at, retry metadata, first SSE fragment timing and nonzero terminal usage. It reuses the pinned SDK, OS sandbox, isolated profiles and owned loopback processes. The effective turn controls use the original main-client `SXn` function; retries use original `shouldRetry` and `retryRequest` methods with bounded wait and counting request adapters. These retry cases establish decisions and delay selection, not automatic SDK transport retry. Tool-reference cases serialize a synthetic follow-up; they do not execute native ToolSearch.

`known_gap` passes only when the recorded baseline defect is reproduced. `correct_behavior` requires its positive control to survive. `unavailable`, missing cases, malformed fields/IDs, unexpected behavior, failed isolation or cleanup, unknown fingerprints and infrastructure failures exit nonzero. The first-fragment test holds terminal events on a closed barrier; the forwarding positive control must emit a decoded text delta before release, and both routes must produce exact eventual terminal text. Usage compares the complete SDK terminal message against nonzero upstream counters, never HTTP status alone.

The printed temporary evidence directory contains manifests, source ranges, synthetic request observations and structured outcomes; keep extracted modules and raw traces local. The [dated inventory](docs/claude-code-contract-inventory-2026-11-20.md) links the structured anchors and maps all nine future P3–P5 acceptance criteria, including unresolved provider conditions. P2 diagnostic success establishes the baseline, not repaired compatibility or live acceptance. The full CI baseline above and P1 command remain required checks.

P2 is a historical known-gap suite: after the P3 forwarding repair, run it only with a preserved pre-repair binary. Its old-gap expectations intentionally fail against repaired behavior. Use the positive P3 suite below for the current direct Anthropic contract; do not weaken P2 assertions.

## Offline positive Codex translation contract (P4)

Use the same local pinned Claude Code 2.1.329 macOS arm64 binary, Node with `vm.SourceTextModule`, Python 3 and a prebuilt gateway. No dependencies are downloaded by this command:

```bash
python3 -B scripts/claude_contract_p4.py --binary /opt/homebrew/bin/claude --gateway target/release/codex-api-gateway
```

All 31 cases are mandatory. Original SDK serialization, retry orchestration and terminal accumulation cross real localhost HTTP. Original `SXn` shapes turn controls. Native `l4o.create(...).call(...)` selects deferred Read, its mapper creates references, `eL` establishes eligibility and `lwi` consumes reference history. Original schema/input validation and `En` permission wrappers surround an explicit synthetic Read executor. Denial must execute zero times; allowance executes once, returning exact correlated content and terminal text. Legacy Read runs separately with low effort and the original pinned `_Bn` builder's retain-all context setting on both turns. Every unsupported semantic control must produce one terminal structured 400 and zero upstream requests; original effort/scope downgrade predicates remain false.

The runner prints `local evidence: ...` and writes manifests, checked source spans, wire receipts and `result.json` there. Raw extracted code and traces stay temporary. It uses the P1 OS loopback sandbox, synthetic credentials, blocked credential canary and redirects, isolated profiles, bounded deadlines and owned cleanup. Check intentional failure separately:

```bash
python3 -B scripts/claude_contract_p4.py --binary /opt/homebrew/bin/claude --gateway target/release/codex-api-gateway --fail-after-start
```

This must exit 1, record an `intentional` error in `result.json`, and set `owned_process_cleanup`, `owned_ports_closed`, and `unrelated_listener_alive` to true. Unknown fingerprints, missing cases/functions and unavailable capabilities fail closed. Run P1/P3 plus the complete CI baseline as regressions. [P4 verification](docs/claude-code-p4-verification-2026-11-21.md) records adapters, original-versus-repaired observations and provider limits; neither local success nor `/health` proves live access.

`--fail-after-start` deliberately fails after starting owned services to verify failure cleanup; expect exit 1 and `owned_ports_closed`, `owned_process_cleanup`, and `unrelated_listener_alive` all true. Both normal and failure paths stop only this run's processes; no process-name matching or port-wide cleanup is used. Every isolated worker has a deadline and an owned process group for timeout cleanup.

## Offline positive Anthropic forwarding contract (P3)

Prebuild with `cargo build --release --locked --offline`, then use the same local Node and pinned official Claude Code binary as P1. The suite downloads nothing and makes no live/model calls:

```bash
python3 -B scripts/claude_contract_p3.py --binary /opt/homebrew/bin/claude --gateway target/release/codex-api-gateway
python3 -B -m unittest discover -s scripts -p 'test_claude_contract_p3.py'
```

All fifteen positive cases are mandatory. A failed expectation, missing case, unknown fingerprint or unavailable function exits nonzero. The runner collects case failures while requiring every case to pass. For a before/after repair comparison, pass an immutable pre-repair release binary using `--gateway`; its SHA-256 is recorded separately from the pinned client fingerprint. Preserve substantive failures as baseline evidence, never classify them as repaired success.

The original SDK `makeRequest`, `shouldRetry` and `retryRequest` execute actual repeated localhost HTTP. The time adapter records bounded delay without sleeping; transport preparation and logging are adapters. Context rejection uses original predicates, regex, removal closure and field-gate expression with a controlled beta registry and disabled unrelated feature hooks. It does not execute the full main request loop. Token counts execute original `zi.countTokens`; the Codex 404 fallback is documented from the original main-client source, without invoking its quota-consuming sample fallback. Every outcome records its executed layer, path, expectation, received request/response and result. The [P3 verification](docs/claude-code-p3-verification-2026-11-21.md) lists exact layers and remaining limits.

Failure cleanup can be checked with the following wrapper. The child must exit 1, print its evidence directory first, write `result.json`, emit an error on stderr and preserve the unrelated listener:

```bash
python3 -B - <<'PY'
import json, pathlib, subprocess
cmd = ['python3', '-B', 'scripts/claude_contract_p3.py', '--binary', '/opt/homebrew/bin/claude',
       '--gateway', 'target/release/codex-api-gateway', '--fail-after-start']
p = subprocess.run(cmd, text=True, capture_output=True, timeout=120)
print(p.stdout, end='')
print(p.stderr, end='')
assert p.returncode == 1 and p.stderr.strip()
assert p.stdout.splitlines()[0].startswith('local evidence: ')
directory = pathlib.Path(p.stdout.splitlines()[0].removeprefix('local evidence: '))
result = json.loads((directory / 'result.json').read_text())
assert 'intentional' in result.get('error', '').lower()
assert all(result.get(k) is True for k in ('owned_process_cleanup', 'owned_ports_closed', 'unrelated_listener_alive'))
PY
```

Normal and intentional failure runs use synthetic credentials, temporary profiles, protected-file/OS network probes, explicit loopback upstreams, redirect rejection and owned process cleanup. Extracted proprietary code and traces stay in the printed local temporary directory. P1 and the complete CI baseline remain required. Dependency-age metadata access is a separate best-effort check.

## Offline incremental Codex SSE contract (P5)

Prebuild the release gateway and use the same pinned local Claude Code 2.1.329 binary and Node installation as P1:

```bash
python3 -B scripts/claude_contract_p5.py --binary /opt/homebrew/bin/claude --gateway target/release/codex-api-gateway
python3 -B -m unittest discover -s scripts -p 'test_claude_contract_p5.py'
```

The 26 mandatory cases execute the original `zi.create`, `zt`/`cc` SSE decoder and `ff.finalMessage` accumulator. Original `ff.abort` cancellation reaches the real fetch signal; an upstream socket probe confirms actual connection closure. Text and tool cases retain a closed terminal barrier until the original consumer observes a fragment. The suite checks completed/unfinished/finalized/parallel tool lifecycles, authoritative terminal tool snapshots, late usage, max-token termination, malformed/contradictory/duplicate/truncated/error/deadline outcomes, actual upstream attempt counts, downstream cancellation, direct Anthropic extended fields, raw Responses wire bytes and OpenAI JSON/Chat SSE controls. Transport is an explicitly bounded adapter; SDK automatic retry and the full CLI loop are not executed by P5. Gateway retries are counted at the fixture.

The runner uses the existing macOS localhost-only OS policy, synthetic credentials, protected credential canary and external-redirect rejection. Unknown fingerprints, missing mandatory cases and unavailable checks fail nonzero. `--fail-after-start` deliberately exits 1 and must leave `owned_process_cleanup`, `owned_ports_closed` and `unrelated_listener_alive` true in `result.json`. Preserve the immutable original gateway and run the identical command against it for before/after evidence; do not change P2 historical expectations. Run P1/P3/P4 and the full CI baseline as regressions. See [dated P5 verification](docs/claude-code-p5-verification-2026-11-23.md) for the precise local scope and source-to-target limitations.

## Offline native Responses streaming contract (C1)

Prebuild the release gateway. On macOS, run the native Responses HTTP harness with synthetic credentials and P1's OS loopback sandbox:

```bash
python3 -B scripts/codex_responses_c1.py --gateway target/release/codex-api-gateway --expect c1
python3 -B scripts/codex_responses_c1.py --gateway target/release/codex-api-gateway --expect c1 --verify-failure-cleanup
python3 -B -m unittest discover -s scripts -p 'test_codex_responses_c1.py'
```

The 24 mandatory cases test decoded text and tool fragments before an upstream terminal barrier, split UTF-8/CRLF/multiline data, native bytes and opaque fields, cancellation, retry and exhaustion, explicit errors after commitment, terminal normalization/outcomes, terminal completion without EOF, valid EOF-delimited terminals with missing final newline/blank delimiter, invalid EOF tails, frame bounds and the configured deadline. To reproduce the prior buffering defect, run the same harness with `--gateway /absolute/path/to/immutable-baseline-gateway --expect baseline`: this mode executes the identical text/tool barriers and requires both to remain buffered until release. It also verifies that a truncated Content-Length before commitment produces HTTP 502 `upstream_protocol_error` after one attempt, as required on both baseline and repaired binaries. Baseline mode is a diagnostic, not C1 acceptance.

The printed temporary directory contains sanitized fixture outcomes, binary/harness hashes and cleanup receipts. `--verify-failure-cleanup` intentionally fails after startup and exits zero only after verifying that failure, owned process/port cleanup and an unrelated listener's survival. Normal missing checks or assertions fail nonzero. P1/P3/P4/P5 and the CI baseline remain separate regression checks. No real credentials, account inference or provider refresh are exercised. See [dated C1 verification](docs/codex-responses-c1-verification-2026-11-23.md).

## Offline native Responses JSON and HTTP errors (C2)

Prebuild the release gateway and preserve an immutable accepted-C1 executable for the original comparison. The same mandatory fixtures execute through both releases; the expectation flag changes only assertions:

```bash
python3 -B scripts/codex_responses_c2.py --gateway /absolute/path/to/immutable-c1-gateway --expect c1
python3 -B scripts/codex_responses_c2.py --gateway target/release/codex-api-gateway --expect c2
python3 -B scripts/codex_responses_c2.py --gateway target/release/codex-api-gateway --expect c2 --verify-failure-cleanup
python3 -B -m unittest discover -s scripts -p 'test_codex_responses_c*.py'
```

The current C2 suite has 66 mandatory cases: the original 56 plus ten C2-R1/R2 repair cases. The original cases cover native completed/incomplete/function-call JSON, nonzero usage details, ordered text and native text precedence, sparse compatibility, absent usage, authoritative terminal output, legacy and EOF terminals, completion before upstream EOF, malformed/missing/failed/error/contradictory terminals and frame bounds. Both stream modes check upstream 400/401/403/404/429, retry exhaustion, valid decimal/HTTP-date retry hints, invalid/duplicate/combined/negative/Connection-nominated hints, send/read timeouts, transport failure, truncated body classification, local validation, attempt counts and synthetic secret exclusion from responses, headers and ordinary logs. C1 mode retains the original 56 cases and their expectations; it precisely records the old projection, legacy-terminal failure and raw terminal-error disclosure and is diagnostic, not C2 acceptance.

The ten appended repair cases verify finalized native message/function items when terminal output is omitted, standalone text-done events, delta/done deduplication, output-index ordering with mixed finalized text, authoritative terminal output/text including empty arrays/strings, and missing completed/incomplete status. Run the identical bounded comparison against the frozen initial C2 and repaired releases:

```bash
python3 -B scripts/codex_responses_c2.py --gateway /absolute/path/to/initial-c2-gateway --expect initial-c2 --repair-only
python3 -B scripts/codex_responses_c2.py --gateway target/release/codex-api-gateway --expect c2 --repair-only
```

`initial-c2` requires the observed original losses and the unchanged authority control; it does not claim repaired behavior. Running the first command with `--expect c2` instead must fail on `r1_message`, demonstrating the regression before repair. Default repaired mode runs all 66 cases. `--repair-only` validates the complete inventory before selecting the ten repair cases, with scope recorded in its result.

The C2 runner explicitly reuses C1/P1's OS sandbox, synthetic credentials, minimal environment, external-egress and protected-file probes, owned-process/port cleanup and unrelated-listener sentinel. Its manifest fingerprints the gateway, runner, shared lifecycle and fixtures. Intentional post-start failure uses the same cleanup checks. Run unchanged C1 and P5 as streaming regressions, and retain the CI and P1/P3/P4 acceptance checks separately. No live upstream calls are made. See [dated C2 verification](docs/codex-responses-c2-verification-2026-11-23.md).
