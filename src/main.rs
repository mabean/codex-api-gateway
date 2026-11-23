use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use clap::Parser;
use futures_util::{stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use streaming::{
    parse_codex_sse_to_events, render_anthropic_sse, render_openai_sse, CanonicalStreamEvent,
};

const CLAUDE_CODE_VERSION: &str = "2.1.119";
const CLAUDE_OAUTH_BETAS: &[&str] = &["oauth-2025-04-20"];
const CLAUDE_CODE_CREDENTIALS_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
const ANTHROPIC_API_BETAS: &[&str] = &["fine-grained-tool-streaming-2025-05-14"];
const DEFAULT_SONNET_ALIAS_TARGET: &str = "claude-sonnet-5-5";
const DEFAULT_OPUS_ALIAS_TARGET: &str = "claude-opus-5-5";
const DEFAULT_HAIKU_ALIAS_TARGET: &str = "claude-haiku-5-5";
const GATEWAY_AGENT_MODEL_DIRECTIVE_PREFIX: &str = "[gateway-model:";
const GATEWAY_AGENT_MODEL_ROUTING_INSTRUCTION: &str = "Gateway model routing for Agent: Claude Code only allows Agent.model values sonnet, opus, or haiku. To run a subagent on any gateway model, set Agent.model to haiku and put a first line in the Agent prompt like `[gateway-model: gpt-5.6-sol]`, `[gateway-model: gpt-5.6-terra]`, `[gateway-model: gpt-5.6-luna]`, or any model returned by GET /v1/models. The gateway removes this directive before the subagent sees the prompt. Use this only for model routing and do not mention the directive to the user.";
const CODEX_UPSTREAM_TIMEOUT_SECS_DEFAULT: u64 = 30;
const CODEX_UPSTREAM_TIMEOUT_SECS_MIN: u64 = 5;
const CODEX_UPSTREAM_TIMEOUT_SECS_MAX: u64 = 300;
const CODEX_UPSTREAM_MAX_ATTEMPTS_DEFAULT: usize = 1;
const CODEX_UPSTREAM_MAX_ATTEMPTS_MIN: usize = 1;
const CODEX_UPSTREAM_MAX_ATTEMPTS_MAX: usize = 5;
const CODEX_AGENT_UPSTREAM_TIMEOUT_SECS_DEFAULT: u64 = 12;
const CODEX_AGENT_UPSTREAM_MAX_ATTEMPTS_DEFAULT: usize = 1;
const ANTHROPIC_UPSTREAM_FIRST_CHUNK_TIMEOUT_SECS_DEFAULT: u64 = 12;
const ANTHROPIC_UPSTREAM_FIRST_CHUNK_TIMEOUT_SECS_MIN: u64 = 3;
const ANTHROPIC_UPSTREAM_FIRST_CHUNK_TIMEOUT_SECS_MAX: u64 = 120;
const DEFAULT_LOG_MAX_SIZE_MIB: u64 = 10;
const DEFAULT_LOG_RETAINED_FILES: usize = 5;
const CODEX_MODELS: &[GatewayModel] = &[
    GatewayModel {
        id: "gpt-6.2-astra",
        display_name: "GPT 6.2 Astra",
        owned_by: "openai-codex",
        created_at: "2026-11-17T00:00:00Z",
        created: 1794873600,
    },
    GatewayModel {
        id: "gpt-6.2-sol",
        display_name: "GPT 6.2 Sol",
        owned_by: "openai-codex",
        created_at: "2026-11-17T00:00:00Z",
        created: 1794873600,
    },
    GatewayModel {
        id: "gpt-6.1-terra",
        display_name: "GPT 6.1 Terra",
        owned_by: "openai-codex",
        created_at: "2026-10-26T00:00:00Z",
        created: 1792972800,
    },
    GatewayModel {
        id: "gpt-6.1-sol",
        display_name: "GPT 6.1 Sol",
        owned_by: "openai-codex",
        created_at: "2026-09-29T00:00:00Z",
        created: 1790640000,
    },
    GatewayModel {
        id: "gpt-6-astra",
        display_name: "GPT 6 Astra",
        owned_by: "openai-codex",
        created_at: "2026-09-03T00:00:00Z",
        created: 1788393600,
    },
    GatewayModel {
        id: "gpt-6-luna",
        display_name: "GPT 6 Luna",
        owned_by: "openai-codex",
        created_at: "2026-09-22T00:00:00Z",
        created: 1790035200,
    },
    GatewayModel {
        id: "gpt-6-sol",
        display_name: "GPT 6 Sol",
        owned_by: "openai-codex",
        created_at: "2026-09-22T00:00:00Z",
        created: 1790035200,
    },
    GatewayModel {
        id: "gpt-5.6",
        display_name: "GPT 5.6 (Sol alias)",
        owned_by: "openai-codex",
        created_at: "2026-07-09T00:00:00Z",
        created: 1783555200,
    },
    GatewayModel {
        id: "gpt-5.6-sol",
        display_name: "GPT 5.6 Sol",
        owned_by: "openai-codex",
        created_at: "2026-07-09T00:00:00Z",
        created: 1783555200,
    },
    GatewayModel {
        id: "gpt-5.6-terra",
        display_name: "GPT 5.6 Terra",
        owned_by: "openai-codex",
        created_at: "2026-07-09T00:00:00Z",
        created: 1783555200,
    },
    GatewayModel {
        id: "gpt-5.6-luna",
        display_name: "GPT 5.6 Luna",
        owned_by: "openai-codex",
        created_at: "2026-07-09T00:00:00Z",
        created: 1783555200,
    },
    GatewayModel {
        id: "gpt-5.5",
        display_name: "GPT 5.5",
        owned_by: "openai-codex",
        created_at: "2026-05-01T00:00:00Z",
        created: 1777593600,
    },
    GatewayModel {
        id: "gpt-5.4",
        display_name: "GPT 5.4",
        owned_by: "openai-codex",
        created_at: "2026-01-01T00:00:00Z",
        created: 1767225600,
    },
    GatewayModel {
        id: "gpt-5.4-mini",
        display_name: "GPT 5.4 Mini",
        owned_by: "openai-codex",
        created_at: "2026-01-01T00:00:00Z",
        created: 1767225600,
    },
];
const ANTHROPIC_MODELS: &[GatewayModel] = &[
    GatewayModel {
        id: "claude-fable-5-2",
        display_name: "Claude Fable 5.2",
        owned_by: "anthropic-upstream",
        created_at: "2026-11-09T00:00:00Z",
        created: 1794182400,
    },
    GatewayModel {
        id: "claude-haiku-5-5",
        display_name: "Claude Haiku 5.5",
        owned_by: "anthropic-upstream",
        created_at: "2026-10-07T00:00:00Z",
        created: 1791331200,
    },
    GatewayModel {
        id: "claude-fable-5-1",
        display_name: "Claude Fable 5.1",
        owned_by: "anthropic-upstream",
        created_at: "2026-09-01T00:00:00Z",
        created: 1788220800,
    },
    GatewayModel {
        id: "claude-opus-5-5",
        display_name: "Claude Opus 5.5",
        owned_by: "anthropic-upstream",
        created_at: "2026-09-22T00:00:00Z",
        created: 1790035200,
    },
    GatewayModel {
        id: "claude-sonnet-5-5",
        display_name: "Claude Sonnet 5.5",
        owned_by: "anthropic-upstream",
        created_at: "2026-09-28T00:00:00Z",
        created: 1790553600,
    },
    GatewayModel {
        id: "claude-fable-5",
        display_name: "Claude Fable 5",
        owned_by: "anthropic-upstream",
        created_at: "2026-06-09T00:00:00Z",
        created: 1780963200,
    },
    GatewayModel {
        id: "claude-opus-5",
        display_name: "Claude Opus 5",
        owned_by: "anthropic-upstream",
        created_at: "2026-07-24T00:00:00Z",
        created: 1784851200,
    },
    GatewayModel {
        id: "claude-sonnet-5",
        display_name: "Claude Sonnet 5",
        owned_by: "anthropic-upstream",
        created_at: "2026-06-30T00:00:00Z",
        created: 1782777600,
    },
    GatewayModel {
        id: "claude-opus-4-8",
        display_name: "Claude Opus 4.8",
        owned_by: "anthropic-upstream",
        created_at: "2026-05-28T00:00:00Z",
        created: 1779926400,
    },
    GatewayModel {
        id: "claude-opus-4-7",
        display_name: "Claude Opus 4.7",
        owned_by: "anthropic-upstream",
        created_at: "2026-04-14T00:00:00Z",
        created: 1776124800,
    },
    GatewayModel {
        id: "claude-sonnet-4-6",
        display_name: "Claude Sonnet 4.6",
        owned_by: "anthropic-upstream",
        created_at: "2026-02-17T00:00:00Z",
        created: 1771286400,
    },
    GatewayModel {
        id: "claude-opus-4-6",
        display_name: "Claude Opus 4.6",
        owned_by: "anthropic-upstream",
        created_at: "2026-02-04T00:00:00Z",
        created: 1770163200,
    },
    GatewayModel {
        id: "claude-opus-4-5-20251101",
        display_name: "Claude Opus 4.5",
        owned_by: "anthropic-upstream",
        created_at: "2025-11-24T00:00:00Z",
        created: 1763942400,
    },
    GatewayModel {
        id: "claude-haiku-4-5-20251001",
        display_name: "Claude Haiku 4.5",
        owned_by: "anthropic-upstream",
        created_at: "2025-10-15T00:00:00Z",
        created: 1760486400,
    },
    GatewayModel {
        id: "claude-sonnet-4-5-20250929",
        display_name: "Claude Sonnet 4.5",
        owned_by: "anthropic-upstream",
        created_at: "2025-09-29T00:00:00Z",
        created: 1759104000,
    },
    GatewayModel {
        id: "claude-opus-4-1-20250805",
        display_name: "Claude Opus 4.1",
        owned_by: "anthropic-upstream",
        created_at: "2025-08-05T00:00:00Z",
        created: 1754352000,
    },
    GatewayModel {
        id: "claude-opus-4-20250514",
        display_name: "Claude Opus 4",
        owned_by: "anthropic-upstream",
        created_at: "2025-05-22T00:00:00Z",
        created: 1747872000,
    },
    GatewayModel {
        id: "claude-sonnet-4-20250514",
        display_name: "Claude Sonnet 4",
        owned_by: "anthropic-upstream",
        created_at: "2025-05-22T00:00:00Z",
        created: 1747872000,
    },
];

pub(crate) fn verbose_tracing_enabled() -> bool {
    matches!(
        std::env::var("CODEX_PROXY_VERBOSE").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

fn normalize_codex_instructions(raw: &str) -> String {
    let mut core = Vec::new();
    let mut in_system = false;
    let mut in_tools_or_reminders = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed == "# System" {
            in_system = true;
            in_tools_or_reminders = false;
            continue;
        }
        if trimmed.starts_with("# Tools")
            || trimmed.starts_with("# Tool")
            || trimmed.starts_with("# Reminders")
        {
            in_tools_or_reminders = true;
            continue;
        }
        if !in_system || in_tools_or_reminders {
            continue;
        }
        if trimmed.is_empty() {
            if !core.last().map(|s: &String| s.is_empty()).unwrap_or(false) {
                core.push(String::new());
            }
            continue;
        }
        if trimmed.starts_with("IMPORTANT: ")
            || trimmed.starts_with("- Tool results and user messages may include <system-reminder>")
            || trimmed.starts_with("- Tools are executed in a user-selected permission mode")
            || trimmed.starts_with("- All text you output outside of tool use")
            || trimmed.starts_with("- If you want to call multiple tools")
            || trimmed.starts_with("- If you do not know the answer")
        {
            continue;
        }
        core.push(trimmed.to_string());
    }
    let normalized = core.join(
        "
",
    );
    let normalized = normalized
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    let normalized = normalized.trim().to_string();
    if normalized.is_empty() {
        "You are a helpful AI assistant. Provide clear, accurate, and concise responses to user questions and requests.".to_string()
    } else {
        normalized
    }
}

fn env_u64_clamped(name: &str, default: u64, min: u64, max: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|value| value.clamp(min, max))
        .unwrap_or(default)
}

fn env_usize_clamped(name: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map(|value| value.clamp(min, max))
        .unwrap_or(default)
}

fn codex_upstream_timeout() -> Duration {
    Duration::from_secs(env_u64_clamped(
        "CODEX_GATEWAY_CODEX_TIMEOUT_SECS",
        CODEX_UPSTREAM_TIMEOUT_SECS_DEFAULT,
        CODEX_UPSTREAM_TIMEOUT_SECS_MIN,
        CODEX_UPSTREAM_TIMEOUT_SECS_MAX,
    ))
}

fn codex_upstream_max_attempts() -> usize {
    env_usize_clamped(
        "CODEX_GATEWAY_CODEX_MAX_ATTEMPTS",
        CODEX_UPSTREAM_MAX_ATTEMPTS_DEFAULT,
        CODEX_UPSTREAM_MAX_ATTEMPTS_MIN,
        CODEX_UPSTREAM_MAX_ATTEMPTS_MAX,
    )
}

#[derive(Clone, Copy)]
struct CodexUpstreamRetryConfig {
    timeout: Duration,
    max_attempts: usize,
    policy_name: &'static str,
}

fn codex_upstream_retry_config() -> CodexUpstreamRetryConfig {
    CodexUpstreamRetryConfig {
        timeout: codex_upstream_timeout(),
        max_attempts: codex_upstream_max_attempts(),
        policy_name: "default",
    }
}

fn codex_agent_upstream_timeout() -> Duration {
    Duration::from_secs(env_u64_clamped(
        "CODEX_GATEWAY_CODEX_AGENT_TIMEOUT_SECS",
        CODEX_AGENT_UPSTREAM_TIMEOUT_SECS_DEFAULT,
        CODEX_UPSTREAM_TIMEOUT_SECS_MIN,
        CODEX_UPSTREAM_TIMEOUT_SECS_MAX,
    ))
}

fn codex_agent_upstream_max_attempts() -> usize {
    env_usize_clamped(
        "CODEX_GATEWAY_CODEX_AGENT_MAX_ATTEMPTS",
        CODEX_AGENT_UPSTREAM_MAX_ATTEMPTS_DEFAULT,
        CODEX_UPSTREAM_MAX_ATTEMPTS_MIN,
        CODEX_UPSTREAM_MAX_ATTEMPTS_MAX,
    )
}

fn codex_agent_upstream_retry_config() -> CodexUpstreamRetryConfig {
    CodexUpstreamRetryConfig {
        timeout: codex_agent_upstream_timeout(),
        max_attempts: codex_agent_upstream_max_attempts(),
        policy_name: "claude-code-agent",
    }
}

fn codex_retry_config_for_anthropic_codex(
    requested_model: &str,
    directive_model: Option<&str>,
) -> CodexUpstreamRetryConfig {
    let came_from_claude_agent_alias = directive_model.is_some()
        && route_anthropic_model(requested_model) == AnthropicModelRoute::Anthropic;
    if came_from_claude_agent_alias {
        codex_agent_upstream_retry_config()
    } else {
        codex_upstream_retry_config()
    }
}

fn anthropic_upstream_first_chunk_timeout() -> Duration {
    Duration::from_secs(env_u64_clamped(
        "CODEX_GATEWAY_ANTHROPIC_FIRST_CHUNK_TIMEOUT_SECS",
        ANTHROPIC_UPSTREAM_FIRST_CHUNK_TIMEOUT_SECS_DEFAULT,
        ANTHROPIC_UPSTREAM_FIRST_CHUNK_TIMEOUT_SECS_MIN,
        ANTHROPIC_UPSTREAM_FIRST_CHUNK_TIMEOUT_SECS_MAX,
    ))
}
use uuid::Uuid;
use warp::{http::StatusCode, Filter, Reply};

mod streaming;
#[cfg(test)]
mod tests;

fn build_http_client() -> Result<Client> {
    Client::builder()
        .user_agent("pi (codex-api-gateway)")
        .no_proxy()
        .build()
        .context("Failed to create HTTP client")
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "8080")]
    port: u16,
    #[arg(long, default_value = "~/.codex/auth.json")]
    auth_path: String,
    #[arg(long, default_value = "https://chatgpt.com/backend-api")]
    upstream_base_url: String,
    #[arg(
        long,
        default_value = "https://api.anthropic.com",
        help = "Anthropic Messages API base URL"
    )]
    anthropic_upstream_base_url: String,
    #[arg(
        long,
        help = "Anthropic upstream API token; alternatively set ANTHROPIC_UPSTREAM_API_KEY"
    )]
    anthropic_auth_token: Option<String>,
    #[arg(
        long,
        help = "Log file path; defaults to the platform application log directory"
    )]
    log_path: Option<String>,
    #[arg(
        long,
        default_value_t = DEFAULT_LOG_MAX_SIZE_MIB,
        help = "Rotate the active log after it reaches this size in MiB"
    )]
    log_max_size_mib: u64,
    #[arg(
        long,
        default_value_t = DEFAULT_LOG_RETAINED_FILES,
        help = "Number of rotated log files to retain"
    )]
    log_retained_files: usize,
}

