use std::{fs, sync::Mutex};

use tempfile::{tempdir, NamedTempFile};

use crate::{
    anthropic_beta_header, anthropic_upstream_first_chunk_timeout,
    apply_claude_code_oauth_system_prompt, apply_gateway_model_directive,
    codex_agent_upstream_max_attempts, codex_agent_upstream_timeout,
    codex_retry_config_for_anthropic_codex, codex_upstream_max_attempts, codex_upstream_timeout,
    extract_response_content, flatten_message_content, gateway_models_response,
    inject_gateway_agent_instruction, is_anthropic_oauth_token, normalize_anthropic_model_id,
    normalize_anthropic_request_model, normalize_codex_model_id, normalize_openai_responses_input,
    parse_claude_cli_access_token, prepare_anthropic_ingress, render_openai_responses_json,
    route_anthropic_model, AnthropicMessage, AnthropicMessagesRequest, AnthropicModelRoute,
    ChatCompletionsRequest, ChatMessage, OpenAiResponsesRequest, ProxyError, ProxyServer,
    RotatingFile,
};

static ALIAS_ENV_LOCK: Mutex<()> = Mutex::new(());
const ALIAS_ENV_KEYS: &[&str] = &[
    "CODEX_GATEWAY_ALIAS_SONNET",
    "CODEX_GATEWAY_ALIAS_OPUS",
    "CODEX_GATEWAY_ALIAS_HAIKU",
    "CODEX_GATEWAY_CODEX_TIMEOUT_SECS",
    "CODEX_GATEWAY_CODEX_MAX_ATTEMPTS",
    "CODEX_GATEWAY_CODEX_AGENT_TIMEOUT_SECS",
    "CODEX_GATEWAY_CODEX_AGENT_MAX_ATTEMPTS",
    "CODEX_GATEWAY_ANTHROPIC_FIRST_CHUNK_TIMEOUT_SECS",
];

fn clear_alias_env() {
    for key in ALIAS_ENV_KEYS {
        std::env::remove_var(key);
    }
}

#[test]
fn normalizes_provider_prefixed_model_ids() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();

    assert_eq!(normalize_codex_model_id("openai/gpt-5.4"), "gpt-5.4");
    assert_eq!(normalize_codex_model_id("gpt-5.4"), "gpt-5.4");
    assert_eq!(normalize_codex_model_id("gpt-5.6"), "gpt-5.6-sol");
    assert_eq!(normalize_codex_model_id("openai/gpt-5.6"), "gpt-5.6-sol");
    assert_eq!(
        normalize_codex_model_id("anthropic/claude-sonnet-4-6"),
        "claude-sonnet-4-6"
    );
}

#[test]
fn routes_anthropic_ingress_by_model_family() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();

    assert_eq!(
        route_anthropic_model("claude-sonnet-4-6"),
        AnthropicModelRoute::Anthropic
    );
    assert_eq!(
        route_anthropic_model("anthropic/claude-opus-4-1"),
        AnthropicModelRoute::Anthropic
    );
    assert_eq!(route_anthropic_model("gpt-5.4"), AnthropicModelRoute::Codex);
    assert_eq!(
        route_anthropic_model("openai/gpt-5.4"),
        AnthropicModelRoute::Codex
    );
    assert_eq!(
        route_anthropic_model("sonnet"),
        AnthropicModelRoute::Anthropic
    );
    assert_eq!(
        route_anthropic_model("opus"),
        AnthropicModelRoute::Anthropic
    );
    assert_eq!(
        route_anthropic_model("haiku"),
        AnthropicModelRoute::Anthropic
    );
}

#[test]
fn normalizes_anthropic_model_aliases_to_current_versions() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();

    assert_eq!(normalize_anthropic_model_id("sonnet"), "claude-sonnet-5");
    assert_eq!(normalize_anthropic_model_id("opus"), "claude-opus-5");
    assert_eq!(
        normalize_anthropic_model_id("haiku"),
        "claude-haiku-4-5-20251001"
    );
    assert_eq!(
        normalize_anthropic_model_id("claude-sonnet-4-5"),
        "claude-sonnet-4-5-20250929"
    );
}

#[test]
fn can_remap_claude_code_agent_alias_to_codex_model() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();
    std::env::set_var("CODEX_GATEWAY_ALIAS_HAIKU", "openai/gpt-5.4-mini");

    assert_eq!(route_anthropic_model("haiku"), AnthropicModelRoute::Codex);
    assert_eq!(normalize_codex_model_id("haiku"), "gpt-5.4-mini");

    clear_alias_env();
}

#[test]
fn gateway_model_directive_overrides_agent_alias_per_request() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();

    let mut req = AnthropicMessagesRequest {
        model: "haiku".to_string(),
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: serde_json::json!("[gateway-model: gpt-5.5]\nRun this subagent task."),
        }],
        system: None,
        max_tokens: Some(64),
        stream: Some(false),
        tools: None,
    };

    let target = apply_gateway_model_directive(&mut req).unwrap();
    req.model = target;

    assert_eq!(req.model, "gpt-5.5");
    assert_eq!(
        route_anthropic_model(&req.model),
        AnthropicModelRoute::Codex
    );
    assert_eq!(
        req.messages[0].content,
        serde_json::json!("Run this subagent task.")
    );
}

#[test]
fn gateway_model_directive_ignores_non_ascii_text_without_panicking() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();

    let mut req = AnthropicMessagesRequest {
        model: "claude-sonnet-4-6".to_string(),
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: serde_json::json!(
                "Claude code сейчас должен работать через специальный gateway."
            ),
        }],
        system: None,
        max_tokens: Some(64),
        stream: Some(false),
        tools: None,
    };

    assert!(apply_gateway_model_directive(&mut req).is_none());
    assert_eq!(req.model, "claude-sonnet-4-6");
}

