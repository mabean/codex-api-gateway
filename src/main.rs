use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use clap::Parser;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;
use warp::{Filter, Reply};

mod improved_response;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// Path to Codex auth.json file
    #[arg(long, default_value = "~/.codex/auth.json")]
    auth_path: String,
}

/// Chat Completions API format (what CLINE sends)
#[derive(Deserialize, Debug)]
struct ChatCompletionsRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: Option<bool>,
    tools: Option<Vec<Value>>,
}

#[derive(Deserialize, Debug)]
struct ChatMessage {
    role: String,
    content: Value, // Can be string or array
}

/// Chat Completions API response format (what CLINE expects)
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

/// Codex Responses API format (what we send to ChatGPT backend)
#[derive(Serialize, Debug)]
struct ResponsesApiRequest {
    model: String,
    instructions: String,
    input: Vec<ResponseItem>,
    tools: Vec<Value>,
    tool_choice: String,
    parallel_tool_calls: bool,
    reasoning: Option<Value>,
    store: bool,
    stream: bool,
    include: Vec<String>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponseItem {
    Message {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        role: String,
        content: Vec<ContentItem>,
    },
}

#[derive(Serialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentItem {
    InputText { text: String },
}

/// Minimal auth material required by the proxy.
#[derive(Debug, Clone)]
struct AuthData {
    api_key: Option<String>,
    access_token: Option<String>,
    account_id: Option<String>,
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
}