struct RotatingFile {
    path: PathBuf,
    file: File,
    bytes_written: u64,
    max_size_bytes: u64,
    retained_files: usize,
}

impl RotatingFile {
    fn open(path: PathBuf, max_size_bytes: u64, retained_files: usize) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create log directory: {}", parent.display()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to open log file: {}", path.display()))?;
        let bytes_written = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        Ok(Self {
            path,
            file,
            bytes_written,
            max_size_bytes: max_size_bytes.max(1),
            retained_files,
        })
    }

    fn write_line(&mut self, line: &[u8]) -> std::io::Result<()> {
        if self.bytes_written > 0
            && self.bytes_written.saturating_add(line.len() as u64) > self.max_size_bytes
        {
            self.rotate()?;
        }
        self.file.write_all(line)?;
        self.file.flush()?;
        self.bytes_written = self.bytes_written.saturating_add(line.len() as u64);
        Ok(())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        self.file.flush()?;
        if self.retained_files == 0 {
            self.file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&self.path)?;
            self.bytes_written = 0;
            return Ok(());
        }

        let oldest = rotated_log_path(&self.path, self.retained_files);
        remove_file_if_present(&oldest)?;
        for index in (1..self.retained_files).rev() {
            let source = rotated_log_path(&self.path, index);
            let destination = rotated_log_path(&self.path, index + 1);
            rename_if_present(&source, &destination)?;
        }
        rename_if_present(&self.path, &rotated_log_path(&self.path, 1))?;
        self.file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        self.bytes_written = 0;
        Ok(())
    }
}

struct RotatingLogger {
    writer: std::sync::Mutex<RotatingFile>,
}

impl log::Log for RotatingLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} {}\n",
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            record.level(),
            record.args()
        );
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.write_line(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.file.flush();
        }
    }
}

fn rotated_log_path(path: &Path, index: usize) -> PathBuf {
    PathBuf::from(format!("{}.{}", path.display(), index))
}

fn remove_file_if_present(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn rename_if_present(source: &Path, destination: &Path) -> std::io::Result<()> {
    match std::fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn default_log_path() -> PathBuf {
    if cfg!(target_os = "macos") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Logs")
                .join("codex-api-gateway")
                .join("gateway.log");
        }
    }
    if let Some(state_home) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(state_home)
            .join("codex-api-gateway")
            .join("gateway.log");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("codex-api-gateway")
            .join("gateway.log");
    }
    PathBuf::from("/tmp/codex-api-gateway.log")
}

fn resolve_log_path(configured: Option<&str>) -> PathBuf {
    configured
        .and_then(expand_home_path)
        .map(PathBuf::from)
        .unwrap_or_else(default_log_path)
}

fn init_rotating_logger(path: PathBuf, max_size_mib: u64, retained_files: usize) -> Result<()> {
    let max_size_bytes = max_size_mib.max(1).saturating_mul(1024 * 1024);
    let logger = RotatingLogger {
        writer: std::sync::Mutex::new(RotatingFile::open(path, max_size_bytes, retained_files)?),
    };
    log::set_boxed_logger(Box::new(logger)).context("Failed to initialize rotating logger")?;
    log::set_max_level(log::LevelFilter::Info);
    Ok(())
}

#[derive(Deserialize, Debug, Clone)]
struct ChatCompletionsRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: Option<bool>,
    tools: Option<Vec<Value>>,
    reasoning_effort: Option<String>,
    #[serde(skip)]
    anthropic_controls: Option<AnthropicCodexControls>,
    tool_choice: Option<Value>,
}

#[derive(Debug, Clone)]
struct AnthropicCodexControls {
    parallel_tool_calls: Option<bool>,
}

#[derive(Clone, Copy)]
enum CodexToolSource {
    OpenAi,
    Anthropic,
}

#[derive(Deserialize, Debug, Clone)]
struct OpenAiResponsesRequest {
    model: String,
    input: Option<Value>,
    instructions: Option<String>,
    tools: Option<Vec<Value>>,
    tool_choice: Option<Value>,
    parallel_tool_calls: Option<bool>,
    reasoning: Option<Value>,
    store: Option<bool>,
    stream: Option<bool>,
    text: Option<Value>,
    include: Option<Vec<String>>,
    prompt_cache_key: Option<String>,
    previous_response_id: Option<String>,
    metadata: Option<Value>,
    truncation: Option<Value>,
    temperature: Option<f64>,
    top_p: Option<f64>,
    user: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct AnthropicMessagesRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Value>>,
    #[serde(flatten)]
    extra: std::collections::BTreeMap<String, Value>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct AnthropicMessage {
    role: String,
    content: Value,
}

#[derive(Serialize, Debug)]
struct AnthropicMessagesResponse {
    id: String,
    #[serde(rename = "type")]
    response_type: String,
    role: String,
    content: Vec<Value>,
    model: String,
    stop_reason: Option<String>,
    stop_sequence: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Serialize, Debug)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Debug, Clone, Copy)]
struct GatewayModel {
    id: &'static str,
    display_name: &'static str,
    owned_by: &'static str,
    created_at: &'static str,
    created: i64,
}

#[derive(Serialize, Debug)]
struct AnthropicErrorEnvelope {
    #[serde(rename = "type")]
    envelope_type: String,
    error: AnthropicErrorBody,
}

#[derive(Serialize, Debug)]
struct AnthropicErrorBody {
    #[serde(rename = "type")]
    error_type: String,
    message: String,
}

#[derive(Deserialize, Debug, Clone)]
struct ChatMessage {
    role: String,
    content: Value,
    #[serde(default)]
    tool_calls: Option<Vec<Value>>,
    tool_call_id: Option<String>,
}

#[derive(Serialize, Debug)]
struct ChatCompletionsResponse {
    id: String,
    object: String,
    created: i64,
    model: String,
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[derive(Serialize, Debug)]
struct Choice {
    index: i32,
    message: ChatResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Serialize, Debug)]
struct ChatResponseMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<Value>>,
}

#[derive(Serialize, Debug)]
struct Usage {
    prompt_tokens: i32,
    completion_tokens: i32,
    total_tokens: i32,
}

#[derive(Serialize, Debug)]
struct ResponsesApiRequest {
    model: String,
    instructions: String,
    input: Vec<ResponsesInputItem>,
    tools: Vec<Value>,
    tool_choice: Value,
    parallel_tool_calls: bool,
    reasoning: Option<Value>,
    store: bool,
    stream: bool,
    text: Value,
    include: Vec<String>,
    prompt_cache_key: String,
}

#[allow(dead_code)]
#[derive(Serialize, Debug)]
#[serde(untagged)]
enum ResponsesInputItem {
    UserMessage {
        role: String,
        content: Vec<InputContentItem>,
    },
    AssistantMessage {
        #[serde(rename = "type")]
        item_type: String,
        role: String,
        content: Vec<AssistantContentItem>,
        status: String,
        id: String,
    },
    FunctionCall {
        #[serde(rename = "type")]
        item_type: String,
        id: String,
        call_id: String,
        name: String,
        arguments: String,
    },
    FunctionCallOutput {
        #[serde(rename = "type")]
        item_type: String,
        call_id: String,
        output: String,
    },
}

#[derive(Serialize, Debug)]
#[serde(tag = "type")]
enum InputContentItem {
    #[serde(rename = "input_text")]
    InputText { text: String },
}

#[allow(dead_code)]
#[derive(Serialize, Debug)]
#[serde(tag = "type")]
enum AssistantContentItem {
    #[serde(rename = "output_text")]
    OutputText {
        text: String,
        annotations: Vec<Value>,
    },
}

#[derive(Debug, Clone)]
enum NormalizedMessage {
    UserText(String),
    AssistantText(String),
    AssistantToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Clone)]
struct AuthData {
    api_key: Option<String>,
    access_token: Option<String>,
    account_id: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum ApiFamily {
    OpenAi,
    Anthropic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnthropicModelRoute {
    Codex,
    Anthropic,
}

#[derive(Debug, Clone)]
enum ProxyError {
    InvalidJson {
        details: Option<String>,
    },
    Validation {
        message: String,
        field: Option<String>,
    },
    Auth {
        message: String,
    },
    UpstreamUnauthorized {
        message: String,
    },
    UpstreamBadRequest {
        message: String,
    },
    UpstreamUnavailable {
        message: String,
    },
    UpstreamProtocol {
        message: String,
    },
}

impl ProxyError {
    fn invalid_json(err: serde_json::Error) -> Self {
        Self::InvalidJson {
            details: Some(err.to_string()),
        }
    }
    fn openai_status_code(&self) -> StatusCode {
        match self {
            Self::InvalidJson { .. } | Self::Validation { .. } | Self::Auth { .. } => {
                StatusCode::BAD_REQUEST
            }
            Self::UpstreamUnauthorized { .. }
            | Self::UpstreamBadRequest { .. }
            | Self::UpstreamUnavailable { .. }
            | Self::UpstreamProtocol { .. } => StatusCode::BAD_GATEWAY,
        }
    }
    fn anthropic_status_code(&self) -> StatusCode {
        self.openai_status_code()
    }
    fn openai_type(&self) -> &'static str {
        match self {
            Self::InvalidJson { .. } | Self::Validation { .. } => "invalid_request_error",
            Self::Auth { .. } => "auth_error",
            Self::UpstreamUnauthorized { .. } => "upstream_unauthorized",
            Self::UpstreamBadRequest { .. } => "upstream_bad_request",
            Self::UpstreamUnavailable { .. } => "upstream_unavailable",
            Self::UpstreamProtocol { .. } => "upstream_protocol_error",
        }
    }
    fn openai_code(&self) -> &'static str {
        match self {
            Self::InvalidJson { .. } => "invalid_json",
            Self::Validation { .. } => "validation_error",
            Self::Auth { .. } => "auth_error",
            Self::UpstreamUnauthorized { .. } => "upstream_unauthorized",
            Self::UpstreamBadRequest { .. } => "upstream_bad_request",
            Self::UpstreamUnavailable { .. } => "upstream_unavailable",
            Self::UpstreamProtocol { .. } => "upstream_protocol_error",
        }
    }
    fn anthropic_type(&self) -> &'static str {
        match self {
            Self::InvalidJson { .. } | Self::Validation { .. } | Self::Auth { .. } => {
                "invalid_request_error"
            }
            Self::UpstreamUnauthorized { .. } => "authentication_error",
            Self::UpstreamBadRequest { .. }
            | Self::UpstreamUnavailable { .. }
            | Self::UpstreamProtocol { .. } => "api_error",
        }
    }
    fn is_retryable_codex_upstream_error(&self) -> bool {
        match self {
            Self::UpstreamUnavailable { .. } => true,
            Self::UpstreamProtocol { message } => {
                let message = message.to_lowercase();
                message.contains("timed out")
                    || message.contains("deadline")
                    || message.contains("connection closed")
            }
            _ => false,
        }
    }
    fn message(&self) -> String {
        match self {
            Self::InvalidJson { details } => details
                .clone()
                .map(|d| format!("Invalid JSON: {}", d))
                .unwrap_or_else(|| "Invalid JSON".to_string()),
            Self::Validation { message, field } => field
                .as_ref()
                .map(|f| format!("{} (field: {})", message, f))
                .unwrap_or_else(|| message.clone()),
            Self::Auth { message }
            | Self::UpstreamUnauthorized { message }
            | Self::UpstreamBadRequest { message }
            | Self::UpstreamUnavailable { message }
            | Self::UpstreamProtocol { message } => message.clone(),
        }
    }
}

#[derive(Deserialize, Debug)]
struct LegacyAuthFile {
    #[serde(rename = "OPENAI_API_KEY")]
    api_key: Option<String>,
    tokens: Option<LegacyTokenData>,
}
#[derive(Deserialize, Debug)]
struct LegacyTokenData {
    access_token: String,
    account_id: String,
}
#[derive(Deserialize, Debug)]
struct OpenClawAuthProfiles {
    profiles: Option<std::collections::HashMap<String, OpenClawProfile>>,
    #[serde(rename = "lastGood")]
    last_good: Option<std::collections::HashMap<String, String>>,
}
#[derive(Deserialize, Debug)]
struct OpenClawProfile {
    #[serde(rename = "type")]
    profile_type: Option<String>,
    provider: Option<String>,
    access: Option<String>,
    #[serde(rename = "accountId")]
    account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeCliCredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<ClaudeCliOauth>,
}

#[derive(Debug, Deserialize)]
struct ClaudeCliOauth {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<f64>,
}

#[derive(Debug, Clone)]
enum AnthropicUpstreamAuth {
    ApiKey(String),
    OAuthToken(String),
    PassthroughAuthorization(reqwest::header::HeaderValue),
}

struct ProxyServer {
    client: Client,
    auth_data: AuthData,
    upstream_base_url: String,
    anthropic_upstream_base_url: String,
    anthropic_auth_token: Option<String>,
    claude_oauth_refresh_lock: Arc<tokio::sync::Mutex<()>>,
}