#[test]
fn codex_upstream_retry_settings_are_configurable_and_clamped() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();

    assert_eq!(codex_upstream_timeout().as_secs(), 30);
    assert_eq!(codex_upstream_max_attempts(), 1);

    std::env::set_var("CODEX_GATEWAY_CODEX_TIMEOUT_SECS", "1");
    std::env::set_var("CODEX_GATEWAY_CODEX_MAX_ATTEMPTS", "99");

    assert_eq!(codex_upstream_timeout().as_secs(), 5);
    assert_eq!(codex_upstream_max_attempts(), 5);

    std::env::set_var("CODEX_GATEWAY_CODEX_TIMEOUT_SECS", "75");
    std::env::set_var("CODEX_GATEWAY_CODEX_MAX_ATTEMPTS", "3");

    assert_eq!(codex_upstream_timeout().as_secs(), 75);
    assert_eq!(codex_upstream_max_attempts(), 3);

    clear_alias_env();
}

#[test]
fn codex_agent_upstream_retry_settings_are_short_by_default() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();

    assert_eq!(codex_agent_upstream_timeout().as_secs(), 12);
    assert_eq!(codex_agent_upstream_max_attempts(), 1);

    std::env::set_var("CODEX_GATEWAY_CODEX_AGENT_TIMEOUT_SECS", "2");
    std::env::set_var("CODEX_GATEWAY_CODEX_AGENT_MAX_ATTEMPTS", "9");

    assert_eq!(codex_agent_upstream_timeout().as_secs(), 5);
    assert_eq!(codex_agent_upstream_max_attempts(), 5);

    clear_alias_env();
}

#[test]
fn anthropic_upstream_first_chunk_timeout_is_short_by_default() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();

    assert_eq!(anthropic_upstream_first_chunk_timeout().as_secs(), 12);

    std::env::set_var("CODEX_GATEWAY_ANTHROPIC_FIRST_CHUNK_TIMEOUT_SECS", "1");
    assert_eq!(anthropic_upstream_first_chunk_timeout().as_secs(), 3);

    std::env::set_var("CODEX_GATEWAY_ANTHROPIC_FIRST_CHUNK_TIMEOUT_SECS", "200");
    assert_eq!(anthropic_upstream_first_chunk_timeout().as_secs(), 120);

    clear_alias_env();
}

#[test]
fn codex_agent_directive_uses_agent_retry_policy() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();

    let retry_config =
        codex_retry_config_for_anthropic_codex("claude-haiku-4-5-20251001", Some("gpt-5.5"));
    assert_eq!(retry_config.timeout.as_secs(), 12);
    assert_eq!(retry_config.max_attempts, 1);
    assert_eq!(retry_config.policy_name, "claude-code-agent");

    let direct_config = codex_retry_config_for_anthropic_codex("gpt-5.5", None);
    assert_eq!(direct_config.timeout.as_secs(), 30);
    assert_eq!(direct_config.max_attempts, 1);
    assert_eq!(direct_config.policy_name, "default");
}

#[test]
fn gateway_model_directive_can_target_claude_versions() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();

    let mut req = AnthropicMessagesRequest {
        model: "haiku".to_string(),
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: serde_json::json!([{
                "type": "text",
                "text": "[gateway-model: claude-opus-4-7]\nReview this carefully."
            }]),
        }],
        system: None,
        max_tokens: Some(64),
        stream: Some(false),
        tools: None,
    };

    let target = apply_gateway_model_directive(&mut req).unwrap();
    req.model = target;

    assert_eq!(req.model, "claude-opus-4-7");
    assert_eq!(
        route_anthropic_model(&req.model),
        AnthropicModelRoute::Anthropic
    );
    assert_eq!(
        req.messages[0].content[0]["text"],
        serde_json::json!("Review this carefully.")
    );
}

#[test]
fn injects_gateway_agent_instruction_when_agent_tool_is_present() {
    let mut req = AnthropicMessagesRequest {
        model: "claude-sonnet-4-6".to_string(),
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: serde_json::json!("Use a subagent."),
        }],
        system: Some(serde_json::json!("Base system.")),
        max_tokens: Some(64),
        stream: Some(false),
        tools: Some(vec![serde_json::json!({
            "name": "Agent",
            "description": "Launch a subagent",
            "input_schema": {
                "type": "object",
                "properties": {
                    "model": {"type": "string", "enum": ["sonnet", "opus", "haiku"]},
                    "prompt": {"type": "string"}
                }
            }
        })]),
    };

    inject_gateway_agent_instruction(&mut req);

    let system = req.system.unwrap().as_str().unwrap().to_string();
    assert!(system.contains("Base system."));
    assert!(system.contains("[gateway-model: gpt-5.6-sol]"));
    assert!(system.contains("Agent.model to haiku"));
}

#[test]
fn rewrites_anthropic_request_model_aliases_before_upstream() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();

    let body = br#"{
        "model": "sonnet",
        "max_tokens": 64,
        "messages": [{"role": "user", "content": "hello"}]
    }"#;

    let normalized = normalize_anthropic_request_model(body).unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&normalized).unwrap();

    assert_eq!(payload["model"], serde_json::json!("claude-sonnet-5"));
}