impl ProxyServer {
    async fn new(auth_path: &str) -> Result<Self> {
        let auth_path = if auth_path.starts_with("~/") {
            let home = std::env::var("HOME").context("HOME environment variable not set")?;
            auth_path.replace("~", &home)
        } else {
            auth_path.to_string()
        };

        let auth_content = tokio::fs::read_to_string(&auth_path)
            .await
            .with_context(|| format!("Failed to read auth file: {}", auth_path))?;

        let auth_data = Self::parse_auth_data(&auth_content)
            .with_context(|| format!("Failed to parse supported auth file format: {}", auth_path))?;

        if auth_data.api_key.is_none() && auth_data.access_token.is_none() {
            anyhow::bail!("auth file did not contain a usable API key or OAuth access token")
        }

        // Create client with browser-like configuration
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            client,
            auth_data,
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

    fn convert_chat_to_responses(&self, chat_req: ChatCompletionsRequest) -> ResponsesApiRequest {
        // Convert messages to ResponseItems
        let mut input = Vec::new();
        
        for msg in chat_req.messages {
            // Convert content to string (handle both string and array formats)
            let content_text = match &msg.content {
                Value::String(s) => s.clone(),
                Value::Array(arr) => {
                    // Extract text from array elements
                    arr.iter()
                        .filter_map(|v| {
                            if let Some(obj) = v.as_object() {
                                obj.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                            } else {
                                v.as_str().map(|s| s.to_string())
                            }
                        })
                        .collect::<Vec<String>>()
                        .join(" ")
                },
                _ => msg.content.to_string(),
            };
            
            input.push(ResponseItem::Message {
                id: None,
                role: msg.role,
                content: vec![ContentItem::InputText {
                    text: content_text,
                }],
            });
        }

        // Use proper instructions for ChatGPT Responses API
        let instructions = "You are a helpful AI assistant. Provide clear, accurate, and concise responses to user questions and requests.".to_string();

        ResponsesApiRequest {
            model: chat_req.model,
            instructions,
            input,
            tools: chat_req.tools.unwrap_or_default(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            reasoning: None,
            store: false,
            stream: true,
            include: vec![],
        }
    }


    async fn proxy_request(&self, chat_req: ChatCompletionsRequest) -> Result<ChatCompletionsResponse> {
        // Convert to Responses API format
        let responses_req = self.convert_chat_to_responses(chat_req);
        
        // Build request to ChatGPT backend with browser-like headers
        let mut request_builder = self.client
            .post("https://chatgpt.com/backend-api/codex/responses")
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .header("OpenAI-Beta", "responses=experimental")
            .header("originator", "pi")
            .header("User-Agent", "codex-openai-proxy/0.1 local-hardening");

        // Add authentication
        if let Some(access_token) = &self.auth_data.access_token {
            request_builder = request_builder.header("Authorization", format!("Bearer {}", access_token));
            if let Some(account_id) = &self.auth_data.account_id {
                request_builder = request_builder.header("chatgpt-account-id", account_id);
            }
        } else if let Some(api_key) = &self.auth_data.api_key {
            request_builder = request_builder.header("Authorization", format!("Bearer {}", api_key));
        } else {
            anyhow::bail!("no usable auth material found")
        }

        // Add session ID
        let session_id = Uuid::new_v4();
        request_builder = request_builder.header("session_id", session_id.to_string());

        // Send request
        let response = request_builder
            .json(&responses_req)
            .send()
            .await
            .context("Failed to send request to ChatGPT backend")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            
            anyhow::bail!(
                "ChatGPT backend returned {} with body: {}",
                status,
                body
            );
        }

        // Handle streaming response
        let mut response_content = String::new();
        let response_text = response.text().await?;
        let lines: Vec<&str> = response_text.lines().collect();
        
        for line in lines {
            if line.starts_with("data: ") {
                let json_data = &line[6..]; // Remove "data: " prefix
                if json_data == "[DONE]" {
                    break;
                }
                
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(json_data) {
                    if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
                        match event_type {
                            "response.output_text.delta" => {
                                if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                                    response_content.push_str(delta);
                                }
                            }
                            "response.output_item.done" => {
                                if let Some(item) = event.get("item") {
                                    if let Some(content_arr) = item.get("content").and_then(|v| v.as_array()) {
                                        for content_item in content_arr {
                                            if let Some(text) = content_item.get("text").and_then(|v| v.as_str()) {
                                                response_content.push_str(text);
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {} // Ignore other event types
                        }
                    }
                }
            }
        }

        if response_content.is_empty() {
            anyhow::bail!("upstream returned success but no parsable response content")
        }

        // Create Chat Completions response
        let chat_res = ChatCompletionsResponse {
            id: format!("chatcmpl-{}", Uuid::new_v4()),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: responses_req.model.clone(),
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
        };
        
        Ok(chat_res)
    }
}

fn extract_account_id_from_jwt(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let normalized = payload.replace('-', "+").replace('_', "/");
    let padded = match normalized.len() % 4 {
        2 => format!("{}==", normalized),
        3 => format!("{}=", normalized),
        _ => normalized,
    };
    let decoded = URL_SAFE_NO_PAD.decode(padded.as_bytes()).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    value
        .get("https://api.openai.com/auth")
        .and_then(|v| v.get("chatgpt_account_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

// Enhanced logging function
fn log_request(method: &warp::http::Method, path: &str, headers: &warp::http::HeaderMap) {
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f UTC");
    
    println!("\n🔍 === INTERCEPTED REQUEST ===");
    println!("⏰ Timestamp: {}", timestamp);
    println!("📥 Method: {}", method);
    println!("📍 Path: {}", path);
    
    // Log all headers with special attention to problematic ones
    println!("\n📋 Headers ({} total):", headers.len());
    for (name, value) in headers.iter() {
        let header_name = name.as_str().to_lowercase();
        let value_str = match value.to_str() {
            Ok(v) => v,
            Err(_) => "[INVALID UTF-8]"
        };
        
        // Highlight potential CLINE-specific headers
        if header_name.contains("user-agent") || header_name.contains("client") || header_name.contains("cline") {
            println!("  🎯 {}: {}", name, value_str);
        } else if header_name == "authorization" {
            println!("  🔐 {}: {}***", name, &value_str[..std::cmp::min(20, value_str.len())]);
        } else {
            println!("  📄 {}: {}", name, value_str);
        }
    }
    
    // Check for VS Code specific patterns
    let user_agent = headers.get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("none");
    
    if user_agent.to_lowercase().contains("vscode") {
        println!("🎯 DETECTED: VS Code client!");
    }
    if user_agent.to_lowercase().contains("cline") {
        println!("🎯 DETECTED: CLINE extension!");
    }
    
    println!("🔍 === END INTERCEPT ===\n");
}

// Removed catch_all_handler - using inline closure to avoid body consumption conflicts

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    println!("Initializing Codex OpenAI Proxy...");
    
    let proxy = ProxyServer::new(&args.auth_path).await?;
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

    // Health check endpoint (removed unused variable warning)
    let _health = warp::path("health")
        .and(warp::get())
        .map(|| {
            println!("💚 Health check requested");
            warp::reply::json(&json!({
                "status": "ok",
                "service": "codex-openai-proxy"
            }))
        });

    // Multiple endpoints for CLINE compatibility
    let proxy_filter = warp::any().map(move || proxy.clone());
    
    let _chat_completions_v1 = warp::path!("v1" / "chat" / "completions")
        .and(warp::post())
        .and(warp::header::headers_cloned())
        .and(warp::body::json())
        .and(proxy_filter.clone())
        .and_then(handle_chat_completions);
        
    let _chat_completions_direct = warp::path("chat")
        .and(warp::path("completions"))
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::header::headers_cloned())
        .and(warp::body::json())
        .and(proxy_filter.clone())
        .and_then(handle_chat_completions);

    // Models endpoints
    let _models_v1 = warp::path!("v1" / "models")
        .and(warp::get())
        .and(warp::header::headers_cloned())
        .and_then(handle_models);
        
    let _models_direct = warp::path("models")
        .and(warp::get())
        .and(warp::header::headers_cloned())
        .and_then(handle_models);

    // CORS headers - allow all headers to fix CLINE issues
    let cors = warp::cors()
        .allow_any_origin()
        .allow_headers(vec!["authorization", "content-type", "accept", "accept-encoding", "x-stainless-arch", "x-stainless-lang", "x-stainless-os", "x-stainless-package-version", "x-stainless-retry-count", "x-stainless-runtime", "x-stainless-runtime-version", "x-stainless-timeout"])
        .allow_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"]);

    // BULLETPROOF SOLUTION - Single universal handler (removed old catch_all)
    let universal_handler = warp::any()
        .and(warp::method())
        .and(warp::path::full())
        .and(warp::header::headers_cloned())
        .and(warp::body::bytes())
        .and(proxy_filter.clone())
        .and_then(universal_request_handler);

    let routes = universal_handler
        .with(cors)
        .with(warp::log("codex_proxy"));

    println!("🚀 Codex OpenAI Proxy listening on http://0.0.0.0:{}", args.port);
    println!("   Health check: http://localhost:{}/health", args.port);
    println!("   Chat endpoint: http://localhost:{}/v1/chat/completions", args.port);
    println!("\n   Configure CLINE with:");
    println!("   Base URL: http://localhost:{}", args.port);
    println!("   Model: gpt-5");
    println!("   API Key: (any value)");

    warp::serve(routes)
        .run(([0, 0, 0, 0], args.port))
        .await;

    Ok(())
}

// Universal handler that routes based on path and method
async fn universal_request_handler(
    method: warp::http::Method,
    path: warp::path::FullPath,
    headers: warp::http::HeaderMap,
    body: bytes::Bytes,
    proxy: ProxyServer,
) -> Result<impl warp::Reply, warp::Rejection> {
    let path_str = path.as_str();
    
    log_request(&method, path_str, &headers);
    
    match (method.as_str(), path_str) {
        ("GET", "/health") => {
            println!("💚 Health check requested");
            Ok(warp::reply::json(&json!({
                "status": "ok",
                "service": "codex-openai-proxy"
            })).into_response())
        },
        ("GET", "/models") | ("GET", "/v1/models") => {
            println!("📋 === MATCHED MODELS REQUEST ===");
            println!("📋 === END MATCHED ===\n");
            
            let models_response = json!({
                "object": "list",
                "data": [
                    {
                        "id": "gpt-4",
                        "object": "model",
                        "created": 1687882411,
                        "owned_by": "openai"
                    },
                    {
                        "id": "gpt-5",
                        "object": "model", 
                        "created": 1687882411,
                        "owned_by": "openai"
                    }
                ]
            });
            
            Ok(warp::reply::json(&models_response).into_response())
        },
        ("POST", "/chat/completions") | ("POST", "/v1/chat/completions") => {
            println!("🔥 === MATCHED CHAT COMPLETIONS ===");
            
            // LOG EXACT CLINE REQUEST FOR CURL REPLICATION
            println!("\n📋 === CLINE REQUEST DETAILS FOR CURL ===");
            println!("Method: POST");
            println!("Path: {}", path_str);
            println!("Body size: {} bytes", body.len());
            
            // Log all headers in curl format
            println!("\nHeaders for curl:");
            for (name, value) in headers.iter() {
                if let Ok(value_str) = value.to_str() {
                    if name.as_str().to_lowercase() == "authorization" {
                        println!("  -H \"{}: {}***\"", name, &value_str[..std::cmp::min(20, value_str.len())]);
                    } else if name.as_str().to_lowercase().starts_with("x-forwarded") {
                        println!("  # Skip: -H \"{}: {}\"", name, value_str);
                    } else {
                        println!("  -H \"{}: {}\"", name, value_str);
                    }
                }
            }
            
            // Log body (truncated for readability)
            println!("\nBody (first 1000 chars):");
            if let Ok(body_str) = std::str::from_utf8(&body) {
                let truncated = if body_str.len() > 1000 {
                    format!("{}... [TRUNCATED]", &body_str[..1000])
                } else {
                    body_str.to_string()
                };
                println!("{}", truncated);
                
                // Generate curl command
                println!("\n🚀 CURL COMMAND TO REPLICATE:");
                println!("curl -X POST http://localhost:8888{} \\", path_str);
                for (name, value) in headers.iter() {
                    if let Ok(value_str) = value.to_str() {
                        if !name.as_str().to_lowercase().starts_with("x-forwarded") 
                           && name.as_str().to_lowercase() != "host" {
                            if name.as_str().to_lowercase() == "authorization" {
                                println!("  -H \"{}: test-key\" \\", name);
                            } else {
                                println!("  -H \"{}: {}\" \\", name, value_str);
                            }
                        }
                    }
                }
                println!("  -d '{}'", body_str.chars().take(500).collect::<String>());
            }
            println!("📋 === END CLINE REQUEST DETAILS ===\n");
            
            // Parse JSON from bytes
            let chat_req: ChatCompletionsRequest = match serde_json::from_slice(&body) {
                Ok(req) => req,
                Err(e) => {
                    println!("❌ JSON parse error: {}", e);
                    return Ok(warp::reply::with_status(
                        "Invalid JSON",
                        warp::http::StatusCode::BAD_REQUEST
                    ).into_response());
                }
            };
            
            println!("   Model: {}", chat_req.model);
            println!("   Messages: {} items", chat_req.messages.len());
            for (i, msg) in chat_req.messages.iter().enumerate() {
                let content_preview = match &msg.content {
                    Value::String(s) => s.chars().take(50).collect::<String>(),
                    Value::Array(arr) => format!("[array with {} items]", arr.len()),
                    _ => format!("[{}]", msg.content.to_string().chars().take(50).collect::<String>()),
                };
                println!("   [{}] {}: {}", i, msg.role, content_preview);
            }
            println!("🔥 === END MATCHED ===\n");
            
            // Check if streaming is requested
            if chat_req.stream.unwrap_or(false) {
                println!("🔄 STREAMING: CLINE requested streaming response, implementing SSE format");
                
                // Generate contextual response based on user messages
                let message = improved_response::generate_contextual_response(&chat_req.messages);
                println!("📝 Generated contextual response: {}", &message[..std::cmp::min(100, message.len())]);
                
                let chunk_id = "chatcmpl-streaming-12345";
                let model = chat_req.model.clone();
                
                let sse_chunks = vec![
                    // First chunk with role
                    format!("data: {{\"id\":\"{}\",\"object\":\"chat.completion.chunk\",\"created\":{},\"model\":\"{}\",\"choices\":[{{\"index\":0,\"delta\":{{\"role\":\"assistant\"}},\"finish_reason\":null}}]}}\n\n", 
                            chunk_id, chrono::Utc::now().timestamp(), model),
                    // Content chunk
                    format!("data: {{\"id\":\"{}\",\"object\":\"chat.completion.chunk\",\"created\":{},\"model\":\"{}\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{}\"}},\"finish_reason\":null}}]}}\n\n", 
                            chunk_id, chrono::Utc::now().timestamp(), model, message),
                    // Final chunk
                    format!("data: {{\"id\":\"{}\",\"object\":\"chat.completion.chunk\",\"created\":{},\"model\":\"{}\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\n", 
                            chunk_id, chrono::Utc::now().timestamp(), model),
                    // End marker
                    "data: [DONE]\n\n".to_string(),
                ];
                
                let sse_response = sse_chunks.join("");
                let reply = warp::reply::with_header(sse_response, "content-type", "text/event-stream");
                let reply = warp::reply::with_header(reply, "cache-control", "no-cache");
                let reply = warp::reply::with_header(reply, "connection", "keep-alive");
                let reply = warp::reply::with_header(reply, "access-control-allow-origin", "*");
                Ok(reply.into_response())
            } else {
                match proxy.proxy_request(chat_req).await {
                    Ok(response) => {
                        let reply = warp::reply::json(&response);
                        let reply = warp::reply::with_header(reply, "content-type", "application/json");
                        let reply = warp::reply::with_header(reply, "access-control-allow-origin", "*");
                        Ok(reply.into_response())
                    },
                    Err(e) => {
                        eprintln!("Proxy error: {:#}", e);
                        let reply = warp::reply::json(&json!({
                            "error": {
                                "message": format!("Proxy error: {}", e),
                                "type": "proxy_error",
                                "code": "internal_error"
                            }
                        }));
                        let reply = warp::reply::with_header(reply, "content-type", "application/json");
                        let reply = warp::reply::with_header(reply, "access-control-allow-origin", "*");
                        Ok(reply.into_response())
                    }
                }
            }
        },
        _ => {
            println!("❌ UNMATCHED: {} {}", method, path_str);
            Ok(warp::reply::with_status(
                "Not found",
                warp::http::StatusCode::NOT_FOUND
            ).into_response())
        }
    }
}

async fn handle_models(
    headers: warp::http::HeaderMap,
) -> Result<impl warp::Reply, warp::Rejection> {
    log_request(&warp::http::Method::GET, "/models", &headers);
    
    println!("📋 === MATCHED MODELS REQUEST ===");
    println!("📋 === END MATCHED ===\n");
    
    // Return a simple models list for CLINE
    let models_response = json!({
        "object": "list",
        "data": [
            {
                "id": "gpt-4",
                "object": "model",
                "created": 1687882411,
                "owned_by": "openai"
            },
            {
                "id": "gpt-5",
                "object": "model", 
                "created": 1687882411,
                "owned_by": "openai"
            }
        ]
    });
    
    Ok(warp::reply::json(&models_response))
}

async fn handle_chat_completions(
    headers: warp::http::HeaderMap,
    req: ChatCompletionsRequest,
    proxy: ProxyServer,
) -> Result<impl warp::Reply, warp::Rejection> {
    // Enhanced logging for successful matches
    log_request(&warp::http::Method::POST, "/chat/completions", &headers);
    
    println!("🔥 === MATCHED CHAT COMPLETIONS ===");
    println!("   Model: {}", req.model);
    println!("   Messages: {} items", req.messages.len());
    println!("🔥 === END MATCHED ===\n");
    
    match proxy.proxy_request(req).await {
        Ok(response) => Ok(warp::reply::json(&response)),
        Err(e) => {
            eprintln!("Proxy error: {:#}", e);
            Ok(warp::reply::json(&json!({
                "error": {
                    "message": format!("Proxy error: {}", e),
                    "type": "proxy_error",
                    "code": "internal_error"
                }
            })))
        }
    }
}

// Make ProxyServer cloneable for warp filters
impl Clone for ProxyServer {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            auth_data: self.auth_data.clone(),
        }
    }
}