impl ProxyServer {
    async fn new(
        auth_path: &str,
        upstream_base_url: &str,
        anthropic_upstream_base_url: &str,
        anthropic_auth_token: Option<String>,
    ) -> Result<Self> {
        let auth_path = if auth_path.starts_with("~/") {
            let home = std::env::var("HOME").context("HOME environment variable not set")?;
            auth_path.replace("~", &home)
        } else {
            auth_path.to_string()
        };
        let auth_content = tokio::fs::read_to_string(&auth_path)
            .await
            .with_context(|| format!("Failed to read auth file: {}", auth_path))?;
        let auth_data = Self::parse_auth_data(&auth_content).with_context(|| {
            format!("Failed to parse supported auth file format: {}", auth_path)
        })?;
        if auth_data.api_key.is_none() && auth_data.access_token.is_none() {
            anyhow::bail!("auth file did not contain a usable API key or OAuth access token")
        }
        let client = build_http_client()?;
        Ok(Self {
            client,
            auth_data,
            upstream_base_url: upstream_base_url.to_string(),
            anthropic_upstream_base_url: anthropic_upstream_base_url.to_string(),
            anthropic_auth_token: anthropic_auth_token
                .or_else(|| std::env::var("ANTHROPIC_UPSTREAM_API_KEY").ok())
                .and_then(normalize_optional_secret),
            claude_oauth_refresh_lock: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    fn parse_auth_data(raw: &str) -> Result<AuthData> {
        if let Ok(legacy) = serde_json::from_str::<LegacyAuthFile>(raw) {
            let access_token = legacy.tokens.as_ref().map(|t| t.access_token.clone());
            let account_id = legacy.tokens.as_ref().map(|t| t.account_id.clone());
            if legacy.api_key.is_some() || access_token.is_some() {
                return Ok(AuthData {
                    api_key: legacy.api_key,
                    access_token,
                    account_id,
                });
            }
        }
        if let Ok(openclaw) = serde_json::from_str::<OpenClawAuthProfiles>(raw) {
            if let Some(profiles) = openclaw.profiles {
                let selected_profile = openclaw
                    .last_good
                    .as_ref()
                    .and_then(|last_good| last_good.get("openai-codex"))
                    .and_then(|profile_id| profiles.get(profile_id))
                    .or_else(|| {
                        profiles.values().find(|profile| {
                            profile.profile_type.as_deref() == Some("oauth")
                                && profile.provider.as_deref() == Some("openai-codex")
                        })
                    });

                if let Some(profile) = selected_profile {
                    if profile.profile_type.as_deref() == Some("oauth") {
                        let access_token = profile.access.clone();
                        let account_id = profile.account_id.clone().or_else(|| {
                            access_token
                                .as_ref()
                                .and_then(|t| extract_account_id_from_jwt(t))
                        });
                        if access_token.is_some() {
                            return Ok(AuthData {
                                api_key: None,
                                access_token,
                                account_id,
                            });
                        }
                    }
                }
            }
        }
        anyhow::bail!("unsupported auth file format")
    }

    async fn resolve_anthropic_upstream_auth(
        &self,
        inbound_headers: &warp::http::HeaderMap,
    ) -> Option<AnthropicUpstreamAuth> {
        if let Some(token) = &self.anthropic_auth_token {
            return Some(anthropic_auth_from_token(token.clone()));
        }
        if let Some(token) = std::env::var("ANTHROPIC_OAUTH_TOKEN")
            .ok()
            .and_then(normalize_optional_secret)
        {
            return Some(AnthropicUpstreamAuth::OAuthToken(token));
        }
        let _refresh_guard = self.claude_oauth_refresh_lock.lock().await;
        if let Some(token) = read_or_refresh_claude_cli_access_token().await {
            return Some(AnthropicUpstreamAuth::OAuthToken(token));
        }
        if let Some(authorization) = inbound_headers.get("authorization") {
            if let Ok(value) = authorization.to_str() {
                if let Some(token) = value.strip_prefix("Bearer ") {
                    if is_anthropic_oauth_token(token) {
                        return Some(AnthropicUpstreamAuth::OAuthToken(token.to_string()));
                    }
                }
            }
            return Some(AnthropicUpstreamAuth::PassthroughAuthorization(
                authorization.clone(),
            ));
        }
        inbound_headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !is_local_proxy_api_key(value))
            .map(|value| anthropic_auth_from_token(value.to_string()))
    }

    fn convert_anthropic_to_chat(
        &self,
        mut anthropic_req: AnthropicMessagesRequest,
    ) -> Result<ChatCompletionsRequest, ProxyError> {
        hoist_anthropic_system_messages(&mut anthropic_req);
        validate_anthropic_request(&anthropic_req)?;
        let mut messages = Vec::new();
        if let Some(system) = anthropic_req.system {
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: system,
                tool_calls: None,
                tool_call_id: None,
            });
        }
        for msg in anthropic_req.messages {
            messages.push(ChatMessage {
                role: msg.role,
                content: msg.content,
                tool_calls: None,
                tool_call_id: None,
            });
        }
        if verbose_tracing_enabled() {
            if let Some(tools) = &anthropic_req.tools {
                log::warn!(
                    "[anthropic-ingress] incoming tools count={} names={:?}",
                    tools.len(),
                    tools
                        .iter()
                        .filter_map(|tool| {
                            tool.get("name")
                                .and_then(Value::as_str)
                                .map(|s| s.to_string())
                                .or_else(|| {
                                    tool.get("function")
                                        .and_then(|f| f.get("name"))
                                        .and_then(Value::as_str)
                                        .map(|s| s.to_string())
                                })
                        })
                        .collect::<Vec<String>>()
                );
            } else {
                log::warn!("[anthropic-ingress] incoming tools count=0 names=[]");
            }
        }
        Ok(ChatCompletionsRequest {
            model: anthropic_req.model,
            messages,
            stream: anthropic_req.stream,
            tools: anthropic_req.tools,
            reasoning_effort: anthropic_req
                .extra
                .get("output_config")
                .and_then(|config| config.get("effort"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            anthropic_controls: Some(AnthropicCodexControls {
                parallel_tool_calls: anthropic_req
                    .extra
                    .get("tool_choice")
                    .and_then(|choice| choice.get("disable_parallel_tool_use"))
                    .and_then(Value::as_bool)
                    .map(|disabled| !disabled),
            }),
            tool_choice: anthropic_req.extra.get("tool_choice").and_then(|choice| {
                match choice.get("type").and_then(Value::as_str) {
                    Some("auto") => Some(json!("auto")),
                    Some("none") => Some(json!("none")),
                    Some("any") => Some(json!("required")),
                    Some("tool") => choice
                        .get("name")
                        .map(|name| json!({"type":"function", "name":name})),
                    _ => None,
                }
            }),
        })
    }

    fn convert_chat_to_responses(
        &self,
        chat_req: ChatCompletionsRequest,
    ) -> Result<ResponsesApiRequest, ProxyError> {
        validate_chat_request(&chat_req)?;
        let mut input = Vec::new();
        let mut system_parts = Vec::new();
        for msg in chat_req.messages {
            if msg.role == "system" {
                let content_text = flatten_message_content(&msg.content).map_err(|message| {
                    ProxyError::Validation {
                        message,
                        field: Some("messages[].content".to_string()),
                    }
                })?;
                system_parts.push(content_text);
                continue;
            }
            for normalized in normalize_chat_message(&msg)? {
                match normalized {
                    NormalizedMessage::UserText(text) => {
                        input.push(ResponsesInputItem::UserMessage {
                            role: "user".to_string(),
                            content: vec![InputContentItem::InputText { text }],
                        })
                    }
                    NormalizedMessage::AssistantText(_text) => {
                        // Codex-side normalization experiment: omit prior assistant prose
                        // from upstream input and keep user/tool history only.
                    }
                    NormalizedMessage::AssistantToolCall {
                        id,
                        name,
                        arguments,
                    } => input.push(ResponsesInputItem::FunctionCall {
                        item_type: "function_call".to_string(),
                        id: format!("fc_{}", Uuid::new_v4().simple()),
                        call_id: id,
                        name,
                        arguments,
                    }),
                    NormalizedMessage::ToolResult {
                        tool_use_id,
                        content,
                    } => input.push(ResponsesInputItem::FunctionCallOutput {
                        item_type: "function_call_output".to_string(),
                        call_id: tool_use_id,
                        output: content,
                    }),
                }
            }
        }
        if input.is_empty() {
            return Err(ProxyError::Validation {
                message: "at least one user message is required for the current hardened path"
                    .to_string(),
                field: Some("messages".to_string()),
            });
        }
        let raw_instructions = if system_parts.is_empty() {
            "You are a helpful AI assistant. Provide clear, accurate, and concise responses to user questions and requests.".to_string()
        } else {
            system_parts.join("\n\n")
        };
        let instructions = normalize_codex_instructions(&raw_instructions);
        if verbose_tracing_enabled() {
            log::warn!("[codex-instructions-raw-bytes] {}", raw_instructions.len());
            log::warn!(
                "[codex-instructions-normalized-bytes] {}",
                instructions.len()
            );
            log::warn!(
                "[codex-instructions-normalized-prefix] {}",
                instructions.chars().take(1200).collect::<String>()
            );
        }
        let tool_source = if chat_req.anthropic_controls.is_some() {
            CodexToolSource::Anthropic
        } else {
            CodexToolSource::OpenAi
        };
        let normalized_tools =
            normalize_tools_for_codex(chat_req.tools.unwrap_or_default(), tool_source);
        if verbose_tracing_enabled() {
            let tool_debug_summary: Vec<Value> = normalized_tools
                .iter()
                .map(|tool| {
                    let required = tool
                        .get("parameters")
                        .and_then(|p| p.get("required"))
                        .cloned()
                        .unwrap_or(Value::Null);
                    json!({
                        "name": tool.get("name").cloned().unwrap_or(Value::Null),
                        "strict": tool.get("strict").cloned().unwrap_or(Value::Null),
                        "required": required,
                        "schema_keys": tool
                            .get("parameters")
                            .and_then(Value::as_object)
                            .map(|o| o.keys().cloned().collect::<Vec<String>>())
                            .unwrap_or_default()
                    })
                })
                .collect();
            log::warn!(
                "[responses-request] model={} tools_count={} tool_names={:?} input_items={} instructions_len={}",
                normalize_codex_model_id(&chat_req.model),
                normalized_tools.len(),
                normalized_tools
                    .iter()
                    .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(|s| s.to_string()))
                    .collect::<Vec<String>>(),
                input.len(),
                instructions.len()
            );
            log::warn!(
                "[responses-request-tools] {}",
                serde_json::to_string(&tool_debug_summary).unwrap_or_else(|_| "[]".to_string())
            );
        }
        Ok(ResponsesApiRequest {
            model: normalize_codex_model_id(&chat_req.model),
            instructions,
            input,
            tools: normalized_tools,
            tool_choice: chat_req
                .tool_choice
                .map(|choice| {
                    if choice.get("type").and_then(Value::as_str) == Some("function") {
                        if let Some(name) = choice.get("function").and_then(|f| f.get("name")) {
                            return json!({"type":"function", "name":name});
                        }
                    }
                    choice
                })
                .unwrap_or_else(|| json!("auto")),
            parallel_tool_calls: chat_req
                .anthropic_controls
                .and_then(|controls| controls.parallel_tool_calls)
                .unwrap_or(true),
            reasoning: chat_req
                .reasoning_effort
                .map(|effort| json!({"effort": effort})),
            store: false,
            stream: true,
            text: json!({"verbosity":"low","format":{"type":"text"}}),
            include: vec!["reasoning.encrypted_content".to_string()],
            prompt_cache_key: Uuid::new_v4().to_string(),
        })
    }

    fn convert_openai_responses_request(
        &self,
        req: OpenAiResponsesRequest,
    ) -> Result<(String, Value), ProxyError> {
        if req.model.trim().is_empty() {
            return Err(ProxyError::Validation {
                message: "model is required".to_string(),
                field: Some("model".to_string()),
            });
        }
        let input = normalize_openai_responses_input(req.input)?;
        let model = normalize_codex_model_id(&req.model);
        let normalized_tools =
            normalize_tools_for_codex(req.tools.unwrap_or_default(), CodexToolSource::OpenAi);
        let mut payload = json!({
            "model": model,
            "instructions": req.instructions.unwrap_or_else(|| "You are a helpful AI assistant. Provide clear, accurate, and concise responses to user questions and requests.".to_string()),
            "input": input,
            "tools": normalized_tools,
            "tool_choice": req.tool_choice.unwrap_or_else(|| json!("auto")),
            "parallel_tool_calls": req.parallel_tool_calls.unwrap_or(true),
            "reasoning": req.reasoning,
            "store": req.store.unwrap_or(false),
            "stream": true,
            "text": req.text.unwrap_or_else(|| json!({"verbosity":"low","format":{"type":"text"}})),
            "include": req.include.unwrap_or_else(|| vec!["reasoning.encrypted_content".to_string()]),
            "prompt_cache_key": req.prompt_cache_key.unwrap_or_else(|| Uuid::new_v4().to_string()),
        });
        let obj = payload.as_object_mut().expect("responses payload object");
        if let Some(previous_response_id) = req.previous_response_id {
            obj.insert(
                "previous_response_id".to_string(),
                json!(previous_response_id),
            );
        }
        if let Some(metadata) = req.metadata {
            obj.insert("metadata".to_string(), metadata);
        }
        if let Some(truncation) = req.truncation {
            obj.insert("truncation".to_string(), truncation);
        }
        if let Some(temperature) = req.temperature {
            obj.insert("temperature".to_string(), json!(temperature));
        }
        if let Some(top_p) = req.top_p {
            obj.insert("top_p".to_string(), json!(top_p));
        }
        if let Some(user) = req.user {
            obj.insert("user".to_string(), json!(user));
        }
        Ok((model, payload))
    }

    async fn upstream_sse_text_for_payload(
        &self,
        model: String,
        payload: Value,
        retry_config: CodexUpstreamRetryConfig,
    ) -> Result<(String, String), ProxyError> {
        let timeout = retry_config.timeout;
        let max_attempts = retry_config.max_attempts;
        if verbose_tracing_enabled() {
            let upstream_request_json = serde_json::to_string(&payload)
                .unwrap_or_else(|_| "<serialize-failed>".to_string());
            let upstream_request_prefix: String =
                upstream_request_json.chars().take(1500).collect();
            log::warn!(
                "[upstream-request-body-bytes] {}",
                upstream_request_json.len()
            );
            log::warn!("[upstream-request-body-prefix] {}", upstream_request_prefix);
        }

        let mut last_error: Option<ProxyError> = None;
        for attempt in 1..=max_attempts {
            log::warn!(
                "[codex-upstream-attempt] model={} attempt={} max_attempts={} timeout_secs={} policy={}",
                model,
                attempt,
                max_attempts,
                timeout.as_secs(),
                retry_config.policy_name
            );
            match self
                .send_codex_upstream_once(&model, &payload, timeout, attempt)
                .await
            {
                Ok(body) => return Ok((model, body)),
                Err(err) if attempt < max_attempts && err.is_retryable_codex_upstream_error() => {
                    log::warn!(
                        "[codex-upstream-retry] model={} attempt={} reason={}",
                        model,
                        attempt,
                        err.message()
                    );
                    last_error = Some(err);
                }
                Err(err) => return Err(err),
            }
        }

        Err(
            last_error.unwrap_or_else(|| ProxyError::UpstreamUnavailable {
                message: "Codex upstream failed without a recorded error".to_string(),
            }),
        )
    }

    async fn open_codex_upstream(
        &self,
        model: &str,
        payload: &Value,
        timeout: Duration,
        attempt: usize,
    ) -> Result<reqwest::Response, ProxyError> {
        let mut request_builder = self
            .client
            .post(format!(
                "{}/codex/responses",
                self.upstream_base_url.trim_end_matches('/')
            ))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .header("OpenAI-Beta", "responses=experimental")
            .header("originator", "pi")
            .header("User-Agent", "pi (codex-api-gateway)")
            .timeout(timeout);
        if let Some(access_token) = &self.auth_data.access_token {
            request_builder =
                request_builder.header("Authorization", format!("Bearer {}", access_token));
            if let Some(account_id) = &self.auth_data.account_id {
                request_builder = request_builder.header("chatgpt-account-id", account_id);
            }
        } else if let Some(api_key) = &self.auth_data.api_key {
            request_builder =
                request_builder.header("Authorization", format!("Bearer {}", api_key));
        } else {
            return Err(ProxyError::Auth {
                message: "no usable auth material found".to_string(),
            });
        }
        let session_id = Uuid::new_v4();
        request_builder = request_builder.header("session_id", session_id.to_string());
        let response = request_builder.json(&payload).send().await.map_err(|e| {
            log::warn!(
                "[codex-upstream-send-error] model={} attempt={} error={}",
                model,
                attempt,
                e
            );
            if e.is_timeout() {
                log::warn!("[codex-upstream-timeout] stage=send attempt={}", attempt);
            }
            ProxyError::UpstreamUnavailable {
                message: format!("failed to send request to upstream: {}", e),
            }
        })?;
        Ok(response)
    }

    async fn send_codex_upstream_once(
        &self,
        model: &str,
        payload: &Value,
        timeout: Duration,
        attempt: usize,
    ) -> Result<String, ProxyError> {
        let started = Instant::now();
        let response = self
            .open_codex_upstream(model, payload, timeout, attempt)
            .await?;
        let header_elapsed_ms = started.elapsed().as_millis();
        let status = response.status();
        let headers = format!("{:?}", response.headers());
        let body = response
            .text()
            .await
            .map_err(|error| Self::codex_body_read_error(error, attempt))?;
        let total_elapsed_ms = started.elapsed().as_millis();
        log::warn!(
            "[upstream-response-status] {} attempt={} model={} header_elapsed_ms={} total_elapsed_ms={}",
            status, attempt, model, header_elapsed_ms, total_elapsed_ms
        );
        if verbose_tracing_enabled() {
            let body_prefix: String = body.chars().take(1200).collect();
            log::warn!("[upstream-response-headers] {}", headers);
            log::warn!("[upstream-response-body-bytes] {}", body.len());
            if !body_prefix.trim().is_empty() {
                log::warn!("[upstream-response-body-prefix] {}", body_prefix);
            }
            if !body.trim().is_empty() {
                log::warn!("[upstream-response-body] {}", body);
            }
        }
        if !status.is_success() {
            log::warn!("[request-outcome] upstream_non_success");
            let message = if body.trim().is_empty() || !verbose_tracing_enabled() {
                format!("upstream returned {}", status)
            } else {
                format!("upstream returned {} with body: {}", status, body)
            };
            return Err(classify_upstream_error(status, message));
        }
        log::warn!("[request-outcome] upstream_success");
        Ok(body)
    }

    async fn upstream_sse_text_with_retry(
        &self,
        chat_req: ChatCompletionsRequest,
        retry_config: CodexUpstreamRetryConfig,
    ) -> Result<(String, String), ProxyError> {
        let responses_req = self.convert_chat_to_responses(chat_req)?;
        let model = responses_req.model.clone();
        let payload =
            serde_json::to_value(responses_req).map_err(|e| ProxyError::UpstreamProtocol {
                message: format!("failed to serialize upstream request payload: {}", e),
            })?;
        self.upstream_sse_text_for_payload(model, payload, retry_config)
            .await
    }

    fn codex_body_read_error(error: reqwest::Error, attempt: usize) -> ProxyError {
        log::warn!("[upstream-read-error] {}", error);
        if error.is_timeout() {
            log::warn!("[codex-upstream-timeout] stage=read attempt={}", attempt);
            log::warn!("[request-outcome] upstream_read_failed");
            return ProxyError::UpstreamUnavailable {
                message: format!("timed out reading upstream response body: {}", error),
            };
        }
        log::warn!("[request-outcome] upstream_read_failed");
        ProxyError::UpstreamProtocol {
            message: format!("failed to read upstream response body: {}", error),
        }
    }

    async fn proxy_native_responses_stream(
        &self,
        model: String,
        payload: Value,
        retry: CodexUpstreamRetryConfig,
    ) -> Result<warp::hyper::Body, ProxyError> {
        // As with translated SSE, the downstream body owns the upstream source.
        // Retry only until the first complete frame commits the response.
        let mut selected = None;
        for attempt in 1..=retry.max_attempts {
            let result = async {
                let response = self
                    .open_codex_upstream(&model, &payload, retry.timeout, attempt)
                    .await?;
                if !response.status().is_success() {
                    return Err(classify_upstream_error(
                        response.status(),
                        format!("upstream returned {}", response.status()),
                    ));
                }
                let mut source = response.bytes_stream();
                let mut parser = streaming::NativeResponsesSseParser::default();
                loop {
                    parser.check_error()?;
                    let frame = match source.next().await {
                        Some(Ok(bytes)) => parser.feed(&bytes)?,
                        Some(Err(error)) => {
                            return Err(Self::codex_body_read_error(error, attempt))
                        }
                        None => parser.finish()?,
                    };
                    if !frame.is_empty() {
                        return Ok((source, parser, frame));
                    }
                }
            }
            .await;
            match result {
                Ok(value) => {
                    selected = Some(value);
                    break;
                }
                Err(error)
                    if attempt < retry.max_attempts
                        && error.is_retryable_codex_upstream_error() =>
                {
                    continue
                }
                Err(error) => return Err(error),
            }
        }
        let (source, parser, first) = selected.ok_or_else(|| ProxyError::UpstreamUnavailable {
            message: "no upstream attempt".into(),
        })?;
        let output = stream::unfold(
            (Some(source), parser, Some(first), false),
            |(mut source, mut parser, mut first, mut done)| async move {
                if let Some(frame) = first.take() {
                    if parser.terminal {
                        source = None;
                    }
                    return Some((
                        Ok::<_, std::io::Error>(frame),
                        (source, parser, first, done),
                    ));
                }
                if done || parser.terminal {
                    return None;
                }
                loop {
                    let result = if let Err(error) = parser.check_error() {
                        Err(error)
                    } else {
                        match source.as_mut().expect("active upstream").next().await {
                            Some(Ok(bytes)) => parser.feed(&bytes),
                            Some(Err(error)) => Err(ProxyError::UpstreamProtocol {
                                message: format!("upstream stream read failed: {error}"),
                            }),
                            None => parser.finish(),
                        }
                    };
                    let frame = match result {
                        Ok(frame) => frame,
                        Err(error) => {
                            done = true;
                            format!("event: error\ndata: {}\n\n", json!({"type":"error","error":{"type":"upstream_protocol_error","message":error.message()}})).into_bytes()
                        }
                    };
                    if done || parser.terminal {
                        source = None;
                    }
                    if !frame.is_empty() {
                        return Some((Ok(frame), (source, parser, first, done)));
                    }
                    if done || parser.terminal {
                        return None;
                    }
                }
            },
        );
        Ok(warp::hyper::Body::wrap_stream(output))
    }

    async fn proxy_anthropic_codex_stream(
        &self,
        chat_req: ChatCompletionsRequest,
        retry: CodexUpstreamRetryConfig,
    ) -> Result<warp::hyper::Body, ProxyError> {
        let req = self.convert_chat_to_responses(chat_req)?;
        let model = req.model.clone();
        let payload = serde_json::to_value(req).map_err(|e| ProxyError::UpstreamProtocol {
            message: e.to_string(),
        })?;
        // Own the upstream stream in the downstream body's pull state. Dropping
        // that body drops reqwest immediately; there is no detached producer.
        let mut selected = None;
        for attempt in 1..=retry.max_attempts {
            let result = async {
                let response = self
                    .open_codex_upstream(&model, &payload, retry.timeout, attempt)
                    .await?;
                if !response.status().is_success() {
                    return Err(classify_upstream_error(
                        response.status(),
                        format!("upstream returned {}", response.status()),
                    ));
                }
                let mut source = response.bytes_stream();
                let mut parser = streaming::CodexSseParser::default();
                let mut encoder = streaming::AnthropicSseEncoder::new(&model);
                let mut terminal = Vec::new();
                loop {
                    parser.check_error()?;
                    let events = match source.next().await {
                        Some(Ok(bytes)) => parser.feed(&bytes)?,
                        Some(Err(e)) => {
                            return Err(ProxyError::UpstreamUnavailable {
                                message: format!("failed to read upstream response body: {e}"),
                            })
                        }
                        None => {
                            let events = parser.finish()?;
                            terminal.extend(events);
                            break;
                        }
                    };
                    let mut visible = Vec::new();
                    for event in events {
                        if matches!(event, CanonicalStreamEvent::Completed { .. }) {
                            terminal.push(event);
                        } else {
                            visible.push(event);
                        }
                    }
                    let first = encoder.encode(&visible);
                    if !first.is_empty() {
                        return Ok((source, parser, encoder, terminal, first, false));
                    }
                }
                let first = encoder.encode(&terminal);
                Ok((source, parser, encoder, Vec::new(), first, true))
            }
            .await;
            match result {
                Ok(value) => {
                    selected = Some(value);
                    break;
                }
                Err(error)
                    if attempt < retry.max_attempts
                        && error.is_retryable_codex_upstream_error() =>
                {
                    continue
                }
                Err(error) => return Err(error),
            }
        }
        let (source, parser, encoder, terminal, first, done) =
            selected.ok_or_else(|| ProxyError::UpstreamUnavailable {
                message: "no upstream attempt".into(),
            })?;
        let state = (source, parser, encoder, terminal, Some(first), done);
        let output = stream::unfold(
            state,
            |(mut source, mut parser, mut encoder, mut terminal, mut first, mut done)| async move {
                if let Some(frame) = first.take() {
                    return Some((
                        Ok::<_, std::io::Error>(frame),
                        (source, parser, encoder, terminal, first, done),
                    ));
                }
                if done {
                    return None;
                }
                loop {
                    let result = if let Err(error) = parser.check_error() {
                        Err(error)
                    } else {
                        match source.next().await {
                            Some(Ok(bytes)) => parser.feed(&bytes),
                            Some(Err(error)) => Err(ProxyError::UpstreamProtocol {
                                message: format!("upstream stream read failed: {error}"),
                            }),
                            None => {
                                done = true;
                                parser.finish()
                            }
                        }
                    };
                    let frame = match result {
                        Ok(events) => {
                            let mut visible = Vec::new();
                            for event in events {
                                if matches!(event, CanonicalStreamEvent::Completed { .. }) {
                                    terminal.push(event);
                                } else {
                                    visible.push(event);
                                }
                            }
                            if done {
                                visible.append(&mut terminal);
                            }
                            encoder.encode(&visible)
                        }
                        Err(error) => {
                            done = true;
                            format!(
                                "event: error\ndata: {}\n\n",
                                json!({"type":"error","error":{"type":"api_error","message":error.message()}})
                            )
                        }
                    };
                    if !frame.is_empty() {
                        return Some((Ok(frame), (source, parser, encoder, terminal, first, done)));
                    }
                    if done {
                        return None;
                    }
                }
            },
        );
        Ok(warp::hyper::Body::wrap_stream(output))
    }

    async fn proxy_request_stream(
        &self,
        chat_req: ChatCompletionsRequest,
        api_family: ApiFamily,
    ) -> Result<(String, String), ProxyError> {
        self.proxy_request_stream_with_retry(chat_req, api_family, codex_upstream_retry_config())
            .await
    }

    async fn proxy_request_stream_with_retry(
        &self,
        chat_req: ChatCompletionsRequest,
        api_family: ApiFamily,
        retry_config: CodexUpstreamRetryConfig,
    ) -> Result<(String, String), ProxyError> {
        let (model, sse_text) = self
            .upstream_sse_text_with_retry(chat_req, retry_config)
            .await?;
        if verbose_tracing_enabled() {
            log::warn!(
                "[raw-codex-sse-begin]
{}
[raw-codex-sse-end]",
                sse_text
            );
        }
        let events = parse_codex_sse_to_events(&sse_text).inspect_err(|e| {
            log::warn!("[codex-parse-error] {}", e.message());
            log::warn!("[request-outcome] codex_parse_failed");
        })?;
        let mut text_deltas = 0usize;
        let mut tool_call_starts = 0usize;
        let mut tool_call_deltas = 0usize;
        let mut tool_call_dones = 0usize;
        let mut completed_finish_reason: Option<String> = None;
        for event in &events {
            match event {
                CanonicalStreamEvent::TextDelta { .. } => text_deltas += 1,
                CanonicalStreamEvent::ToolCallStart { .. } => tool_call_starts += 1,
                CanonicalStreamEvent::ToolCallDelta { .. } => tool_call_deltas += 1,
                CanonicalStreamEvent::ToolCallDone { .. } => tool_call_dones += 1,
                CanonicalStreamEvent::Completed { finish_reason } => {
                    completed_finish_reason = finish_reason.clone();
                }
                _ => {}
            }
        }
        log::warn!(
            "[codex-events-summary] {}",
            serde_json::json!({
                "text_deltas": text_deltas,
                "tool_call_starts": tool_call_starts,
                "tool_call_deltas": tool_call_deltas,
                "tool_call_dones": tool_call_dones,
                "completed_finish_reason": completed_finish_reason,
            })
        );
        if tool_call_starts > 0 {
            log::warn!("[tool-path-stage] codex_tool_path_detected");
        } else {
            log::warn!("[tool-path-stage] codex_no_tool_path");
        }
        log::warn!("[request-outcome] success");
        let rendered = match api_family {
            ApiFamily::OpenAi => render_openai_sse(&events, &model),
            ApiFamily::Anthropic => render_anthropic_sse(&events, &model),
        };
        if matches!(api_family, ApiFamily::Anthropic) && verbose_tracing_enabled() {
            log::warn!(
                "[raw-anthropic-sse-begin]\n{}\n[raw-anthropic-sse-end]",
                rendered
            );
        }
        Ok((model, rendered))
    }

    async fn proxy_request(
        &self,
        chat_req: ChatCompletionsRequest,
    ) -> Result<ChatCompletionsResponse, ProxyError> {
        self.proxy_request_with_retry(chat_req, codex_upstream_retry_config())
            .await
    }

    async fn proxy_request_with_retry(
        &self,
        chat_req: ChatCompletionsRequest,
        retry_config: CodexUpstreamRetryConfig,
    ) -> Result<ChatCompletionsResponse, ProxyError> {
        let (model, body) = self
            .upstream_sse_text_with_retry(chat_req, retry_config)
            .await?;
        let events = parse_codex_sse_to_events(&body)?;
        render_chat_json(&events, &model)
    }

    async fn proxy_openai_responses_request(
        &self,
        req: OpenAiResponsesRequest,
    ) -> Result<Value, ProxyError> {
        let (model, payload) = self.convert_openai_responses_request(req)?;
        let (_model, body) = self
            .upstream_sse_text_for_payload(model.clone(), payload, codex_upstream_retry_config())
            .await?;
        let events = parse_codex_sse_to_events(&body)?;
        Ok(render_openai_responses_json(&events, &model))
    }

    async fn proxy_anthropic_upstream_request(
        &self,
        body: bytes::Bytes,
        inbound_headers: &warp::http::HeaderMap,
        path: &str,
        query: &str,
    ) -> Result<UpstreamHttpResponse, ProxyError> {
        let body = normalize_anthropic_request_model(&body)?;
        // Only the supported beta query is forwarded; the upstream origin and
        // endpoint remain configuration-owned.
        let beta_query = query.split('&').any(|pair| pair == "beta=true");
        let mut request_builder = self
            .client
            .post(format!(
                "{}{}{}",
                self.anthropic_upstream_base_url.trim_end_matches('/'),
                path,
                if beta_query { "?beta=true" } else { "" }
            ))
            .header("Content-Type", "application/json");

        if let Some(accept) = inbound_headers.get("accept") {
            request_builder = request_builder.header("Accept", accept);
        } else {
            request_builder = request_builder.header("Accept", "application/json");
        }
        if let Some(version) = inbound_headers.get("anthropic-version") {
            request_builder = request_builder.header("anthropic-version", version);
        } else {
            request_builder = request_builder.header("anthropic-version", "2023-06-01");
        }
        request_builder =
            request_builder.header("anthropic-dangerous-direct-browser-access", "true");

        let auth = self
            .resolve_anthropic_upstream_auth(inbound_headers)
            .await
            .ok_or_else(|| ProxyError::Auth {
                message: "no Anthropic upstream auth found; set ANTHROPIC_OAUTH_TOKEN, ANTHROPIC_UPSTREAM_API_KEY, or sign in with Claude Code locally".to_string(),
            })?;

        let oauth = matches!(auth, AnthropicUpstreamAuth::OAuthToken(_));
        if let Some(beta_header) = anthropic_beta_header(oauth, inbound_headers) {
            request_builder = request_builder.header("anthropic-beta", beta_header);
        }

        match auth {
            AnthropicUpstreamAuth::OAuthToken(token) => {
                request_builder = request_builder
                    .header("authorization", format!("Bearer {token}"))
                    .header(
                        "User-Agent",
                        format!("claude-cli/{CLAUDE_CODE_VERSION} (external, cli)"),
                    )
                    .header("anthropic-client-platform", "claude_code_cli")
                    .header("x-app", "cli");
                let body = if path == "/v1/messages" {
                    apply_claude_code_oauth_system_prompt(&body)?
                } else {
                    body
                };
                return self
                    .send_anthropic_upstream_request(request_builder, body)
                    .await;
            }
            AnthropicUpstreamAuth::ApiKey(token) => {
                request_builder = request_builder
                    .header("x-api-key", token)
                    .header("User-Agent", "codex-api-gateway/anthropic-router");
            }
            AnthropicUpstreamAuth::PassthroughAuthorization(authorization) => {
                request_builder = request_builder
                    .header("authorization", authorization)
                    .header("User-Agent", "codex-api-gateway/anthropic-router");
            }
        }

        self.send_anthropic_upstream_request(request_builder, body)
            .await
    }

    async fn send_anthropic_upstream_request(
        &self,
        request_builder: reqwest::RequestBuilder,
        body: bytes::Bytes,
    ) -> Result<UpstreamHttpResponse, ProxyError> {
        let timeout = anthropic_upstream_first_chunk_timeout();
        let started = Instant::now();
        let response = tokio::time::timeout(timeout, request_builder.body(body).send())
            .await
            .map_err(|_| {
                log::warn!(
                    "[anthropic-upstream-timeout] stage=headers timeout_secs={}",
                    timeout.as_secs()
                );
                ProxyError::UpstreamUnavailable {
                    message: format!(
                        "timed out waiting for Anthropic upstream response headers after {}s",
                        timeout.as_secs()
                    ),
                }
            })?
            .map_err(|e| ProxyError::UpstreamUnavailable {
                message: format!("failed to send request to Anthropic upstream: {}", e),
            })?;
        let header_elapsed_ms = started.elapsed().as_millis();
        let status = response.status();
        let headers = anthropic_response_headers(response.headers());
        log::warn!(
            "[anthropic-upstream-response-status] {} header_elapsed_ms={}",
            status,
            header_elapsed_ms
        );
        let mut body_stream = response.bytes_stream();
        let first_chunk = tokio::time::timeout(timeout, body_stream.next())
            .await
            .map_err(|_| {
                log::warn!(
                    "[anthropic-upstream-timeout] stage=first_chunk timeout_secs={}",
                    timeout.as_secs()
                );
                ProxyError::UpstreamUnavailable {
                    message: format!(
                        "timed out waiting for Anthropic upstream first response chunk after {}s",
                        timeout.as_secs()
                    ),
                }
            })?
            .transpose()
            .map_err(|e| ProxyError::UpstreamProtocol {
                message: format!(
                    "failed to read Anthropic upstream first response chunk: {}",
                    e
                ),
            })?;
        let first_chunk_elapsed_ms = started.elapsed().as_millis();
        log::warn!(
            "[anthropic-upstream-first-chunk] status={} elapsed_ms={}",
            status,
            first_chunk_elapsed_ms
        );

        let body = if let Some(first_chunk) = first_chunk {
            let first_stream = stream::once(async move { Ok::<_, std::io::Error>(first_chunk) });
            let rest_stream = body_stream.map(|chunk| {
                chunk.map_err(|e| {
                    std::io::Error::other(format!("Anthropic upstream stream error: {e}"))
                })
            });
            warp::hyper::Body::wrap_stream(first_stream.chain(rest_stream))
        } else {
            warp::hyper::Body::empty()
        };
        Ok(UpstreamHttpResponse {
            status,
            headers,
            body,
        })
    }
}

struct UpstreamHttpResponse {
    status: StatusCode,
    headers: warp::http::HeaderMap,
    body: warp::hyper::Body,
}

fn anthropic_response_headers(upstream: &warp::http::HeaderMap) -> warp::http::HeaderMap {
    let nominated: Vec<String> = upstream
        .get_all("connection")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|name| name.trim().to_ascii_lowercase())
        .collect();
    let mut headers = warp::http::HeaderMap::new();
    for (name, value) in upstream {
        let key = name.as_str();
        let allowed = matches!(
            key,
            "content-type" | "retry-after" | "retry-after-ms" | "x-should-retry" | "request-id"
        ) || key.starts_with("anthropic-ratelimit-unified-");
        if allowed && !nominated.iter().any(|excluded| excluded == key) {
            headers.append(name.clone(), value.clone());
        }
    }
    headers
}