#[test]
fn models_endpoint_lists_codex_and_all_known_anthropic_versions() {
    let response = gateway_models_response();
    let ids = response["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|model| model["id"].as_str())
        .collect::<Vec<_>>();

    assert!(ids.contains(&"gpt-5.6"));
    assert!(ids.contains(&"gpt-5.6-sol"));
    assert!(ids.contains(&"gpt-5.6-terra"));
    assert!(ids.contains(&"gpt-5.6-luna"));
    assert!(ids.contains(&"gpt-5.5"));
    assert!(ids.contains(&"gpt-5.4"));
    assert!(ids.contains(&"gpt-5.4-mini"));
    assert!(ids.contains(&"claude-fable-5"));
    assert!(ids.contains(&"claude-opus-5"));
    assert!(ids.contains(&"claude-sonnet-5"));
    assert!(ids.contains(&"claude-opus-4-8"));
    assert!(ids.contains(&"claude-opus-4-7"));
    assert!(ids.contains(&"claude-sonnet-4-6"));
    assert!(ids.contains(&"claude-opus-4-6"));
    assert!(ids.contains(&"claude-opus-4-5-20251101"));
    assert!(ids.contains(&"claude-haiku-4-5-20251001"));
    assert!(ids.contains(&"claude-sonnet-4-5-20250929"));
    assert!(ids.contains(&"claude-opus-4-1-20250805"));
    assert!(ids.contains(&"claude-opus-4-20250514"));
    assert!(ids.contains(&"claude-sonnet-4-20250514"));
}

#[test]
fn detects_anthropic_oauth_tokens() {
    assert!(is_anthropic_oauth_token("sk-ant-oat01-test-token"));
    assert!(!is_anthropic_oauth_token("sk-ant-api03-test-token"));
}

#[test]
fn parses_claude_cli_credentials_contract() {
    let token = parse_claude_cli_access_token(
        r#"{
            "claudeAiOauth": {
                "accessToken": "sk-ant-oat01-test-token",
                "refreshToken": "refresh-token",
                "expiresAt": 1893456000000
            }
        }"#,
    );
    assert_eq!(token.as_deref(), Some("sk-ant-oat01-test-token"));

    assert!(parse_claude_cli_access_token(
        r#"{"claudeAiOauth":{"accessToken":"","expiresAt":1893456000000}}"#
    )
    .is_none());
    assert!(parse_claude_cli_access_token(
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-expired-token","expiresAt":1}}"#
    )
    .is_none());
}

#[test]
fn builds_claude_oauth_beta_header_from_claude_code_contract() {
    let mut headers = warp::http::HeaderMap::new();
    headers.insert("anthropic-beta", "custom-beta-2026-01-01".parse().unwrap());

    let beta = anthropic_beta_header(true, &headers).unwrap();

    assert!(beta.contains("oauth-2025-04-20"));
    assert!(beta.contains("custom-beta-2026-01-01"));
    assert!(!beta.contains("claude-code-20250219"));
    assert!(!beta.contains("fine-grained-tool-streaming-2025-05-14"));
}

#[test]
fn prefixes_claude_oauth_system_prompt_with_billing_marker() {
    let body = br#"{
        "model": "claude-sonnet-4-6",
        "max_tokens": 64,
        "system": "Existing instruction.",
        "messages": [{"role": "user", "content": "hello"}]
    }"#;

    let normalized = apply_claude_code_oauth_system_prompt(body).unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&normalized).unwrap();
    let system = payload["system"].as_array().unwrap();

    assert_eq!(
        system[0]["text"].as_str().unwrap(),
        "x-anthropic-billing-header: cc_version=2.1.119.unknown; cc_entrypoint=unknown; cch=00000;"
    );
    assert_eq!(
        system[1]["text"].as_str().unwrap(),
        "You are Claude Code, Anthropic's official CLI for Claude."
    );
    assert_eq!(system[2]["text"].as_str().unwrap(), "Existing instruction.");
}

#[tokio::test]
async fn parses_legacy_auth_file() {
    let file = NamedTempFile::new().unwrap();
    fs::write(
        file.path(),
        r#"{
            "OPENAI_API_KEY": null,
            "tokens": {
                "access_token": "legacy-token",
                "account_id": "acc_123"
            }
        }"#,
    )
    .unwrap();

    let proxy = ProxyServer::new(
        file.path().to_str().unwrap(),
        "https://example.test",
        "https://api.anthropic.test",
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        proxy.auth_data.access_token.as_deref(),
        Some("legacy-token")
    );
    assert_eq!(proxy.auth_data.account_id.as_deref(), Some("acc_123"));
}

#[tokio::test]
async fn parses_openclaw_auth_profiles_file() {
    let file = NamedTempFile::new().unwrap();
    let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjX29wZW5jbGF3In19.signature";
    fs::write(
        file.path(),
        format!(
            r#"{{
                "profiles": {{
                    "profile-1": {{
                        "type": "oauth",
                        "access": "{}"
                    }}
                }},
                "lastGood": {{
                    "openai-codex": "profile-1"
                }}
            }}"#,
            jwt
        ),
    )
    .unwrap();

    let proxy = ProxyServer::new(
        file.path().to_str().unwrap(),
        "https://example.test",
        "https://api.anthropic.test",
        None,
    )
    .await
    .unwrap();
    assert_eq!(proxy.auth_data.access_token.as_deref(), Some(jwt));
    assert_eq!(proxy.auth_data.account_id.as_deref(), Some("acc_openclaw"));
}

#[tokio::test]
async fn parses_openclaw_auth_profiles_without_last_good() {
    let file = NamedTempFile::new().unwrap();
    fs::write(
        file.path(),
        r#"{
            "version": 1,
            "profiles": {
                "vercel-ai-gateway:default": {
                    "type": "api_key",
                    "provider": "vercel-ai-gateway",
                    "key": "not-selected"
                },
                "openai-codex:user@example.com": {
                    "type": "oauth",
                    "provider": "openai-codex",
                    "access": "profile-token",
                    "accountId": "acc_profile"
                }
            }
        }"#,
    )
    .unwrap();

    let proxy = ProxyServer::new(
        file.path().to_str().unwrap(),
        "https://example.test",
        "https://api.anthropic.test",
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        proxy.auth_data.access_token.as_deref(),
        Some("profile-token")
    );
    assert_eq!(proxy.auth_data.account_id.as_deref(), Some("acc_profile"));
}

