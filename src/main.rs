use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use clap::Parser;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use streaming::{parse_codex_sse_to_events, render_anthropic_sse, render_openai_sse};
use uuid::Uuid;
use warp::{http::StatusCode, Filter, Reply};

mod streaming;
#[cfg(test)]
mod tests;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "8080")]
    port: u16,
    #[arg(long, default_value = "~/.codex/auth.json")]
    auth_path: String,
    #[arg(long, default_value = "https://chatgpt.com/backend-api")]
    upstream_base_url: String,
}

#[derive(Deserialize, Debug, Clone)]
struct ChatCompletionsRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: Option<bool>,
    tools: Option<Vec<Value>>,
}

#[derive(Deserialize, Debug, Clone)]
struct AnthropicMessagesRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    system: Option<Value>,
    max_tokens: Option<u32>,
    stream: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
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
    content: Vec<AnthropicTextBlock>,
    model: String,
    stop_reason: Option<String>,
    stop_sequence: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Serialize, Debug)]
struct AnthropicTextBlock {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

#[derive(Serialize, Debug)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
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
    tool_choice: String,
    parallel_tool_calls: bool,
    reasoning: Option<Value>,
    store: bool,
    stream: bool,
    text: Value,
    include: Vec<String>,
    prompt_cache_key: String,
}

#[derive(Serialize, Debug)]
#[serde(untagged)]
enum ResponsesInputItem {
    UserMessage {
        role: String,
        content: Vec<InputContentItem>,
    },
}

#[derive(Serialize, Debug)]
#[serde(tag = "type")]
enum InputContentItem {
    #[serde(rename = "input_text")]
    InputText { text: String },
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
    access: Option<String>,
}

struct ProxyServer {
    client: Client,
    auth_data: AuthData,
    upstream_base_url: String,
}