impl UpstreamHttpResponse {
    fn into_response(self) -> warp::reply::Response {
        let mut response = warp::reply::Response::new(self.body);
        *response.status_mut() = self.status;
        *response.headers_mut() = self.headers;
        response
    }
}

fn normalize_openai_responses_input(input: Option<Value>) -> Result<Value, ProxyError> {
    match input {
        Some(Value::String(text)) => Ok(json!([
            {"role":"user","content":[{"type":"input_text","text":text}]}
        ])),
        Some(Value::Array(items)) if items.is_empty() => Err(ProxyError::Validation {
            message: "input must not be empty".to_string(),
            field: Some("input".to_string()),
        }),
        Some(Value::Array(items)) => Ok(Value::Array(items)),
        Some(Value::Object(obj)) => Ok(Value::Array(vec![Value::Object(obj)])),
        Some(Value::Null) | None => Err(ProxyError::Validation {
            message: "input is required".to_string(),
            field: Some("input".to_string()),
        }),
        Some(other) => Ok(json!([
            {"role":"user","content":[{"type":"input_text","text":other.to_string()}]}
        ])),
    }
}

fn render_chat_json(
    events: &[CanonicalStreamEvent],
    model: &str,
) -> Result<ChatCompletionsResponse, ProxyError> {
    let response = render_openai_responses_json(events, model);
    let calls: Vec<Value> = response["output"].as_array().unwrap().iter()
        .filter(|item| item["type"] == "function_call")
        .map(|item| json!({"id":item["call_id"], "type":"function", "function":{"name":item["name"], "arguments":item["arguments"]}})).collect();
    let finish_reason = if calls.is_empty() {
        "stop"
    } else {
        "tool_calls"
    };
    if calls.is_empty()
        && response["output_text"]
            .as_str()
            .unwrap_or_default()
            .is_empty()
    {
        return Err(ProxyError::UpstreamProtocol {
            message: "upstream returned success but no parsable response content or tool calls"
                .to_string(),
        });
    }
    Ok(ChatCompletionsResponse {
        id: format!("chatcmpl-{}", Uuid::new_v4()),
        object: "chat.completion".to_string(),
        created: chrono::Utc::now().timestamp(),
        model: model.to_string(),
        choices: vec![Choice {
            index: 0,
            message: ChatResponseMessage {
                role: "assistant".to_string(),
                content: response["output_text"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                tool_calls: if calls.is_empty() { None } else { Some(calls) },
            },
            finish_reason: Some(finish_reason.to_string()),
        }],
        usage: Some(Usage {
            prompt_tokens: response["usage"]["input_tokens"].as_i64().unwrap_or(0) as i32,
            completion_tokens: response["usage"]["output_tokens"].as_i64().unwrap_or(0) as i32,
            total_tokens: response["usage"]["total_tokens"].as_i64().unwrap_or(0) as i32,
        }),
    })
}

fn render_openai_responses_json(events: &[CanonicalStreamEvent], model: &str) -> Value {
    let mut text = String::new();
    let mut output = Vec::new();
    let mut input_tokens = 0u32;
    let mut output_tokens = 0u32;
    for event in events {
        match event {
            CanonicalStreamEvent::TextDelta { text: delta } => text.push_str(delta),
            CanonicalStreamEvent::ToolCallDone {
                call_id,
                name,
                arguments,
            } => output.push(json!({
                "type": "function_call",
                "id": format!("fc_{}", Uuid::new_v4().simple()),
                "call_id": call_id,
                "name": name,
                "arguments": arguments,
                "status": "completed"
            })),
            CanonicalStreamEvent::Usage {
                input_tokens: in_tokens,
                output_tokens: out_tokens,
            } => {
                input_tokens = *in_tokens;
                output_tokens = *out_tokens;
            }
            _ => {}
        }
    }
    if !text.is_empty() {
        output.insert(
            0,
            json!({
                "type": "message",
                "id": format!("msg_{}", Uuid::new_v4().simple()),
                "status": "completed",
                "role": "assistant",
                "content": [{"type": "output_text", "text": text, "annotations": []}]
            }),
        );
    }
    json!({
        "id": format!("resp_{}", Uuid::new_v4().simple()),
        "object": "response",
        "created_at": chrono::Utc::now().timestamp(),
        "status": "completed",
        "model": model,
        "output": output,
        "output_text": text,
        "parallel_tool_calls": true,
        "store": false,
        "text": {"format": {"type": "text"}},
        "tool_choice": "auto",
        "tools": [],
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "output_tokens_details": {"reasoning_tokens": 0},
            "total_tokens": input_tokens + output_tokens
        }
    })
}