#[tokio::test]
async fn rejects_file_without_usable_credentials() {
    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), r#"{"tokens": {}}"#).unwrap();

    let result = ProxyServer::new(
        file.path().to_str().unwrap(),
        "https://example.test",
        "https://api.anthropic.test",
        None,
    )
    .await;
    assert!(result.is_err());
    let err = result.err().unwrap();
    let msg = err.to_string();
    assert!(
        msg.contains("parse")
            || msg.contains("supported auth file format")
            || msg.contains("usable")
    );
}

#[test]
fn converts_anthropic_messages_to_internal_chat_shape() {
    let proxy = ProxyServer {
        client: crate::build_http_client().unwrap(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
        anthropic_upstream_base_url: "https://api.anthropic.test".to_string(),
        anthropic_auth_token: None,
        claude_oauth_refresh_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
    };

    let req = AnthropicMessagesRequest {
        model: "claude-test".to_string(),
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: serde_json::json!("hello"),
        }],
        system: Some(serde_json::json!("be concise")),
        max_tokens: Some(128),
        stream: Some(false),
        tools: None,
    };

    let converted = proxy.convert_anthropic_to_chat(req).unwrap();
    assert_eq!(converted.model, "claude-test");
    assert_eq!(converted.messages.len(), 2);
    assert_eq!(converted.messages[0].role, "system");
    assert_eq!(converted.messages[1].role, "user");
}

#[test]
fn converts_anthropic_messages_without_system_prompt() {
    let proxy = ProxyServer {
        client: crate::build_http_client().unwrap(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
        anthropic_upstream_base_url: "https://api.anthropic.test".to_string(),
        anthropic_auth_token: None,
        claude_oauth_refresh_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
    };

    let req = AnthropicMessagesRequest {
        model: "claude-test".to_string(),
        messages: vec![
            AnthropicMessage {
                role: "user".to_string(),
                content: serde_json::json!("hello"),
            },
            AnthropicMessage {
                role: "assistant".to_string(),
                content: serde_json::json!("hi"),
            },
        ],
        system: None,
        max_tokens: Some(64),
        stream: Some(false),
        tools: None,
    };

    let converted = proxy.convert_anthropic_to_chat(req).unwrap();
    assert_eq!(converted.messages.len(), 2);
    assert_eq!(converted.messages[0].role, "user");
    assert_eq!(converted.messages[1].role, "assistant");
}

#[test]
fn anthropic_requires_max_tokens() {
    let proxy = ProxyServer {
        client: crate::build_http_client().unwrap(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
        anthropic_upstream_base_url: "https://api.anthropic.test".to_string(),
        anthropic_auth_token: None,
        claude_oauth_refresh_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
    };

    let req = AnthropicMessagesRequest {
        model: "claude-test".to_string(),
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: serde_json::json!("hello"),
        }],
        system: None,
        max_tokens: None,
        stream: Some(false),
        tools: None,
    };

    let err = proxy.convert_anthropic_to_chat(req).unwrap_err();
    match err {
        ProxyError::Validation { field, .. } => {
            assert_eq!(field.as_deref(), Some("max_tokens"));
        }
        other => panic!("unexpected error: {:?}", other),
    }
}

