# Model catalog verification — 2026-11-18

The Gateway catalog now advertises three additional models:

| Model ID | Release date | Source |
| --- | --- | --- |
| `gpt-6.2-astra` | 2026-11-17 | [OpenAI API changelog](https://developers.openai.com/api/docs/changelog#november-2026) |
| `gpt-6.2-sol` | 2026-11-17 | [OpenAI API changelog](https://developers.openai.com/api/docs/changelog#november-2026) |
| `claude-fable-5-2` | 2026-11-09 | [Claude Fable 5.2 model page](https://platform.claude.com/docs/en/models/fable-5-2/overview) |

OpenAI lists both GPT-6.2 models for Codex on supported plans and clients. The optional live suites now cover all three added IDs by default. Earlier catalog IDs remain selectable, and the default Claude aliases still resolve to Sonnet, Opus, and Haiku 5.5. [Codex model documentation](https://learn.chatgpt.com/docs/models)

For GPT-6.2 requests, use reasoning effort `low` or higher. For Fable 5.2, use adaptive thinking and tool choice `auto` or `none`. Clients that replay thinking blocks to Fable 5.2 must keep the earlier request prefix intact. [GPT-6.2 guide](https://developers.openai.com/api/docs/guides/latest-model) · [Fable 5.2 migration guide](https://platform.claude.com/docs/en/models/fable-5-2/migration-guide)

## Validation

- Format and Clippy checks passed. All 68 Rust tests and 8 Python tests passed. The release build succeeded.
- The dependency-age check covered 227 registry crates and found no violations.
- A temporary Gateway with synthetic credentials and loopback upstream URLs returned 32 unique IDs from `GET /v1/models`, including all three additions with the expected provider and release metadata.
- Account access and inference on the provider APIs remain unverified.