fn normalize_codex_model_id(model: &str) -> String {
    let short = model.rsplit('/').next().unwrap_or(model);
    if short.eq_ignore_ascii_case("gpt-5.6") {
        return "gpt-5.6-sol".to_string();
    }
    if let Some(target) = resolve_gateway_model_alias(short) {
        return target.rsplit('/').next().unwrap_or(&target).to_string();
    }
    short.to_string()
}

fn normalize_anthropic_model_id(model: &str) -> String {
    let short = model.rsplit('/').next().unwrap_or(model);
    if let Some(target) = resolve_gateway_model_alias(short) {
        let target_short = target.rsplit('/').next().unwrap_or(&target);
        if !target_short.eq_ignore_ascii_case(short) {
            return normalize_anthropic_model_shorthand(target_short);
        }
    }
    normalize_anthropic_model_shorthand(short)
}

fn normalize_anthropic_model_shorthand(model: &str) -> String {
    match model.to_lowercase().as_str() {
        "sonnet" => DEFAULT_SONNET_ALIAS_TARGET.to_string(),
        "opus" => DEFAULT_OPUS_ALIAS_TARGET.to_string(),
        "haiku" => DEFAULT_HAIKU_ALIAS_TARGET.to_string(),
        "claude-opus-4-5" => "claude-opus-4-5-20251101".to_string(),
        "claude-haiku-4-5" => "claude-haiku-4-5-20251001".to_string(),
        "claude-sonnet-4-5" => "claude-sonnet-4-5-20250929".to_string(),
        "claude-opus-4-1" => "claude-opus-4-1-20250805".to_string(),
        "claude-opus-4" => "claude-opus-4-20250514".to_string(),
        "claude-sonnet-4" => "claude-sonnet-4-20250514".to_string(),
        _ => model.to_string(),
    }
}

fn route_anthropic_model(model: &str) -> AnthropicModelRoute {
    let short = model.rsplit('/').next().unwrap_or(model);
    let normalized = short.to_lowercase();
    let route_model = resolve_gateway_model_alias(&normalized).unwrap_or(normalized);
    let route_short = route_model.rsplit('/').next().unwrap_or(&route_model);
    if route_short.to_lowercase().starts_with("claude-") {
        AnthropicModelRoute::Anthropic
    } else {
        AnthropicModelRoute::Codex
    }
}

fn valid_gateway_model_directive_target(model: &str) -> bool {
    let trimmed = model.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 128
        && trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'/'))
}

fn strip_gateway_model_directive_from_text(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim_start();
    let prefix_len = GATEWAY_AGENT_MODEL_DIRECTIVE_PREFIX.len();
    let prefix = trimmed.get(..prefix_len)?;
    if !prefix.eq_ignore_ascii_case(GATEWAY_AGENT_MODEL_DIRECTIVE_PREFIX) {
        return None;
    }
    let close = trimmed.find(']')?;
    let model = trimmed[prefix_len..close].trim();
    if !valid_gateway_model_directive_target(model) {
        return None;
    }
    let remainder = trimmed[close + 1..]
        .trim_start_matches(['\r', '\n'])
        .to_string();
    Some((model.to_string(), remainder))
}

fn strip_gateway_model_directive_from_content(content: &mut Value) -> Option<String> {
    match content {
        Value::String(text) => {
            let (model, stripped) = strip_gateway_model_directive_from_text(text)?;
            *text = stripped;
            Some(model)
        }
        Value::Array(blocks) => {
            for block in blocks {
                let Some(obj) = block.as_object_mut() else {
                    continue;
                };
                let Some(text_value) = obj.get_mut("text") else {
                    continue;
                };
                let Some(text) = text_value.as_str() else {
                    continue;
                };
                let Some((model, stripped)) = strip_gateway_model_directive_from_text(text) else {
                    continue;
                };
                *text_value = Value::String(stripped);
                return Some(model);
            }
            None
        }
        _ => None,
    }
}

fn apply_gateway_model_directive(req: &mut AnthropicMessagesRequest) -> Option<String> {
    for message in &mut req.messages {
        if let Some(model) = strip_gateway_model_directive_from_content(&mut message.content) {
            return Some(model);
        }
    }
    None
}