#[test]
fn anthropic_rejects_invalid_role() {
    let proxy = ProxyServer {
        client: crate::build_http_client().unwrap(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
        anthropic_upstream_base_url: "https://api.anthropic.test".to_string(),
        anthropic_auth_token: None,
        claude_oauth_refresh_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
    };

    let req = AnthropicMessagesRequest {
        model: "claude-test".to_string(),
        messages: vec![AnthropicMessage {
            role: "developer".to_string(),
            content: serde_json::json!("hello"),
        }],
        system: None,
        max_tokens: Some(16),
        stream: Some(false),
        tools: None,
    };

    let err = proxy.convert_anthropic_to_chat(req).unwrap_err();
    match err {
        ProxyError::Validation { field, .. } => {
            assert_eq!(field.as_deref(), Some("messages[0].role"));
        }
        other => panic!("unexpected error: {:?}", other),
    }
}

#[test]
fn anthropic_hoists_claude_code_system_messages() {
    let proxy = ProxyServer {
        client: crate::build_http_client().unwrap(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
        anthropic_upstream_base_url: "https://api.anthropic.test".to_string(),
        anthropic_auth_token: None,
        claude_oauth_refresh_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
    };

    let req = AnthropicMessagesRequest {
        model: "gpt-5.6-sol".to_string(),
        messages: vec![
            AnthropicMessage {
                role: "user".to_string(),
                content: serde_json::json!("first"),
            },
            AnthropicMessage {
                role: "system".to_string(),
                content: serde_json::json!("nested system"),
            },
            AnthropicMessage {
                role: "user".to_string(),
                content: serde_json::json!("second"),
            },
        ],
        system: Some(serde_json::json!("top-level system")),
        max_tokens: Some(64),
        stream: Some(false),
        tools: None,
    };

    let chat = proxy.convert_anthropic_to_chat(req).unwrap();
    assert_eq!(chat.messages.len(), 3);
    assert_eq!(chat.messages[0].role, "system");
    assert_eq!(chat.messages[1].role, "user");
    assert_eq!(chat.messages[2].role, "user");
    let system = crate::flatten_message_content(&chat.messages[0].content).unwrap();
    assert!(system.contains("top-level system"));
    assert!(system.contains("nested system"));
}

#[test]
fn anthropic_ingress_preparation_routes_nested_system_directive_to_codex() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();

    let mut req = AnthropicMessagesRequest {
        model: "haiku".to_string(),
        messages: vec![
            AnthropicMessage {
                role: "user".to_string(),
                content: serde_json::json!(
                    "[gateway-model: gpt-5.6]\nRun through the Codex route."
                ),
            },
            AnthropicMessage {
                role: "system".to_string(),
                content: serde_json::json!("nested compatibility system"),
            },
        ],
        system: Some(serde_json::json!("top-level system")),
        max_tokens: Some(64),
        stream: Some(false),
        tools: Some(vec![serde_json::json!({
            "name": "Agent",
            "input_schema": {"type": "object", "properties": {}}
        })]),
    };

    let (directive, route) = prepare_anthropic_ingress(&mut req);

    assert_eq!(directive.as_deref(), Some("gpt-5.6"));
    assert_eq!(route, AnthropicModelRoute::Codex);
    assert_eq!(normalize_codex_model_id(&req.model), "gpt-5.6-sol");
    assert!(req.messages.iter().all(|message| message.role != "system"));
    assert_eq!(
        req.messages[0].content,
        serde_json::json!("Run through the Codex route.")
    );
    let system = flatten_message_content(req.system.as_ref().unwrap()).unwrap();
    assert!(system.contains("top-level system"));
    assert!(system.contains("nested compatibility system"));
    assert!(system.contains("Gateway model routing for Agent"));

    clear_alias_env();
}

#[test]
fn anthropic_ingress_preparation_serializes_nested_system_for_passthrough() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();

    let mut req = AnthropicMessagesRequest {
        model: "opus".to_string(),
        messages: vec![
            AnthropicMessage {
                role: "user".to_string(),
                content: serde_json::json!("first"),
            },
            AnthropicMessage {
                role: "system".to_string(),
                content: serde_json::json!([{
                    "type": "text",
                    "text": "nested passthrough system",
                    "cache_control": {"type": "ephemeral"}
                }]),
            },
            AnthropicMessage {
                role: "assistant".to_string(),
                content: serde_json::json!("second"),
            },
        ],
        system: None,
        max_tokens: Some(64),
        stream: Some(true),
        tools: Some(vec![serde_json::json!({
            "name": "Agent",
            "input_schema": {"type": "object", "properties": {}}
        })]),
    };

    let (directive, route) = prepare_anthropic_ingress(&mut req);

    assert!(directive.is_none());
    assert_eq!(route, AnthropicModelRoute::Anthropic);
    assert!(req.messages.iter().all(|message| message.role != "system"));
    let encoded = serde_json::to_vec(&req).unwrap();
    let normalized = normalize_anthropic_request_model(&encoded).unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&normalized).unwrap();
    assert_eq!(payload["model"], serde_json::json!("claude-opus-5"));
    assert_eq!(payload["messages"].as_array().unwrap().len(), 2);
    let system = flatten_message_content(&payload["system"]).unwrap();
    assert!(system.contains("nested passthrough system"));
    assert!(system.contains("Gateway model routing for Agent"));
    assert_eq!(
        payload["system"][0]["cache_control"]["type"],
        serde_json::json!("ephemeral")
    );

    clear_alias_env();
}

#[test]
fn chat_request_requires_messages() {
    let proxy = ProxyServer {
        client: crate::build_http_client().unwrap(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
        anthropic_upstream_base_url: "https://api.anthropic.test".to_string(),
        anthropic_auth_token: None,
        claude_oauth_refresh_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
    };

    let req = ChatCompletionsRequest {
        model: "gpt-test".to_string(),
        messages: vec![],
        stream: Some(false),
        tools: None,
    };

    let err = proxy.convert_chat_to_responses(req).unwrap_err();
    match err {
        ProxyError::Validation { field, .. } => {
            assert_eq!(field.as_deref(), Some("messages"));
        }
        other => panic!("unexpected error: {:?}", other),
    }
}

#[test]
fn flatten_message_content_supports_text_blocks() {
    let content = serde_json::json!([
        {"type": "text", "text": "hello"},
        {"type": "text", "text": "world"}
    ]);
    let flat = flatten_message_content(&content).unwrap();
    assert_eq!(flat, "hello world");
}

#[test]
fn flatten_message_content_rejects_empty_arrays() {
    let content = serde_json::json!([]);
    let err = flatten_message_content(&content).unwrap_err();
    assert!(err.contains("must contain at least one text item"));
}

#[test]
fn extract_response_content_prefers_deltas() {
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hel\"}\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n",
        "data: [DONE]\n"
    );

    let content = extract_response_content(sse).unwrap();
    assert_eq!(content, "Hello");
}

#[test]
fn extract_response_content_falls_back_to_final_item() {
    let sse = concat!(
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"content\":[{\"text\":\"Fallback\"}]}}\n",
        "data: [DONE]\n"
    );

    let content = extract_response_content(sse).unwrap();
    assert_eq!(content, "Fallback");
}

#[test]
fn extract_response_content_errors_when_empty() {
    let sse = "data: [DONE]\n";
    let err = extract_response_content(sse).unwrap_err();
    match err {
        ProxyError::UpstreamProtocol { message } => {
            assert!(message.contains("no parsable response content"));
        }
        other => panic!("unexpected error: {:?}", other),
    }
}

#[test]
fn convert_chat_to_responses_supports_array_message_content() {
    let proxy = ProxyServer {
        client: crate::build_http_client().unwrap(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
        anthropic_upstream_base_url: "https://api.anthropic.test".to_string(),
        anthropic_auth_token: None,
        claude_oauth_refresh_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
    };

    let req = ChatCompletionsRequest {
        model: "gpt-test".to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: serde_json::json!([
                {"type": "text", "text": "first"},
                {"type": "text", "text": "second"}
            ]),
        }],
        stream: Some(false),
        tools: None,
    };

    let converted = proxy.convert_chat_to_responses(req).unwrap();
    assert_eq!(converted.model, "gpt-test");
    assert_eq!(converted.input.len(), 2);
}