impl ProxyServer {
    async fn new(auth_path: &str, upstream_base_url: &str) -> Result<Self> {
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
        let client = Client::builder()
            .user_agent("pi (codex-api-gateway)")
            .build()
            .context("Failed to create HTTP client")?;
        Ok(Self {
            client,
            auth_data,
            upstream_base_url: upstream_base_url.to_string(),
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
            if let (Some(profiles), Some(last_good)) = (openclaw.profiles, openclaw.last_good) {
                if let Some(profile_id) = last_good.get("openai-codex") {
                    if let Some(profile) = profiles.get(profile_id) {
                        if profile.profile_type.as_deref() == Some("oauth") {
                            let access_token = profile.access.clone();
                            let account_id = access_token
                                .as_ref()
                                .and_then(|t| extract_account_id_from_jwt(t));
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
        }
        anyhow::bail!("unsupported auth file format")
    }

    fn convert_anthropic_to_chat(
        &self,
        anthropic_req: AnthropicMessagesRequest,
    ) -> Result<ChatCompletionsRequest, ProxyError> {
        validate_anthropic_request(&anthropic_req)?;
        let mut messages = Vec::new();
        if let Some(system) = anthropic_req.system {
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: system,
            });
        }
        for msg in anthropic_req.messages {
            messages.push(ChatMessage {
                role: msg.role,
                content: msg.content,
            });
        }
        Ok(ChatCompletionsRequest {
            model: anthropic_req.model,
            messages,
            stream: anthropic_req.stream,
            tools: None,
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
            let content_text = flatten_message_content(&msg.content).map_err(|message| {
                ProxyError::Validation {
                    message,
                    field: Some("messages[].content".to_string()),
                }
            })?;
            match msg.role.as_str() {
                "system" => system_parts.push(content_text),
                "user" => input.push(ResponsesInputItem::UserMessage {
                    role: "user".to_string(),
                    content: vec![InputContentItem::InputText { text: content_text }],
                }),
                "assistant" => {}
                _ => input.push(ResponsesInputItem::UserMessage {
                    role: "user".to_string(),
                    content: vec![InputContentItem::InputText { text: content_text }],
                }),
            }
        }
        if input.is_empty() {
            return Err(ProxyError::Validation {
                message: "at least one user message is required for the current hardened path"
                    .to_string(),
                field: Some("messages".to_string()),
            });
        }
        let instructions = if system_parts.is_empty() {
            "You are a helpful AI assistant. Provide clear, accurate, and concise responses to user questions and requests.".to_string()
        } else {
            system_parts.join("\n\n")
        };
        Ok(ResponsesApiRequest {
            model: normalize_codex_model_id(&chat_req.model),
            instructions,
            input,
            tools: chat_req.tools.unwrap_or_default(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: true,
            reasoning: None,
            store: false,
            stream: true,
            text: json!({"verbosity":"medium","format":{"type":"text"}}),
            include: vec!["reasoning.encrypted_content".to_string()],
            prompt_cache_key: Uuid::new_v4().to_string(),
        })
    }

    async fn upstream_sse_text(
        &self,
        chat_req: ChatCompletionsRequest,
    ) -> Result<(String, String), ProxyError> {
        let responses_req = self.convert_chat_to_responses(chat_req)?;
        let model = responses_req.model.clone();
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
            .header("User-Agent", "pi (codex-api-gateway)");
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
        let response = request_builder
            .json(&responses_req)
            .send()
            .await
            .map_err(|e| ProxyError::UpstreamUnavailable {
                message: format!("failed to send request to upstream: {}", e),
            })?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| ProxyError::UpstreamProtocol {
                message: format!("failed to read upstream response body: {}", e),
            })?;
        if !status.is_success() {
            let message = if body.trim().is_empty() {
                format!("upstream returned {}", status)
            } else {
                format!("upstream returned {} with body: {}", status, body)
            };
            return Err(classify_upstream_error(status, message));
        }
        Ok((model, body))
    }

    async fn proxy_request_stream(
        &self,
        chat_req: ChatCompletionsRequest,
        api_family: ApiFamily,
    ) -> Result<(String, String), ProxyError> {
        let (model, sse_text) = self.upstream_sse_text(chat_req).await?;
        let events = parse_codex_sse_to_events(&sse_text)?;
        let rendered = match api_family {
            ApiFamily::OpenAi => render_openai_sse(&events, &model),
            ApiFamily::Anthropic => render_anthropic_sse(&events, &model),
        };
        Ok((model, rendered))
    }

    async fn proxy_request(
        &self,
        chat_req: ChatCompletionsRequest,
    ) -> Result<ChatCompletionsResponse, ProxyError> {
        let (model, body) = self.upstream_sse_text(chat_req).await?;
        let response_content = extract_response_content(&body)?;
        Ok(ChatCompletionsResponse {
            id: format!("chatcmpl-{}", Uuid::new_v4()),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model,
            choices: vec![Choice {
                index: 0,
                message: ChatResponseMessage {
                    role: "assistant".to_string(),
                    content: response_content,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            }),
        })
    }
}

fn normalize_codex_model_id(model: &str) -> String {
    let short = model.rsplit('/').next().unwrap_or(model);
    let lower = short.to_lowercase();
    if lower.starts_with("claude-") {
        "gpt-5.4".to_string()
    } else {
        short.to_string()
    }
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
                message: "anthropic messages role must be 'user' or 'assistant'".to_string(),
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
    let text = chat
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .unwrap_or_default();
    AnthropicMessagesResponse {
        id: format!("msg_{}", Uuid::new_v4().simple()),
        response_type: "message".to_string(),
        role: "assistant".to_string(),
        content: vec![AnthropicTextBlock {
            content_type: "text".to_string(),
            text,
        }],
        model: source_model.to_string(),
        stop_reason: Some("end_turn".to_string()),
        stop_sequence: None,
        usage: AnthropicUsage {
            input_tokens: 0,
            output_tokens: 0,
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
    println!(
        "[request] method={} path={} auth_present={} user_agent={}",
        method, path, auth_present, ua
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();
    let proxy = ProxyServer::new(&args.auth_path, &args.upstream_base_url).await?;
    println!("Initializing Codex API Gateway...");
    println!("✓ Loaded authentication from {}", args.auth_path);
    println!(
        "✓ Auth mode: {}",
        if proxy.auth_data.access_token.is_some() {
            "oauth/codex"
        } else if proxy.auth_data.api_key.is_some() {
            "api-key"
        } else {
            "unknown"
        }
    );
    println!(
        "✓ Account id: {}",
        if proxy.auth_data.account_id.is_some() {
            "present"
        } else {
            "missing"
        }
    );
    println!("✓ Upstream base URL: {}", proxy.upstream_base_url);
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
        .and(warp::header::headers_cloned())
        .and(warp::body::bytes())
        .and(proxy_filter.clone())
        .and_then(universal_request_handler);
    let routes = universal_handler.with(cors).with(warp::log("codex_proxy"));
    println!(
        "🚀 Codex API Gateway listening on http://127.0.0.1:{}",
        args.port
    );
    println!("   Health check: http://127.0.0.1:{}/health", args.port);
    println!(
        "   Chat endpoint: http://127.0.0.1:{}/v1/chat/completions",
        args.port
    );
    println!(
        "   Anthropic endpoint: http://127.0.0.1:{}/v1/messages",
        args.port
    );
    println!("   Binding mode: localhost-only");
    warp::serve(routes).run(([127, 0, 0, 1], args.port)).await;
    Ok(())
}

async fn universal_request_handler(
    method: warp::http::Method,
    path: warp::path::FullPath,
    headers: warp::http::HeaderMap,
    body: bytes::Bytes,
    proxy: ProxyServer,
) -> Result<impl warp::Reply, warp::Rejection> {
    let path_str = path.as_str();
    log_request(&method, path_str, &headers);
    let response = match (method.as_str(), path_str) {
        ("GET", "/health") => warp::reply::json(&json!({"status":"ok","service":"codex-api-gateway"})).into_response(),
        ("GET", "/models") | ("GET", "/v1/models") => warp::reply::json(&json!({"object":"list","data":[{"id":"gpt-5.4","object":"model","created":1687882411,"owned_by":"openai-codex"}]})).into_response(),
        ("POST", "/v1/chat/completions") => {
            let chat_req = match serde_json::from_slice::<ChatCompletionsRequest>(&body) { Ok(req) => req, Err(err) => return Ok(openai_error_response(ProxyError::invalid_json(err))) };
            if chat_req.stream.unwrap_or(false) {
                match proxy.proxy_request_stream(chat_req, ApiFamily::OpenAi).await {
                    Ok((_model, sse_body)) => warp::reply::with_header(sse_body, "content-type", "text/event-stream").into_response(),
                    Err(err) => openai_error_response(err),
                }
            } else {
                match proxy.proxy_request(chat_req).await { Ok(response) => warp::reply::json(&response).into_response(), Err(err) => openai_error_response(err) }
            }
        }
        ("POST", "/v1/messages") => {
            let anthropic_req = match serde_json::from_slice::<AnthropicMessagesRequest>(&body) { Ok(req) => req, Err(err) => return Ok(anthropic_error_response(ProxyError::invalid_json(err))) };
            let stream = anthropic_req.stream.unwrap_or(false);
            let model = anthropic_req.model.clone();
            let chat_req = match proxy.convert_anthropic_to_chat(anthropic_req) { Ok(req) => req, Err(err) => return Ok(anthropic_error_response(err)) };
            if stream {
                match proxy.proxy_request_stream(chat_req, ApiFamily::Anthropic).await {
                    Ok((_model, sse_body)) => warp::reply::with_header(sse_body, "content-type", "text/event-stream").into_response(),
                    Err(err) => anthropic_error_response(err),
                }
            } else {
                match proxy.proxy_request(chat_req).await {
                    Ok(response) => warp::reply::json(&convert_chat_to_anthropic(&normalize_codex_model_id(&model), response)).into_response(),
                    Err(err) => anthropic_error_response(err),
                }
            }
        }
        ("GET", "/v1/chat/completions") => method_not_allowed_response(ApiFamily::OpenAi),
        ("GET", "/v1/messages") => method_not_allowed_response(ApiFamily::Anthropic),
        _ => if path_str.starts_with("/v1/messages") { not_found_response(ApiFamily::Anthropic) } else { not_found_response(ApiFamily::OpenAi) },
    };
    Ok(response)
}

impl Clone for ProxyServer {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            auth_data: self.auth_data.clone(),
            upstream_base_url: self.upstream_base_url.clone(),
        }
    }
}