fn anthropic_tool_name(tool: &Value) -> Option<&str> {
    tool.get("name")
        .and_then(Value::as_str)
        .or_else(|| tool.get("function")?.get("name")?.as_str())
}

fn anthropic_tools_include_agent(tools: Option<&Vec<Value>>) -> bool {
    tools
        .into_iter()
        .flatten()
        .any(|tool| anthropic_tool_name(tool) == Some("Agent"))
}

fn append_gateway_agent_instruction(system: Option<Value>) -> Value {
    match system {
        None | Some(Value::Null) => {
            Value::String(GATEWAY_AGENT_MODEL_ROUTING_INSTRUCTION.to_string())
        }
        Some(Value::String(text)) => Value::String(format!(
            "{text}\n\n{GATEWAY_AGENT_MODEL_ROUTING_INSTRUCTION}"
        )),
        Some(Value::Array(mut blocks)) => {
            blocks.push(text_system_block(GATEWAY_AGENT_MODEL_ROUTING_INSTRUCTION));
            Value::Array(blocks)
        }
        Some(other) => Value::Array(vec![
            other,
            text_system_block(GATEWAY_AGENT_MODEL_ROUTING_INSTRUCTION),
        ]),
    }
}

fn inject_gateway_agent_instruction(req: &mut AnthropicMessagesRequest) {
    if !anthropic_tools_include_agent(req.tools.as_ref()) {
        return;
    }
    req.system = Some(append_gateway_agent_instruction(req.system.take()));
}

fn append_anthropic_system_value(system: Option<Value>, value: Value) -> Value {
    let mut blocks = Vec::new();
    for item in system.into_iter().chain(std::iter::once(value)) {
        match item {
            Value::Null => {}
            Value::String(text) => blocks.push(text_system_block(text)),
            Value::Array(items) => blocks.extend(items),
            other => blocks.push(other),
        }
    }
    Value::Array(blocks)
}

// Inspect controls before the narrow typed ingress can discard their scope.
// Discovery is request-local evidence, never permission to execute a tool.
fn prepare_codex_anthropic_body(body: &[u8]) -> Result<Vec<u8>, ProxyError> {
    let mut request: Value = serde_json::from_slice(body).map_err(ProxyError::invalid_json)?;
    let invalid = |field: &str, message: &str| ProxyError::Validation {
        field: Some(field.to_owned()),
        message: format!("Codex translation cannot preserve {message}"),
    };
    let read_effort = |config: &Value| -> Result<Option<String>, ProxyError> {
        let object = config
            .as_object()
            .ok_or_else(|| invalid("output_config", "non-object output controls"))?;
        if object.keys().any(|key| key != "effort") {
            return Err(invalid(
                "output_config",
                "format or unknown output controls",
            ));
        }
        match object.get("effort") {
            None => Ok(None),
            Some(Value::String(effort))
                if ["low", "medium", "high", "xhigh"].contains(&effort.as_str()) =>
            {
                Ok(Some(effort.clone()))
            }
            _ => Err(invalid(
                "output_config.effort",
                "this effort value; supported values are low, medium, high, xhigh",
            )),
        }
    };
    // The original client's _Bn builder emits this retain-all no-op for
    // ordinary thinking requests. Any added field/edit changes its semantics.
    if request.get("context_management").is_some_and(|context| {
        context
            != &json!({
                "edits":[{"type":"clear_thinking_20251015","keep":"all"}]
            })
    }) {
        return Err(invalid("context_management", "context editing controls"));
    }
    let mut effort = request
        .get("output_config")
        .map(&read_effort)
        .transpose()?
        .flatten();
    let declarations = request
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let supported = |name: &str| ["Read", "Write", "Bash", "Edit"].contains(&name);
    let discovery = declarations.iter().any(|tool| tool["name"] == "ToolSearch");
    let mut active = std::collections::HashSet::<String>::new();
    let mut searches = std::collections::HashSet::<String>::new();
    let messages = request
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("messages", "missing message history"))?;
    let mut saw_conversation = false;
    for message in messages.iter_mut() {
        if message["role"] == "system" {
            if message.get("clear_at").is_some() {
                return Err(invalid("messages[].clear_at", "temporary system scope"));
            }
            if let Some(config) = message.get("output_config") {
                if let Some(value) = read_effort(config)? {
                    effort = Some(value);
                }
            }
            let empty = message["content"].as_array().is_some_and(Vec::is_empty);
            if saw_conversation && !empty {
                return Err(invalid(
                    "messages[].content",
                    "system content inserted within conversation history",
                ));
            }
            if empty {
                message["content"] = json!("");
            }
            continue;
        }
        saw_conversation = true;
        if message.get("output_config").is_some() || message.get("clear_at").is_some() {
            return Err(invalid("messages[]", "controls on non-system messages"));
        }
        let assistant = message["role"] == "assistant";
        let Some(blocks) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        for block in blocks {
            if assistant && block["type"] == "tool_use" && block["name"] == "ToolSearch" {
                let id = block["id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| invalid("messages[].content", "discovery call without ID"))?;
                if !discovery || !searches.insert(id.to_owned()) {
                    return Err(invalid(
                        "messages[].content",
                        "undeclared or duplicate discovery call",
                    ));
                }
            }
            if block["type"] != "tool_result" {
                continue;
            }
            let correlated = !assistant
                && block["tool_use_id"]
                    .as_str()
                    .is_some_and(|id| searches.remove(id));
            let Some(parts) = block.get_mut("content").and_then(Value::as_array_mut) else {
                continue;
            };
            for part in parts {
                if part["type"] != "tool_reference" {
                    continue;
                }
                let name = part["tool_name"]
                    .as_str()
                    .ok_or_else(|| invalid("tool_reference", "missing tool name"))?;
                let declaration = declarations.iter().find(|tool| tool["name"] == name);
                if !correlated || !supported(name) || declaration.is_none() {
                    return Err(invalid(
                        "tool_reference",
                        "unrelated, undeclared or unsupported discovered tool",
                    ));
                }
                let declaration = declaration.expect("checked declaration");
                if !declaration
                    .get("input_schema")
                    .is_some_and(Value::is_object)
                {
                    return Err(invalid(
                        "tool_reference",
                        "discovered tool without declared schema",
                    ));
                }
                active.insert(name.to_owned());
                let definition = json!({"description":declaration.get("description"),
                    "name":name,"parameters":declaration["input_schema"]});
                *part = json!({"type":"text", "text":format!("<functions>\n<function>{definition}</function>\n</functions>")});
            }
        }
    }
    if let Some(effort) = effort {
        request["output_config"] = json!({"effort":effort});
    }
    let mut tools = Vec::new();
    for tool in declarations {
        let name = tool["name"].as_str().unwrap_or_default();
        if name == "ToolSearch"
            || (supported(name)
                && (tool.get("defer_loading") != Some(&json!(true)) || active.contains(name)))
        {
            tools.push(tool);
        }
    }
    if let Some(choice) = request.get("tool_choice") {
        let object = choice
            .as_object()
            .ok_or_else(|| invalid("tool_choice", "non-object tool selection"))?;
        if object
            .keys()
            .any(|key| !["type", "name", "disable_parallel_tool_use"].contains(&key.as_str()))
            || object
                .get("disable_parallel_tool_use")
                .is_some_and(|value| !value.is_boolean())
        {
            return Err(invalid(
                "tool_choice",
                "unknown or malformed tool selection controls",
            ));
        }
        match choice["type"].as_str() {
            Some("auto" | "none" | "any") if choice.get("name").is_none() => {}
            Some("tool")
                if choice["name"]
                    .as_str()
                    .is_some_and(|name| tools.iter().any(|tool| tool["name"] == name)) => {}
            _ => {
                return Err(invalid(
                    "tool_choice",
                    "malformed selection or unavailable tool name",
                ))
            }
        }
    }
    request["tools"] = json!(tools);
    serde_json::to_vec(&request).map_err(ProxyError::invalid_json)
}

fn hoist_anthropic_system_messages(req: &mut AnthropicMessagesRequest) {
    let mut system = req.system.take();
    let mut messages = Vec::with_capacity(req.messages.len());
    for message in req.messages.drain(..) {
        if message.role == "system" {
            system = Some(append_anthropic_system_value(system, message.content));
        } else {
            messages.push(message);
        }
    }
    req.system = system;
    req.messages = messages;
}

fn prepare_anthropic_ingress(
    req: &mut AnthropicMessagesRequest,
) -> (Option<String>, AnthropicModelRoute) {
    hoist_anthropic_system_messages(req);
    inject_gateway_agent_instruction(req);
    let directive_model = apply_gateway_model_directive(req);
    if let Some(model) = &directive_model {
        req.model = model.clone();
    }
    let route = route_anthropic_model(&req.model);
    (directive_model, route)
}

// The direct route must not round-trip through AnthropicMessage: that type is
// deliberately narrower than the upstream protocol used for scoped controls.
fn prepare_anthropic_passthrough(
    body: &[u8],
    messages_endpoint: bool,
) -> Result<(bytes::Bytes, AnthropicModelRoute), ProxyError> {
    let mut payload: Value = serde_json::from_slice(body).map_err(ProxyError::invalid_json)?;
    let object = payload
        .as_object_mut()
        .ok_or_else(|| ProxyError::Validation {
            message: "Anthropic request body must be a JSON object".to_string(),
            field: None,
        })?;
    let mut directive = None;
    if messages_endpoint {
        if let Some(messages) = object.get_mut("messages").and_then(Value::as_array_mut) {
            for message in messages {
                if message.get("role").and_then(Value::as_str) == Some("system") {
                    continue;
                }
                if let Some(content) = message.get_mut("content") {
                    directive = strip_gateway_model_directive_from_content(content);
                    if directive.is_some() {
                        break;
                    }
                }
            }
        }
    }
    if let Some(model) = directive {
        object.insert("model".to_string(), Value::String(model));
    }
    let model =
        object
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| ProxyError::Validation {
                message: "model must be a string".to_string(),
                field: Some("model".to_string()),
            })?;
    let route = route_anthropic_model(model);
    if messages_endpoint && route == AnthropicModelRoute::Anthropic {
        let has_agent = object
            .get("tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| anthropic_tools_include_agent(Some(tools)));
        if has_agent {
            let system = append_gateway_agent_instruction(object.remove("system"));
            object.insert("system".to_string(), system);
        }
    }
    serde_json::to_vec(&payload)
        .map(|body| (bytes::Bytes::from(body), route))
        .map_err(|error| ProxyError::UpstreamProtocol {
            message: format!("failed to serialize Anthropic request payload: {error}"),
        })
}

fn resolve_gateway_model_alias(model: &str) -> Option<String> {
    let short = model.rsplit('/').next().unwrap_or(model).to_lowercase();
    let (env_name, default_target) = match short.as_str() {
        "sonnet" => ("CODEX_GATEWAY_ALIAS_SONNET", DEFAULT_SONNET_ALIAS_TARGET),
        "opus" => ("CODEX_GATEWAY_ALIAS_OPUS", DEFAULT_OPUS_ALIAS_TARGET),
        "haiku" => ("CODEX_GATEWAY_ALIAS_HAIKU", DEFAULT_HAIKU_ALIAS_TARGET),
        _ => return None,
    };
    std::env::var(env_name)
        .ok()
        .and_then(normalize_optional_secret)
        .or_else(|| Some(default_target.to_string()))
}

fn is_local_proxy_api_key(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("local-proxy")
}

fn is_anthropic_oauth_token(value: &str) -> bool {
    value.contains("sk-ant-oat")
}

fn anthropic_auth_from_token(token: String) -> AnthropicUpstreamAuth {
    if is_anthropic_oauth_token(&token) {
        AnthropicUpstreamAuth::OAuthToken(token)
    } else {
        AnthropicUpstreamAuth::ApiKey(token)
    }
}

fn normalize_optional_secret(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_claude_cli_access_token(raw: &str) -> Option<String> {
    let credentials = serde_json::from_str::<ClaudeCliCredentialsFile>(raw).ok()?;
    let oauth = credentials.claude_ai_oauth?;
    let access_token = oauth.access_token?.trim().to_string();
    if access_token.is_empty() {
        return None;
    }
    let expires_at = oauth.expires_at?;
    if !expires_at.is_finite() || expires_at <= claude_oauth_expiry_floor_ms() {
        return None;
    }
    Some(access_token)
}

fn claude_oauth_expiry_floor_ms() -> f64 {
    const EXPIRY_SKEW_MS: f64 = 60_000.0;
    claude_oauth_now_ms() + EXPIRY_SKEW_MS
}

fn claude_oauth_now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as f64)
        .unwrap_or(f64::MAX)
}

fn expand_home_path(path: &str) -> Option<String> {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var("HOME").ok()?;
        Some(format!("{home}/{rest}"))
    } else {
        Some(path.to_string())
    }
}