#[test]
fn convert_chat_to_responses_preserves_assistant_history() {
    let proxy = ProxyServer {
        client: crate::build_http_client().unwrap(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
        anthropic_upstream_base_url: "https://api.anthropic.test".to_string(),
        anthropic_auth_token: None,
        claude_oauth_refresh_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
    };
    let req = ChatCompletionsRequest {
        model: "gpt-test".to_string(),
        messages: vec![
            ChatMessage {
                role: "user".to_string(),
                content: serde_json::json!("hello"),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: serde_json::json!("hi there"),
            },
        ],
        stream: Some(false),
        tools: None,
    };
    let converted = proxy.convert_chat_to_responses(req).unwrap();
    let json = serde_json::to_value(&converted).unwrap();
    let input = json.get("input").and_then(|v| v.as_array()).unwrap();
    assert_eq!(input.len(), 1);
    assert_eq!(input[0].get("role").and_then(|v| v.as_str()), Some("user"));
}

#[test]
fn parse_codex_sse_emits_tool_call_events() {
    let sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_123\",\"name\":\"Edit\"}}\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"call_123\",\"delta\":\"{\\\"path\\\":\"}\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_123\",\"name\":\"Edit\",\"arguments\":\"{\\\"path\\\":\\\"x\\\"}\"}}\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n",
        "data: [DONE]\n"
    );
    let events = crate::streaming::parse_codex_sse_to_events(sse).unwrap();
    assert!(events.iter().any(|e| matches!(e, crate::streaming::CanonicalStreamEvent::ToolCallStart { call_id, name } if call_id == "call_123" && name == "Edit")));
    assert!(events.iter().any(|e| matches!(e, crate::streaming::CanonicalStreamEvent::ToolCallDelta { call_id, arguments_delta } if call_id == "call_123" && arguments_delta.contains("path"))));
    assert!(events.iter().any(|e| matches!(e, crate::streaming::CanonicalStreamEvent::ToolCallDone { call_id, name, .. } if call_id == "call_123" && name == "Edit")));
}

#[test]
fn render_anthropic_sse_includes_tool_use_blocks() {
    let events = vec![
        crate::streaming::CanonicalStreamEvent::MessageStart,
        crate::streaming::CanonicalStreamEvent::ToolCallStart {
            call_id: "call_123".to_string(),
            name: "Edit".to_string(),
        },
        crate::streaming::CanonicalStreamEvent::ToolCallDelta {
            call_id: "call_123".to_string(),
            arguments_delta: "{\"path\":\"a\"}".to_string(),
        },
        crate::streaming::CanonicalStreamEvent::ToolCallDone {
            call_id: "call_123".to_string(),
            name: "Edit".to_string(),
            arguments: "{\"path\":\"a\"}".to_string(),
        },
        crate::streaming::CanonicalStreamEvent::Completed {
            finish_reason: Some("tool_use".to_string()),
        },
    ];
    let out = crate::streaming::render_anthropic_sse(&events, "gpt-5.4");
    assert!(out.contains("tool_use"));
    assert!(out.contains("input_json_delta"));
    assert!(out.contains("call_123"));
}

#[test]
fn anthropic_tool_result_blocks_become_function_call_output_items() {
    let proxy = ProxyServer {
        client: crate::build_http_client().unwrap(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
        anthropic_upstream_base_url: "https://api.anthropic.test".to_string(),
        anthropic_auth_token: None,
        claude_oauth_refresh_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
    };
    let req = ChatCompletionsRequest {
        model: "openai/gpt-5.4".to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: serde_json::json!([
                {"type":"tool_result","tool_use_id":"toolu_123","content":"edited file"}
            ]),
        }],
        stream: Some(false),
        tools: None,
    };
    let converted = proxy.convert_chat_to_responses(req).unwrap();
    let json = serde_json::to_value(&converted).unwrap();
    let input = json.get("input").and_then(|v| v.as_array()).unwrap();
    assert_eq!(input.len(), 1);
    assert_eq!(
        input[0].get("type").and_then(|v| v.as_str()),
        Some("function_call_output")
    );
    assert_eq!(
        input[0].get("call_id").and_then(|v| v.as_str()),
        Some("toolu_123")
    );
    assert_eq!(
        input[0].get("output").and_then(|v| v.as_str()),
        Some("edited file")
    );
}

#[test]
fn render_anthropic_sse_sets_tool_use_stop_reason_when_tool_calls_exist() {
    let events = vec![
        crate::streaming::CanonicalStreamEvent::MessageStart,
        crate::streaming::CanonicalStreamEvent::ToolCallStart {
            call_id: "toolu_1".to_string(),
            name: "Edit".to_string(),
        },
        crate::streaming::CanonicalStreamEvent::ToolCallDone {
            call_id: "toolu_1".to_string(),
            name: "Edit".to_string(),
            arguments: "{}".to_string(),
        },
        crate::streaming::CanonicalStreamEvent::Completed {
            finish_reason: Some("tool_use".to_string()),
        },
    ];
    let out = crate::streaming::render_anthropic_sse(&events, "gpt-5.4");
    assert!(out.contains("\"stop_reason\":\"tool_use\""));
    assert!(out.contains("event: message_stop\n"));
    assert!(out.contains("data: {\"type\":\"message_stop\"}\n\n"));
    assert!(out.contains("event: content_block_stop\n"));
}

#[test]
fn render_anthropic_sse_handles_completed_function_call_items() {
    let sse = concat!(
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_xyz\",\"name\":\"Edit\",\"arguments\":\"{\\\"file_path\\\":\\\"note.txt\\\"}\",\"status\":\"completed\"}}\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n",
        "data: [DONE]\n"
    );
    let events = crate::streaming::parse_codex_sse_to_events(sse).unwrap();
    let out = crate::streaming::render_anthropic_sse(&events, "gpt-5.4");
    assert!(out.contains("tool_use"));
    assert!(out.contains("call_xyz"));
    assert!(out.contains("input_json_delta"));
    assert!(out.contains("\"stop_reason\":\"tool_use\""));
}

