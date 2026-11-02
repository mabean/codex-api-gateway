# Model catalog verification — 2026-11-02

Status: official catalogs checked; local tests, lint, release build, and HTTP catalog check passed. Account entitlements and live inference are not verified by catalog inclusion.

## Catalog changes and sources

- Add `gpt-6.1-terra` (GPT 6.1 Terra), released October 26, 2026. Sources: [OpenAI model catalog](https://developers.openai.com/api/docs/models), [Terra model page](https://developers.openai.com/api/docs/models/gpt-6.1-terra), and [October 26 changelog](https://developers.openai.com/api/docs/changelog#october-2026).
- Add `claude-haiku-5-5` (Claude Haiku 5.5), released October 7, 2026. Sources: [Claude model catalog](https://platform.claude.com/docs/en/models/overview), [Haiku model page](https://platform.claude.com/docs/en/models/haiku-5-5/overview), and [October 7 release notes](https://platform.claude.com/docs/en/release-notes/overview#october-7-2026).
- Update the default `haiku` alias to `claude-haiku-5-5`. Keep the explicit Haiku 4.5 ID and version alias available for existing callers. Environment alias overrides still apply.
- Keep existing catalog entries for compatibility. This static catalog contains known model IDs, including historical entries; it is not a list of currently entitled models. Restricted and specialized models are outside this general text/tool catalog refresh.
- Extend the optional live suites' default selections and alias expectations to cover both added models.

`created_at` uses the documented release day at midnight UTC; `created` is the matching Unix timestamp. These dates are not knowledge cutoffs or account-specific availability dates.

Haiku 5.5 enables adaptive thinking by default and rejects several Haiku 4.5 controls, including manual thinking budgets, sampling parameters, and assistant prefill. The gateway forwards request fields and upstream errors. See the release notes and the explicit 4.5 override in [README.md](../README.md#models-and-aliases).

## Validation

- `cargo fmt --check`: passed.
- `cargo test --all-targets --locked --offline`: 68 passed, including updated catalog metadata, explicit routing, default Haiku alias, legacy Haiku version alias, and alias override coverage.
- `python3 -B -m unittest discover -s scripts -p 'test_*.py'`: 8 passed.
- `cargo clippy --all-targets --all-features --locked --offline -- -D warnings`: passed.
- `cargo build --release --locked --offline`: passed.
- `python3 scripts/check_crate_age.py 7`: passed with public network access; 227 registry crates checked, no age-policy violations. The initial restricted-environment run checked zero crates because DNS access was unavailable.
- Repository-relative links in changed documentation: passed.
- Credential-free HTTP check of the newly built binary: `GET /v1/models` returned 29 unique model IDs, including both additions with the expected owner and release metadata. The owned temporary process used synthetic credentials and loopback-only dummy upstream destinations; no inference was requested.

Validation ran against a source copy before applying the same files to the checkout. No paid inference, service restart, binary installation, staging, or commit was performed. The running service requires a rebuild and restart to adopt this static catalog and alias default.