fn claude_keychain_enabled() -> bool {
    !matches!(
        std::env::var("CODEX_GATEWAY_DISABLE_CLAUDE_KEYCHAIN").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

async fn read_or_refresh_claude_cli_access_token() -> Option<String> {
    let keychain_raw = read_claude_cli_keychain_credentials_raw().await;
    if let Some(raw) = keychain_raw.as_deref() {
        if let Some(token) = parse_claude_cli_access_token(raw) {
            return Some(token);
        }
    }

    let file_path = expand_home_path("~/.claude/.credentials.json");
    let file_raw = if let Some(path) = file_path.as_deref() {
        tokio::fs::read_to_string(path).await.ok()
    } else {
        None
    };
    if let Some(raw) = file_raw.as_deref() {
        if let Some(token) = parse_claude_cli_access_token(raw) {
            return Some(token);
        }
    }

    if keychain_raw.is_some() || file_raw.is_some() {
        refresh_claude_cli_credentials_via_cli().await;
    }

    if let Some(raw) = read_claude_cli_keychain_credentials_raw().await.as_deref() {
        if let Some(token) = parse_claude_cli_access_token(raw) {
            return Some(token);
        }
    }
    if let Some(path) = file_path.as_deref() {
        if let Ok(raw) = tokio::fs::read_to_string(path).await {
            return parse_claude_cli_access_token(&raw);
        }
    }
    None
}

async fn read_claude_cli_keychain_credentials_raw() -> Option<String> {
    if !cfg!(target_os = "macos") || !claude_keychain_enabled() {
        return None;
    }
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new("security")
            .args([
                "find-generic-password",
                "-s",
                CLAUDE_CODE_CREDENTIALS_KEYCHAIN_SERVICE,
                "-w",
            ])
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

async fn refresh_claude_cli_credentials_via_cli() -> bool {
    let claude_exe = std::env::var("CODEX_GATEWAY_CLAUDE_EXE")
        .ok()
        .and_then(normalize_optional_secret)
        .unwrap_or_else(|| "claude".to_string());
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(claude_exe)
            .args(["auth", "status", "--json"])
            .output(),
    )
    .await;
    let Ok(Ok(output)) = output else {
        log::warn!("[claude-oauth-refresh] claude auth status timed out or failed to start");
        return false;
    };
    if !output.status.success() {
        log::warn!(
            "[claude-oauth-refresh] claude auth status failed status={}",
            output.status
        );
        return false;
    }
    let Ok(status) = serde_json::from_slice::<Value>(&output.stdout) else {
        return false;
    };
    status
        .get("loggedIn")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn anthropic_beta_header(oauth: bool, inbound_headers: &warp::http::HeaderMap) -> Option<String> {
    let required = if oauth {
        CLAUDE_OAUTH_BETAS
    } else {
        ANTHROPIC_API_BETAS
    };
    let mut features: Vec<String> = required.iter().map(|feature| feature.to_string()).collect();
    for value in inbound_headers.get_all("anthropic-beta") {
        let Ok(raw) = value.to_str() else {
            continue;
        };
        for feature in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if !features.iter().any(|existing| existing == feature) {
                features.push(feature.to_string());
            }
        }
    }
    if features.is_empty() {
        None
    } else {
        Some(features.join(","))
    }
}

fn claude_code_oauth_billing_header_text() -> String {
    format!("x-anthropic-billing-header: cc_version={CLAUDE_CODE_VERSION}.unknown; cc_entrypoint=unknown; cch=00000;")
}

fn claude_code_system_text() -> &'static str {
    "You are Claude Code, Anthropic's official CLI for Claude."
}

fn text_system_block(text: impl Into<String>) -> Value {
    json!({
        "type": "text",
        "text": text.into(),
    })
}

fn apply_claude_code_oauth_system_prompt(body: &[u8]) -> Result<bytes::Bytes, ProxyError> {
    let mut payload = serde_json::from_slice::<Value>(body).map_err(ProxyError::invalid_json)?;
    let Some(obj) = payload.as_object_mut() else {
        return Err(ProxyError::Validation {
            message: "Anthropic request body must be a JSON object".to_string(),
            field: None,
        });
    };

    let existing_system = obj.remove("system");
    // Current clients already supply their own attribution. Keep that original
    // block, its cache marker and position; the legacy prefix is a fallback.
    let has_attribution = match &existing_system {
        Some(Value::String(text)) => text.starts_with("x-anthropic-billing-header:"),
        Some(Value::Array(blocks)) => blocks.iter().any(|block| {
            block
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| text.starts_with("x-anthropic-billing-header:"))
        }),
        _ => false,
    };
    if has_attribution {
        return Ok(bytes::Bytes::copy_from_slice(body));
    }
    let mut system_blocks = vec![
        text_system_block(claude_code_oauth_billing_header_text()),
        text_system_block(claude_code_system_text()),
    ];

    match existing_system {
        None | Some(Value::Null) => {}
        Some(Value::String(text)) => system_blocks.push(text_system_block(text)),
        Some(Value::Array(blocks)) => system_blocks.extend(blocks),
        Some(other) => system_blocks.push(other),
    }
    obj.insert("system".to_string(), Value::Array(system_blocks));

    serde_json::to_vec(&payload)
        .map(bytes::Bytes::from)
        .map_err(|e| ProxyError::UpstreamProtocol {
            message: format!("failed to serialize Anthropic OAuth request payload: {}", e),
        })
}

fn normalize_anthropic_request_model(body: &[u8]) -> Result<bytes::Bytes, ProxyError> {
    let mut payload = serde_json::from_slice::<Value>(body).map_err(ProxyError::invalid_json)?;
    let Some(obj) = payload.as_object_mut() else {
        return Err(ProxyError::Validation {
            message: "Anthropic request body must be a JSON object".to_string(),
            field: None,
        });
    };
    if let Some(model) = obj.get("model").and_then(Value::as_str) {
        let normalized = normalize_anthropic_model_id(model);
        if normalized != model {
            obj.insert("model".to_string(), Value::String(normalized));
        }
    }
    serde_json::to_vec(&payload)
        .map(bytes::Bytes::from)
        .map_err(|e| ProxyError::UpstreamProtocol {
            message: format!("failed to serialize Anthropic request payload: {}", e),
        })
}

fn gateway_model_json(model: &GatewayModel) -> Value {
    json!({
        "id": model.id,
        "object": "model",
        "type": "model",
        "display_name": model.display_name,
        "created": model.created,
        "created_at": model.created_at,
        "owned_by": model.owned_by,
    })
}

fn gateway_models_response() -> Value {
    let data = CODEX_MODELS
        .iter()
        .chain(ANTHROPIC_MODELS.iter())
        .map(gateway_model_json)
        .collect::<Vec<_>>();
    json!({
        "object": "list",
        "type": "list",
        "data": data,
    })
}