#[test]
fn anthropic_edit_prompt_stream_must_use_tool_use_contract() {
    let fixtures = vec![
        (
            "completed_function_call_item",
            concat!(
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_edit\",\"name\":\"Edit\",\"arguments\":\"{\\\"file_path\\\":\\\"note.txt\\\",\\\"old_string\\\":\\\"\\\",\\\"new_string\\\":\\\"TOOL_USE_OK\\\"}\",\"status\":\"completed\"}}\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n",
                "data: [DONE]\n"
            )
        ),
        (
            "streaming_function_call_arguments",
            concat!(
                "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_read\",\"name\":\"Read\"}}\n",
                "data: {\"type\":\"response.function_call_arguments.delta\",\"call_id\":\"call_read\",\"delta\":\"{\\\"file_path\\\":\\\"note.txt\\\"\"}\n",
                "data: {\"type\":\"response.function_call_arguments.delta\",\"call_id\":\"call_read\",\"delta\":\",\\\"offset\\\":1}\"}\n",
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_read\",\"name\":\"Read\",\"status\":\"completed\"}}\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n",
                "data: [DONE]\n"
            )
        ),
        (
            "bash_tool_call",
            concat!(
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_bash\",\"name\":\"Bash\",\"arguments\":\"{\\\"command\\\":\\\"pwd\\\"}\",\"status\":\"completed\"}}\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n",
                "data: [DONE]\n"
            )
        ),
        (
            "write_tool_call",
            concat!(
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_write\",\"name\":\"Write\",\"arguments\":\"{\\\"file_path\\\":\\\"note.txt\\\",\\\"content\\\":\\\"hello\\\"}\",\"status\":\"completed\"}}\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n",
                "data: [DONE]\n"
            )
        ),
        (
            "edit_multidelta_json",
            concat!(
                "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_edit2\",\"name\":\"Edit\"}}\n",
                "data: {\"type\":\"response.function_call_arguments.delta\",\"call_id\":\"call_edit2\",\"delta\":\"{\\\"file_path\\\":\\\"note.txt\\\",\"}\n",
                "data: {\"type\":\"response.function_call_arguments.delta\",\"call_id\":\"call_edit2\",\"delta\":\"\\\"old_string\\\":\\\"before\\\",\"}\n",
                "data: {\"type\":\"response.function_call_arguments.delta\",\"call_id\":\"call_edit2\",\"delta\":\"\\\"new_string\\\":\\\"after\\\"}\"}\n",
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_edit2\",\"name\":\"Edit\",\"status\":\"completed\"}}\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n",
                "data: [DONE]\n"
            )
        ),
        (
            "mixed_text_then_tool_use",
            concat!(
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Let me check that.\"}\n",
                "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_read2\",\"name\":\"Read\"}}\n",
                "data: {\"type\":\"response.function_call_arguments.delta\",\"call_id\":\"call_read2\",\"delta\":\"{\\\"file_path\\\":\\\"note.txt\\\"}\"}\n",
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_read2\",\"name\":\"Read\",\"status\":\"completed\"}}\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n",
                "data: [DONE]\n"
            )
        ),
    ];

    for (name, sse) in fixtures {
        let events = crate::streaming::parse_codex_sse_to_events(sse).unwrap();
        let out = crate::streaming::render_anthropic_sse(&events, "gpt-5.4");
        assert!(
            out.contains("tool_use"),
            "fixture {name} must render Anthropic tool_use block"
        );
        assert!(
            out.contains("\"stop_reason\":\"tool_use\""),
            "fixture {name} must stop with tool_use"
        );
        assert!(
            out.contains("event: message_stop\n"),
            "fixture {name} must include message_stop"
        );
        assert!(
            out.contains("event: content_block_start\n"),
            "fixture {name} must include content_block_start"
        );
        assert!(
            out.contains("event: content_block_stop\n"),
            "fixture {name} must include content_block_stop"
        );
    }
}

#[test]
fn anthropic_assistant_tool_use_blocks_become_codex_function_call_items() {
    let proxy = ProxyServer {
        client: crate::build_http_client().unwrap(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
        anthropic_upstream_base_url: "https://api.anthropic.test".to_string(),
        anthropic_auth_token: None,
        claude_oauth_refresh_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
    };
    let req = ChatCompletionsRequest {
        model: "openai/gpt-5.4".to_string(),
        messages: vec![ChatMessage {
            role: "assistant".to_string(),
            content: serde_json::json!([
                {"type":"tool_use","id":"toolu_123","name":"Edit","input":{"file_path":"note.txt","old_string":"","new_string":"TOOL_USE_OK"}}
            ]),
        }],
        stream: Some(false),
        tools: None,
    };
    let converted = proxy.convert_chat_to_responses(req).unwrap();
    let json = serde_json::to_value(&converted).unwrap();
    let input = json.get("input").and_then(|v| v.as_array()).unwrap();
    assert_eq!(
        input[0].get("type").and_then(|v| v.as_str()),
        Some("function_call")
    );
    assert_eq!(
        input[0].get("call_id").and_then(|v| v.as_str()),
        Some("toolu_123")
    );
    assert_eq!(input[0].get("name").and_then(|v| v.as_str()), Some("Edit"));
}

#[test]
fn normalize_tools_for_codex_sets_strict_and_additional_properties_false() {
    let tools = vec![serde_json::json!({
        "type": "function",
        "function": {
            "name": "Edit",
            "description": "Edit a file",
            "parameters": {
                "type": "object",
                "properties": {
                    "file_path": {"type": "string"},
                    "old_string": {"type": "string"},
                    "new_string": {"type": "string"}
                },
                "required": ["file_path", "old_string", "new_string"]
            }
        }
    })];
    let normalized = crate::normalize_tools_for_codex(tools);
    let tool = &normalized[0];
    assert_eq!(tool["strict"].as_bool(), Some(false));
    assert_eq!(
        tool["parameters"]["required"],
        serde_json::json!(["file_path", "old_string", "new_string"])
    );
    assert_eq!(tool["name"].as_str(), Some("Edit"));
}

#[test]
fn normalize_tool_parameters_schema_preserves_optional_parameters() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "command": {"type": "string"},
            "isolation": {
                "type": "object",
                "properties": {
                    "mode": {"type": "string"},
                    "network": {"type": "boolean"}
                }
            }
        },
        "required": ["command"]
    });
    let normalized = crate::normalize_tool_parameters_schema(schema.clone());
    assert_eq!(normalized, schema);
    assert_eq!(normalized["required"], serde_json::json!(["command"]));
    assert!(normalized["properties"]["isolation"]
        .get("required")
        .is_none());
}

#[test]
fn normalize_openai_responses_input_wraps_string_as_user_input_text() {
    let normalized = normalize_openai_responses_input(Some(serde_json::json!("hello"))).unwrap();
    assert_eq!(
        normalized,
        serde_json::json!([
            {"role":"user","content":[{"type":"input_text","text":"hello"}]}
        ])
    );
}

#[test]
fn normalize_openai_responses_input_requires_value() {
    let err = normalize_openai_responses_input(None).unwrap_err();
    match err {
        ProxyError::Validation { field, .. } => assert_eq!(field.as_deref(), Some("input")),
        other => panic!("unexpected error: {:?}", other),
    }
}

#[test]
fn convert_openai_responses_request_normalizes_model_and_tools() {
    let proxy = ProxyServer {
        client: crate::build_http_client().unwrap(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
        anthropic_upstream_base_url: "https://api.anthropic.test".to_string(),
        anthropic_auth_token: None,
        claude_oauth_refresh_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
    };

    let req = OpenAiResponsesRequest {
        model: "openai/gpt-5.4".to_string(),
        input: Some(serde_json::json!("Hello")),
        instructions: None,
        tools: Some(vec![serde_json::json!({
            "type":"function",
            "function": {
                "name": "Edit",
                "description": "Edit a file",
                "parameters": {"type":"object","properties":{"file_path":{"type":"string"}}}
            }
        })]),
        tool_choice: None,
        parallel_tool_calls: None,
        reasoning: None,
        store: None,
        stream: Some(false),
        text: None,
        include: None,
        prompt_cache_key: None,
        previous_response_id: Some("resp_prev".to_string()),
        metadata: Some(serde_json::json!({"source":"test"})),
        truncation: None,
        temperature: None,
        top_p: None,
        user: Some("user-123".to_string()),
    };

    let (model, payload) = proxy.convert_openai_responses_request(req).unwrap();
    assert_eq!(model, "gpt-5.4");
    assert_eq!(payload["model"], serde_json::json!("gpt-5.4"));
    assert_eq!(
        payload["previous_response_id"],
        serde_json::json!("resp_prev")
    );
    assert_eq!(payload["metadata"]["source"], serde_json::json!("test"));
    assert_eq!(payload["user"], serde_json::json!("user-123"));
    assert_eq!(payload["tools"][0]["name"], serde_json::json!("Edit"));
}

#[test]
fn render_openai_responses_json_includes_text_tool_calls_and_usage() {
    let events = vec![
        crate::streaming::CanonicalStreamEvent::MessageStart,
        crate::streaming::CanonicalStreamEvent::TextDelta {
            text: "Hello".to_string(),
        },
        crate::streaming::CanonicalStreamEvent::ToolCallDone {
            call_id: "call_1".to_string(),
            name: "Edit".to_string(),
            arguments: "{\"file_path\":\"note.txt\"}".to_string(),
        },
        crate::streaming::CanonicalStreamEvent::Usage {
            input_tokens: 3,
            output_tokens: 5,
        },
        crate::streaming::CanonicalStreamEvent::Completed {
            finish_reason: Some("tool_use".to_string()),
        },
    ];

    let rendered = render_openai_responses_json(&events, "gpt-5.4");
    assert_eq!(rendered["object"], serde_json::json!("response"));
    assert_eq!(rendered["model"], serde_json::json!("gpt-5.4"));
    assert_eq!(rendered["output_text"], serde_json::json!("Hello"));
    assert_eq!(rendered["usage"]["total_tokens"], serde_json::json!(8));
    assert!(rendered["output"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["type"] == "message"));
    assert!(rendered["output"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["type"] == "function_call" && item["name"] == "Edit"));
}

#[test]
fn rotating_log_retains_only_the_configured_archives() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("gateway.log");
    let mut log = RotatingFile::open(path.clone(), 8, 2).unwrap();

    for line in ["01\n", "02\n", "03\n", "04\n", "05\n", "06\n", "07\n"] {
        log.write_line(line.as_bytes()).unwrap();
    }

    assert_eq!(fs::read_to_string(&path).unwrap(), "07\n");
    assert_eq!(
        fs::read_to_string(directory.path().join("gateway.log.1")).unwrap(),
        "05\n06\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("gateway.log.2")).unwrap(),
        "03\n04\n"
    );
    assert!(!directory.path().join("gateway.log.3").exists());
}

#[test]
fn rotating_log_can_truncate_without_retaining_archives() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("gateway.log");
    let mut log = RotatingFile::open(path.clone(), 4, 0).unwrap();

    log.write_line(b"one\n").unwrap();
    log.write_line(b"two\n").unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), "two\n");
    assert!(!directory.path().join("gateway.log.1").exists());
}