#[allow(dead_code)]
fn anthropic_text_blocks(content: &Value) -> Vec<String> {
    match content {
        Value::String(s) => vec![s.clone()],
        Value::Array(arr) => arr
            .iter()
            .filter_map(|item| {
                let obj = item.as_object()?;
                if obj.get("type").and_then(Value::as_str) == Some("text") {
                    obj.get("text")
                        .and_then(Value::as_str)
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[allow(dead_code)]
fn anthropic_tool_result_blocks(content: &Value) -> Vec<(String, String)> {
    match content {
        Value::Array(arr) => arr
            .iter()
            .filter_map(|item| {
                let obj = item.as_object()?;
                if obj.get("type").and_then(Value::as_str) != Some("tool_result") {
                    return None;
                }
                let id = obj.get("tool_use_id").and_then(Value::as_str)?.to_string();
                let output = match obj.get("content") {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Array(parts)) => parts
                        .iter()
                        .filter_map(|part| {
                            let pobj = part.as_object()?;
                            if pobj.get("type").and_then(Value::as_str) == Some("text") {
                                pobj.get("text")
                                    .and_then(Value::as_str)
                                    .map(|s| s.to_string())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<String>>()
                        .join("\n"),
                    Some(other) => other.to_string(),
                    None => String::new(),
                };
                Some((id, output))
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn extract_tool_call_id(content: &Value) -> Option<String> {
    match content {
        Value::Object(obj) => obj
            .get("tool_call_id")
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .or_else(|| obj.get("id").and_then(Value::as_str).map(|s| s.to_string())),
        _ => None,
    }
}

fn extract_tool_output(content: &Value) -> Option<String> {
    match content {
        Value::String(s) => Some(s.clone()),
        Value::Object(obj) => obj
            .get("content")
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .or_else(|| {
                obj.get("output")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
            }),
        _ => None,
    }
}

fn normalize_chat_message(msg: &ChatMessage) -> Result<Vec<NormalizedMessage>, ProxyError> {
    match msg.role.as_str() {
        "system" => Ok(vec![]),
        "user" => {
            if let Value::Array(arr) = &msg.content {
                let mut out = Vec::new();
                for item in arr {
                    let Some(obj) = item.as_object() else {
                        continue;
                    };
                    match obj.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(text) = obj.get("text").and_then(Value::as_str) {
                                out.push(NormalizedMessage::UserText(text.to_string()));
                            }
                        }
                        Some("tool_result") => {
                            if let Some(id) = obj.get("tool_use_id").and_then(Value::as_str) {
                                let content = match obj.get("content") {
                                    Some(Value::String(s)) => s.clone(),
                                    Some(Value::Array(parts)) => parts
                                        .iter()
                                        .filter_map(|part| {
                                            let pobj = part.as_object()?;
                                            if pobj.get("type").and_then(Value::as_str)
                                                == Some("text")
                                            {
                                                pobj.get("text")
                                                    .and_then(Value::as_str)
                                                    .map(|s| s.to_string())
                                            } else {
                                                None
                                            }
                                        })
                                        .collect::<Vec<String>>()
                                        .join(
                                            "
",
                                        ),
                                    Some(other) => other.to_string(),
                                    None => String::new(),
                                };
                                out.push(NormalizedMessage::ToolResult {
                                    tool_use_id: id.to_string(),
                                    content,
                                });
                            }
                        }
                        _ => {}
                    }
                }
                if out.is_empty() {
                    let text = flatten_message_content(&msg.content).map_err(|message| {
                        ProxyError::Validation {
                            message,
                            field: Some("messages[].content".to_string()),
                        }
                    })?;
                    out.push(NormalizedMessage::UserText(text));
                }
                Ok(out)
            } else {
                Ok(vec![NormalizedMessage::UserText(
                    flatten_message_content(&msg.content).map_err(|message| {
                        ProxyError::Validation {
                            message,
                            field: Some("messages[].content".to_string()),
                        }
                    })?,
                )])
            }
        }
        "assistant" => {
            if let Some(calls) = &msg.tool_calls {
                let mut out = Vec::new();
                for call in calls {
                    let id = call.get("id").and_then(Value::as_str);
                    let function = call.get("function");
                    let name = function.and_then(|f| f.get("name")).and_then(Value::as_str);
                    let arguments = function
                        .and_then(|f| f.get("arguments"))
                        .and_then(Value::as_str);
                    let (Some(id), Some(name), Some(arguments)) = (id, name, arguments) else {
                        return Err(ProxyError::Validation {
                            message: "tool_calls require id, function.name and function.arguments"
                                .to_string(),
                            field: Some("messages[].tool_calls".to_string()),
                        });
                    };
                    out.push(NormalizedMessage::AssistantToolCall {
                        id: id.to_string(),
                        name: name.to_string(),
                        arguments: arguments.to_string(),
                    });
                }
                if !out.is_empty() {
                    return Ok(out);
                }
            }
            if let Value::Array(arr) = &msg.content {
                let mut out = Vec::new();
                for item in arr {
                    let Some(obj) = item.as_object() else {
                        continue;
                    };
                    match obj.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(text) = obj.get("text").and_then(Value::as_str) {
                                out.push(NormalizedMessage::AssistantText(text.to_string()));
                            }
                        }
                        Some("tool_use") => {
                            let id = obj
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            let name = obj
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            let arguments = obj
                                .get("input")
                                .cloned()
                                .unwrap_or(Value::Object(Default::default()))
                                .to_string();
                            out.push(NormalizedMessage::AssistantToolCall {
                                id,
                                name,
                                arguments,
                            });
                        }
                        _ => {}
                    }
                }
                if out.is_empty() {
                    let text = flatten_message_content(&msg.content).map_err(|message| {
                        ProxyError::Validation {
                            message,
                            field: Some("messages[].content".to_string()),
                        }
                    })?;
                    out.push(NormalizedMessage::AssistantText(text));
                }
                Ok(out)
            } else {
                Ok(vec![NormalizedMessage::AssistantText(
                    flatten_message_content(&msg.content).map_err(|message| {
                        ProxyError::Validation {
                            message,
                            field: Some("messages[].content".to_string()),
                        }
                    })?,
                )])
            }
        }
        "tool" => Ok(vec![NormalizedMessage::ToolResult {
            tool_use_id: msg
                .tool_call_id
                .clone()
                .or_else(|| extract_tool_call_id(&msg.content))
                .ok_or_else(|| ProxyError::Validation {
                    message: "tool messages require tool_call_id".to_string(),
                    field: Some("messages[].tool_call_id".to_string()),
                })?,
            content: extract_tool_output(&msg.content)
                .unwrap_or_else(|| flatten_message_content(&msg.content).unwrap_or_default()),
        }]),
        _ => Ok(vec![NormalizedMessage::UserText(
            flatten_message_content(&msg.content).map_err(|message| ProxyError::Validation {
                message,
                field: Some("messages[].content".to_string()),
            })?,
        )]),
    }
}

fn normalize_tool_parameters_schema(schema: Value) -> Value {
    match schema {
        Value::Null => json!({"type":"object","properties":{}}),
        other => other,
    }
}

fn normalize_tools_for_codex(tools: Vec<Value>, source: CodexToolSource) -> Vec<Value> {
    let allowed_names = ["Edit", "Read", "Write", "Bash"];
    tools
        .into_iter()
        .filter_map(|tool| {
            let obj = tool.as_object()?;
            let normalized = if let Some(function) = obj.get("function") {
                let f = function.as_object()?;
                json!({
                    "type": "function",
                    "name": f.get("name").cloned().unwrap_or(Value::Null),
                    "description": f.get("description").cloned().unwrap_or(Value::Null),
                    "parameters": normalize_tool_parameters_schema(
                        f.get("parameters")
                            .cloned()
                            .unwrap_or_else(|| json!({"type":"object","properties":{}}))
                    ),
                    "strict": false
                })
            } else {
                json!({
                    "type": "function",
                    "name": obj.get("name").cloned().unwrap_or(Value::Null),
                    "description": obj.get("description").cloned().unwrap_or(Value::Null),
                    "parameters": normalize_tool_parameters_schema(
                        obj.get("input_schema")
                            .cloned()
                            .or_else(|| obj.get("parameters").cloned())
                            .unwrap_or_else(|| json!({"type":"object","properties":{}}))
                    ),
                    "strict": false
                })
            };
            let name = normalized.get("name").and_then(Value::as_str)?;
            if allowed_names.contains(&name)
                || (name == "ToolSearch" && matches!(source, CodexToolSource::Anthropic))
            {
                Some(normalized)
            } else {
                None
            }
        })
        .collect()
}

fn validate_chat_request(req: &ChatCompletionsRequest) -> Result<(), ProxyError> {
    if req.model.trim().is_empty() {
        return Err(ProxyError::Validation {
            message: "model is required".to_string(),
            field: Some("model".to_string()),
        });
    }
    if req.messages.is_empty() {
        return Err(ProxyError::Validation {
            message: "messages must not be empty".to_string(),
            field: Some("messages".to_string()),
        });
    }
    for (idx, msg) in req.messages.iter().enumerate() {
        if msg.role.trim().is_empty() {
            return Err(ProxyError::Validation {
                message: "message role is required".to_string(),
                field: Some(format!("messages[{}].role", idx)),
            });
        }
    }
    Ok(())
}

fn validate_anthropic_request(req: &AnthropicMessagesRequest) -> Result<(), ProxyError> {
    if req.model.trim().is_empty() {
        return Err(ProxyError::Validation {
            message: "model is required".to_string(),
            field: Some("model".to_string()),
        });
    }
    if req.messages.is_empty() {
        return Err(ProxyError::Validation {
            message: "messages must not be empty".to_string(),
            field: Some("messages".to_string()),
        });
    }
    if req.max_tokens.unwrap_or(0) == 0 {
        return Err(ProxyError::Validation {
            message: "max_tokens must be greater than 0".to_string(),
            field: Some("max_tokens".to_string()),
        });
    }
    for (idx, msg) in req.messages.iter().enumerate() {
        if msg.role != "user" && msg.role != "assistant" {
            return Err(ProxyError::Validation {
                message: format!(
                    "anthropic messages role must be 'user' or 'assistant'; received {:?}",
                    msg.role
                ),
                field: Some(format!("messages[{}].role", idx)),
            });
        }
    }
    Ok(())
}

fn flatten_message_content(content: &Value) -> std::result::Result<String, String> {
    match content {
        Value::String(s) => Ok(s.clone()),
        Value::Array(arr) => {
            let mut parts = Vec::new();
            for item in arr {
                match item {
                    Value::String(s) => parts.push(s.clone()),
                    Value::Object(obj) => {
                        if obj.get("type").and_then(Value::as_str) == Some("text") {
                            if let Some(text) = obj.get("text").and_then(Value::as_str) {
                                parts.push(text.to_string());
                                continue;
                            }
                        }
                        if let Some(text) = obj.get("text").and_then(Value::as_str) {
                            parts.push(text.to_string());
                        }
                    }
                    _ => {}
                }
            }
            if parts.is_empty() {
                Err("message content array must contain at least one text item".to_string())
            } else {
                Ok(parts.join(" "))
            }
        }
        Value::Null => Err("message content must not be null".to_string()),
        _ => Err("message content must be a string or array of text blocks".to_string()),
    }
}

fn classify_upstream_error(status: StatusCode, message: String) -> ProxyError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            ProxyError::UpstreamUnauthorized { message }
        }
        StatusCode::BAD_REQUEST => ProxyError::UpstreamBadRequest { message },
        StatusCode::TOO_MANY_REQUESTS
        | StatusCode::BAD_GATEWAY
        | StatusCode::SERVICE_UNAVAILABLE
        | StatusCode::GATEWAY_TIMEOUT => ProxyError::UpstreamUnavailable { message },
        _ if status.is_server_error() => ProxyError::UpstreamUnavailable { message },
        _ => ProxyError::UpstreamProtocol { message },
    }
}

#[cfg(test)]
fn extract_response_content(response_text: &str) -> Result<String, ProxyError> {
    let mut response_content = String::new();
    let mut final_item_content = String::new();
    let mut saw_delta = false;
    for line in response_text.lines() {
        if let Some(json_data) = line.strip_prefix("data: ") {
            if json_data == "[DONE]" {
                break;
            }
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(json_data) {
                if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
                    match event_type {
                        "response.output_text.delta" => {
                            if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                                saw_delta = true;
                                response_content.push_str(delta);
                            }
                        }
                        "response.output_item.done" => {
                            if let Some(item) = event.get("item") {
                                if let Some(content_arr) =
                                    item.get("content").and_then(|v| v.as_array())
                                {
                                    for content_item in content_arr {
                                        if let Some(text) =
                                            content_item.get("text").and_then(|v| v.as_str())
                                        {
                                            final_item_content.push_str(text);
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    if !saw_delta && !final_item_content.is_empty() {
        response_content = final_item_content;
    }
    if response_content.is_empty() {
        return Err(ProxyError::UpstreamProtocol {
            message: "upstream returned success but no parsable response content".to_string(),
        });
    }
    Ok(response_content)
}

fn convert_chat_to_anthropic(
    source_model: &str,
    chat: ChatCompletionsResponse,
) -> AnthropicMessagesResponse {
    let mut content = Vec::new();
    let mut stop_reason = "end_turn";
    if let Some(choice) = chat.choices.into_iter().next() {
        if !choice.message.content.is_empty() {
            content.push(json!({"type":"text", "text":choice.message.content}));
        }
        if let Some(calls) = choice.message.tool_calls {
            stop_reason = "tool_use";
            for call in calls {
                let arguments = call["function"]["arguments"].as_str().unwrap_or("{}");
                content.push(json!({"type":"tool_use", "id":call["id"], "name":call["function"]["name"], "input":serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| json!({}))}));
            }
        }
    }
    let usage = chat.usage.unwrap_or(Usage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
    });
    AnthropicMessagesResponse {
        id: format!("msg_{}", Uuid::new_v4().simple()),
        response_type: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model: source_model.to_string(),
        stop_reason: Some(stop_reason.to_string()),
        stop_sequence: None,
        usage: AnthropicUsage {
            input_tokens: usage.prompt_tokens as u32,
            output_tokens: usage.completion_tokens as u32,
        },
    }
}

fn openai_error_response(error: ProxyError) -> warp::reply::Response {
    let status = error.openai_status_code();
    let body = json!({"error":{"message": error.message(),"type": error.openai_type(),"code": error.openai_code()}});
    warp::reply::with_status(warp::reply::json(&body), status).into_response()
}
fn anthropic_error_response(error: ProxyError) -> warp::reply::Response {
    let status = error.anthropic_status_code();
    let body = AnthropicErrorEnvelope {
        envelope_type: "error".to_string(),
        error: AnthropicErrorBody {
            error_type: error.anthropic_type().to_string(),
            message: error.message(),
        },
    };
    warp::reply::with_status(warp::reply::json(&body), status).into_response()
}
fn method_not_allowed_response(api_family: ApiFamily) -> warp::reply::Response {
    match api_family {
        ApiFamily::OpenAi => openai_error_response(ProxyError::Validation {
            message: "method not allowed for this endpoint".to_string(),
            field: None,
        }),
        ApiFamily::Anthropic => anthropic_error_response(ProxyError::Validation {
            message: "method not allowed for this endpoint".to_string(),
            field: None,
        }),
    }
}
fn not_found_response(api_family: ApiFamily) -> warp::reply::Response {
    match api_family {
        ApiFamily::OpenAi => warp::reply::with_status(warp::reply::json(&json!({"error":{"message":"Not found","type":"invalid_request_error","code":"not_found"}})), StatusCode::NOT_FOUND).into_response(),
        ApiFamily::Anthropic => warp::reply::with_status(warp::reply::json(&AnthropicErrorEnvelope { envelope_type: "error".to_string(), error: AnthropicErrorBody { error_type: "invalid_request_error".to_string(), message: "Not found".to_string() } }), StatusCode::NOT_FOUND).into_response(),
    }
}

fn extract_account_id_from_jwt(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let padded = match payload.len() % 4 {
        2 => format!("{}==", payload),
        3 => format!("{}=", payload),
        _ => payload.to_string(),
    };
    let decoded = URL_SAFE_NO_PAD.decode(padded.as_bytes()).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    value
        .get("https://api.openai.com/auth")
        .and_then(|v| v.get("chatgpt_account_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn log_request(method: &warp::http::Method, path: &str, headers: &warp::http::HeaderMap) {
    let auth_present = headers.get("authorization").is_some();
    let ua = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");
    log::info!(
        "[request] method={} path={} auth_present={} user_agent={}",
        method,
        path,
        auth_present,
        ua
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let log_path = resolve_log_path(args.log_path.as_deref());
    init_rotating_logger(
        log_path.clone(),
        args.log_max_size_mib,
        args.log_retained_files,
    )?;
    let proxy = ProxyServer::new(
        &args.auth_path,
        &args.upstream_base_url,
        &args.anthropic_upstream_base_url,
        args.anthropic_auth_token,
    )
    .await?;
    log::info!("Initializing Codex API Gateway...");
    log::info!(
        "✓ Log rotation: {} MiB, {} retained files, path={}",
        args.log_max_size_mib,
        args.log_retained_files,
        log_path.display()
    );
    log::info!("✓ Loaded authentication from {}", args.auth_path);
    log::info!(
        "✓ Auth mode: {}",
        if proxy.auth_data.access_token.is_some() {
            "oauth/codex"
        } else if proxy.auth_data.api_key.is_some() {
            "api-key"
        } else {
            "unknown"
        }
    );
    log::info!(
        "✓ Account id: {}",
        if proxy.auth_data.account_id.is_some() {
            "present"
        } else {
            "missing"
        }
    );
    log::info!("✓ Upstream base URL: {}", proxy.upstream_base_url);
    log::info!(
        "✓ Anthropic upstream base URL: {}",
        proxy.anthropic_upstream_base_url
    );
    let proxy_filter = warp::any().map(move || proxy.clone());
    let cors = warp::cors()
        .allow_any_origin()
        .allow_headers(vec![
            "authorization",
            "content-type",
            "accept",
            "accept-encoding",
            "anthropic-version",
            "x-api-key",
            "x-stainless-arch",
            "x-stainless-lang",
            "x-stainless-os",
            "x-stainless-package-version",
            "x-stainless-retry-count",
            "x-stainless-runtime",
            "x-stainless-runtime-version",
            "x-stainless-timeout",
        ])
        .allow_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"]);
    let universal_handler = warp::any()
        .and(warp::method())
        .and(warp::path::full())
        .and(warp::query::raw().or(warp::any().map(String::new)).unify())
        .and(warp::header::headers_cloned())
        .and(warp::body::bytes())
        .and(proxy_filter.clone())
        .and_then(universal_request_handler);
    let routes = universal_handler.with(cors).with(warp::log("codex_proxy"));
    log::info!(
        "🚀 Codex API Gateway listening on http://127.0.0.1:{}",
        args.port
    );
    log::info!("   Health check: http://127.0.0.1:{}/health", args.port);
    log::info!(
        "   Responses endpoint: http://127.0.0.1:{}/v1/responses",
        args.port
    );
    log::info!(
        "   Chat endpoint: http://127.0.0.1:{}/v1/chat/completions",
        args.port
    );
    log::info!(
        "   Anthropic endpoint: http://127.0.0.1:{}/v1/messages",
        args.port
    );
    log::info!("   Binding mode: localhost-only");
    warp::serve(routes).run(([127, 0, 0, 1], args.port)).await;
    Ok(())
}

async fn universal_request_handler(
    method: warp::http::Method,
    path: warp::path::FullPath,
    query: String,
    headers: warp::http::HeaderMap,
    body: bytes::Bytes,
    proxy: ProxyServer,
) -> Result<impl warp::Reply, warp::Rejection> {
    let path_str = path.as_str();
    log_request(&method, path_str, &headers);
    let response = match (method.as_str(), path_str) {
        ("GET", "/health") => {
            warp::reply::json(&json!({"status":"ok","service":"codex-api-gateway"})).into_response()
        }
        ("GET", "/models") | ("GET", "/v1/models") => {
            warp::reply::json(&gateway_models_response()).into_response()
        }
        ("POST", "/v1/responses") => {
            let responses_req = match serde_json::from_slice::<OpenAiResponsesRequest>(&body) {
                Ok(req) => req,
                Err(err) => return Ok(openai_error_response(ProxyError::invalid_json(err))),
            };
            if responses_req.stream.unwrap_or(false) {
                let (model, payload) = match proxy.convert_openai_responses_request(responses_req) {
                    Ok(v) => v,
                    Err(err) => return Ok(openai_error_response(err)),
                };
                match proxy
                    .proxy_native_responses_stream(model, payload, codex_upstream_retry_config())
                    .await
                {
                    Ok(sse_body) => warp::http::Response::builder()
                        .header("content-type", "text/event-stream")
                        .body(sse_body)
                        .expect("valid SSE response"),
                    Err(err) => openai_error_response(err),
                }
            } else {
                match proxy.proxy_openai_responses_request(responses_req).await {
                    Ok(response) => warp::reply::json(&response).into_response(),
                    Err(err) => openai_error_response(err),
                }
            }
        }
        ("POST", "/v1/chat/completions") => {
            let chat_req = match serde_json::from_slice::<ChatCompletionsRequest>(&body) {
                Ok(req) => req,
                Err(err) => return Ok(openai_error_response(ProxyError::invalid_json(err))),
            };
            if chat_req.stream.unwrap_or(false) {
                match proxy
                    .proxy_request_stream(chat_req, ApiFamily::OpenAi)
                    .await
                {
                    Ok((_model, sse_body)) => {
                        warp::reply::with_header(sse_body, "content-type", "text/event-stream")
                            .into_response()
                    }
                    Err(err) => openai_error_response(err),
                }
            } else {
                match proxy.proxy_request(chat_req).await {
                    Ok(response) => warp::reply::json(&response).into_response(),
                    Err(err) => openai_error_response(err),
                }
            }
        }
        ("POST", "/v1/messages") | ("POST", "/v1/messages/count_tokens") => {
            let count_tokens = path_str == "/v1/messages/count_tokens";
            let (upstream_body, direct_route) =
                match prepare_anthropic_passthrough(&body, !count_tokens) {
                    Ok(prepared) => prepared,
                    Err(error) => return Ok(anthropic_error_response(error)),
                };
            if count_tokens {
                return Ok(if direct_route == AnthropicModelRoute::Anthropic {
                    match proxy
                        .proxy_anthropic_upstream_request(upstream_body, &headers, path_str, &query)
                        .await
                    {
                        Ok(response) => response.into_response(),
                        Err(error) => anthropic_error_response(error),
                    }
                } else {
                    // The client's count API error path returns to its local
                    // estimate. A 501 would activate model sampling instead.
                    not_found_response(ApiFamily::Anthropic)
                });
            }
            let codex_body;
            let typed_body = if direct_route == AnthropicModelRoute::Codex {
                codex_body = match prepare_codex_anthropic_body(&body) {
                    Ok(body) => body,
                    Err(error) => return Ok(anthropic_error_response(error)),
                };
                codex_body.as_slice()
            } else {
                body.as_ref()
            };
            let mut anthropic_req =
                match serde_json::from_slice::<AnthropicMessagesRequest>(typed_body) {
                    Ok(req) => req,
                    Err(err) => return Ok(anthropic_error_response(ProxyError::invalid_json(err))),
                };
            let requested_model = anthropic_req.model.clone();
            let (directive_model, route) = if direct_route == AnthropicModelRoute::Anthropic {
                (None, direct_route)
            } else {
                prepare_anthropic_ingress(&mut anthropic_req)
            };
            log::warn!(
                "[anthropic-route] model={} effective_model={} route={}",
                requested_model,
                anthropic_req.model,
                match route {
                    AnthropicModelRoute::Codex => "codex",
                    AnthropicModelRoute::Anthropic => "anthropic",
                }
            );
            if route == AnthropicModelRoute::Anthropic {
                match proxy
                    .proxy_anthropic_upstream_request(upstream_body, &headers, path_str, &query)
                    .await
                {
                    Ok(upstream_response) => upstream_response.into_response(),
                    Err(err) => anthropic_error_response(err),
                }
            } else {
                let stream = anthropic_req.stream.unwrap_or(false);
                let model = anthropic_req.model.clone();
                let retry_config = codex_retry_config_for_anthropic_codex(
                    &requested_model,
                    directive_model.as_deref(),
                );
                let chat_req = match proxy.convert_anthropic_to_chat(anthropic_req) {
                    Ok(req) => req,
                    Err(err) => return Ok(anthropic_error_response(err)),
                };
                if stream {
                    match proxy
                        .proxy_anthropic_codex_stream(chat_req, retry_config)
                        .await
                    {
                        Ok(body) => warp::http::Response::builder()
                            .header("content-type", "text/event-stream")
                            .body(body)
                            .unwrap(),
                        Err(err) => anthropic_error_response(err),
                    }
                } else {
                    match proxy.proxy_request_with_retry(chat_req, retry_config).await {
                        Ok(response) => warp::reply::json(&convert_chat_to_anthropic(
                            &normalize_codex_model_id(&model),
                            response,
                        ))
                        .into_response(),
                        Err(err) => anthropic_error_response(err),
                    }
                }
            }
        }
        ("GET", "/v1/responses") => method_not_allowed_response(ApiFamily::OpenAi),
        ("GET", "/v1/chat/completions") => method_not_allowed_response(ApiFamily::OpenAi),
        ("GET", "/v1/messages") => method_not_allowed_response(ApiFamily::Anthropic),
        _ => {
            if path_str.starts_with("/v1/messages") {
                not_found_response(ApiFamily::Anthropic)
            } else {
                not_found_response(ApiFamily::OpenAi)
            }
        }
    };
    Ok(response)
}

impl Clone for ProxyServer {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            auth_data: self.auth_data.clone(),
            upstream_base_url: self.upstream_base_url.clone(),
            anthropic_upstream_base_url: self.anthropic_upstream_base_url.clone(),
            anthropic_auth_token: self.anthropic_auth_token.clone(),
            claude_oauth_refresh_lock: self.claude_oauth_refresh_lock.clone(),
        }
    }
}
