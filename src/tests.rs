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

#[test]
fn p4_r1_accepts_only_exact_retain_all_context_noop() {
    let base = serde_json::json!({"model":"gpt-6.2-sol","max_tokens":64,
        "output_config":{"effort":"low"},"messages":[{"role":"user","content":"Read fixture"}],
        "context_management":{"edits":[{"type":"clear_thinking_20251015","keep":"all"}]}});
    let actual = crate::prepare_codex_anthropic_body(&serde_json::to_vec(&base).unwrap());
    assert!(
        actual.is_ok(),
        "P4-R1 retain-all no-op rejected: {actual:?}"
    );
    for context in [
        serde_json::json!({"edits":[{"type":"clear_thinking_20251015","keep":"all","extra":true}]}),
        serde_json::json!({"edits":[{"type":"clear_thinking_20251015","keep":"none"}]}),
        serde_json::json!({"edits":[{"type":"clear_thinking_20251015","keep":"all"},{"type":"clear_tool_uses_20250919"}]}),
        serde_json::json!({"edits":[{"type":"clear_thinking_20251015","keep":"all"}],"extra":true}),
        serde_json::json!({"edits":[]}),
    ] {
        let mut request = base.clone();
        request["context_management"] = context;
        assert!(
            crate::prepare_codex_anthropic_body(&serde_json::to_vec(&request).unwrap()).is_err()
        );
    }
}

#[test]
fn p4_r2_discovery_authorization_is_internal_to_anthropic() {
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
    let tool = serde_json::json!({"name":"ToolSearch","input_schema":{"type":"object"},
        "_codex_client_discovery":true});
    let chat: ChatCompletionsRequest = serde_json::from_value(serde_json::json!({
        "model":"gpt-6.2-sol","messages":[{"role":"user","content":"hello"}],
        "tools":[tool.clone(),{"type":"function","function":{"name":"ToolSearch","parameters":{"type":"object"}},"_codex_client_discovery":true}],"parallel_tool_calls":false,
        "anthropic_controls":{"parallel_tool_calls":false}}))
    .unwrap();
    let chat = proxy.convert_chat_to_responses(chat).unwrap();
    let responses: OpenAiResponsesRequest = serde_json::from_value(serde_json::json!({
        "model":"gpt-6.2-sol","input":"hello","tools":[{"type":"function","name":"ToolSearch","parameters":{"type":"object"},"_codex_client_discovery":true}],"parallel_tool_calls":false}))
    .unwrap();
    let (_, responses) = proxy.convert_openai_responses_request(responses).unwrap();
    assert_eq!(
        (
            chat.tools.len(),
            chat.parallel_tool_calls,
            responses["tools"].as_array().unwrap().len(),
            responses["parallel_tool_calls"].as_bool()
        ),
        (0, true, 1, Some(false)),
        "P4-R2 raw OpenAI compatibility changed"
    );
    let raw = serde_json::json!({"model":"gpt-6.2-sol","max_tokens":64,
        "messages":[{"role":"user","content":"hello"}],"tools":[tool],
        "tool_choice":{"type":"auto","disable_parallel_tool_use":true}});
    let prepared = crate::prepare_codex_anthropic_body(&serde_json::to_vec(&raw).unwrap()).unwrap();
    let request: AnthropicMessagesRequest = serde_json::from_slice(&prepared).unwrap();
    let chat = proxy.convert_anthropic_to_chat(request).unwrap();
    let actual = proxy.convert_chat_to_responses(chat).unwrap();
    assert_eq!(actual.tools[0]["name"], "ToolSearch");
    assert!(!actual.parallel_tool_calls);
}

#[test]
fn codex_controls_preserve_effective_effort_and_reject_scope() {
    for effort in ["low", "medium", "high", "xhigh"] {
        let request = serde_json::json!({"model":"gpt-6.2-sol","max_tokens":64,
            "output_config":{"effort":"low"},"messages":[{"role":"user","content":"hello"},
              {"role":"system","content":[],"output_config":{"effort":effort}}]});
        let normalized =
            crate::prepare_codex_anthropic_body(&serde_json::to_vec(&request).unwrap()).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&normalized).unwrap();
        assert_eq!(value["output_config"]["effort"], effort);
        assert_eq!(value["messages"][1]["content"], "");
    }
    for extension in [
        serde_json::json!({"output_config":{"effort":"max"}}),
        serde_json::json!({"output_config":{"effort":false}}),
        serde_json::json!({"output_config":{"format":{"type":"json_schema"}}}),
        serde_json::json!({"context_management":{"edits":[]}}),
        serde_json::json!({"tool_choice":{"type":"auto","disable_parallel_tool_use":"false"}}),
    ] {
        let mut request = serde_json::json!({"model":"gpt-6.2-sol","messages":[{"role":"user","content":"hello"}]});
        request
            .as_object_mut()
            .unwrap()
            .extend(extension.as_object().unwrap().clone());
        assert!(
            crate::prepare_codex_anthropic_body(&serde_json::to_vec(&request).unwrap()).is_err()
        );
    }
    for control in [
        serde_json::json!({"role":"system","content":[],"clear_at":"next_user_message"}),
        serde_json::json!({"role":"system","content":"scoped"}),
    ] {
        let request = serde_json::json!({"messages":[{"role":"user","content":"hello"},control]});
        assert!(
            crate::prepare_codex_anthropic_body(&serde_json::to_vec(&request).unwrap()).is_err()
        );
    }
}

#[test]
fn codex_discovery_requires_declared_supported_correlated_reference() {
    let base = serde_json::json!({"tools":[{"name":"ToolSearch","input_schema":{"type":"object"}},
        {"name":"Read","defer_loading":true,"input_schema":{"type":"object","required":["file_path"]}}],
        "messages":[{"role":"user","content":"hello"},
          {"role":"assistant","content":[{"type":"tool_use","id":"search","name":"ToolSearch","input":{"query":"select:Read"}}]},
          {"role":"user","content":[{"type":"tool_result","tool_use_id":"search","content":[
            {"type":"text","text":"before"},{"type":"tool_reference","tool_name":"Read"},{"type":"text","text":"after"}]}]}]});
    let bytes = crate::prepare_codex_anthropic_body(&serde_json::to_vec(&base).unwrap()).unwrap();
    let actual: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(actual["tools"][1]["name"], "Read");
    let parts = &actual["messages"][2]["content"][0]["content"];
    assert_eq!(parts[0]["text"], "before");
    assert!(parts[1]["text"].as_str().unwrap().contains("parameters"));
    assert_eq!(parts[2]["text"], "after");
    for (field, value) in [
        ("tool_name", "NotebookEdit"),
        ("tool_name", "Write"),
        ("tool_use_id", "other"),
    ] {
        let mut request = base.clone();
        if field == "tool_name" {
            request["messages"][2]["content"][0]["content"][1][field] = serde_json::json!(value);
        } else {
            request["messages"][2]["content"][0][field] = serde_json::json!(value);
        }
        assert!(
            crate::prepare_codex_anthropic_body(&serde_json::to_vec(&request).unwrap()).is_err()
        );
    }
}

#[test]
fn direct_anthropic_preserves_scoped_controls_and_extensions() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();
    let body = serde_json::json!({
        "model":"haiku", "max_tokens":32,
        "system":[{"type":"text","text":"x-anthropic-billing-header: current;","cache_control":{"type":"ephemeral"}}],
        "output_config":{"effort":"low"},
        "messages":[{"role":"user","content":"first","future_field":{"preserve":true}},
          {"role":"system","content":[],"output_config":{"effort":"high"}},
          {"role":"system","content":[{"type":"text","text":"scoped"}],"clear_at":"next_user_message"},
          {"role":"user","content":"next"}],
        "future_top_level":[1,2,3]
    });
    let (encoded, route) =
        crate::prepare_anthropic_passthrough(&serde_json::to_vec(&body).unwrap(), true).unwrap();
    assert_eq!(route, AnthropicModelRoute::Anthropic);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&encoded).unwrap(),
        body
    );
    let oauth = apply_claude_code_oauth_system_prompt(&encoded).unwrap();
    assert_eq!(oauth, encoded);
    clear_alias_env();
}

#[test]
fn direct_anthropic_keeps_agent_and_directive_behavior_without_hoisting() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();
    let body = serde_json::json!({"model":"gpt-6.2-sol", "tools":[{"name":"Agent"}],
        "messages":[{"role":"user","content":"[gateway-model: haiku]\nkeep"},
                    {"role":"system","content":"scoped","clear_at":"next_user_message"}]});
    let (encoded, route) =
        crate::prepare_anthropic_passthrough(&serde_json::to_vec(&body).unwrap(), true).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(route, AnthropicModelRoute::Anthropic);
    assert_eq!(value["model"], "haiku");
    assert_eq!(value["messages"][0]["content"], "keep");
    assert_eq!(value["messages"][1], body["messages"][1]);
    assert!(value["system"]
        .as_str()
        .unwrap()
        .contains("Gateway model routing for Agent"));
    let (count, _) =
        crate::prepare_anthropic_passthrough(&serde_json::to_vec(&body).unwrap(), false).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&count).unwrap(),
        body
    );
    clear_alias_env();
}

#[test]
fn anthropic_headers_allow_metadata_but_exclude_connection_nominations() {
    let mut upstream = warp::http::HeaderMap::new();
    for (name, value) in [
        ("content-type", "text/event-stream"),
        ("retry-after", "9"),
        ("retry-after-ms", "125"),
        ("x-should-retry", "false"),
        ("request-id", "req_test"),
        ("anthropic-ratelimit-unified-status", "allowed"),
        (
            "connection",
            "close, Request-ID, anthropic-ratelimit-unified-hidden",
        ),
        ("anthropic-ratelimit-unified-hidden", "private"),
        ("set-cookie", "private"),
        ("transfer-encoding", "chunked"),
        ("x-private", "private"),
    ] {
        upstream.insert(
            warp::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            value.parse().unwrap(),
        );
    }
    let headers = crate::anthropic_response_headers(&upstream);
    assert_eq!(headers.len(), 5);
    assert_eq!(headers["retry-after-ms"], "125");
    assert_eq!(headers["retry-after"], "9");
    assert_eq!(headers["x-should-retry"], "false");
    assert_eq!(headers["content-type"], "text/event-stream");
    assert_eq!(headers["anthropic-ratelimit-unified-status"], "allowed");
}
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

    assert_eq!(normalize_anthropic_model_id("sonnet"), "claude-sonnet-5-5");
    assert_eq!(normalize_anthropic_model_id("opus"), "claude-opus-5-5");
    assert_eq!(normalize_anthropic_model_id("haiku"), "claude-haiku-5-5");
    assert_eq!(
        normalize_anthropic_model_id("claude-haiku-4-5"),
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
        extra: Default::default(),
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
        extra: Default::default(),
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
        extra: Default::default(),
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
        extra: Default::default(),
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

    assert_eq!(payload["model"], serde_json::json!("claude-sonnet-5-5"));
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
    assert!(ids.contains(&"claude-opus-5-5"));
    assert!(ids.contains(&"claude-sonnet-5"));
    assert!(ids.contains(&"claude-sonnet-5-5"));
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
                        "provider": "openai-codex",
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
    assert_eq!(msg, "Codex auth: access token required");
}

#[test]
fn c4_credentials_reject_invalid_sources_without_disclosing_values() {
    use serde_json::{json, Value};
    let native = |token: &str, account: Value| json!({"auth_mode":"chatgpt","tokens":{"access_token":token,"account_id":account}});
    let profile = json!({"type":"oauth","provider":"openai-codex","access":"synthetic-access","accountId":"account"});
    let inputs = [
        json!({"OPENAI_API_KEY":"sk-synthetic-secret"}),
        native("sk-synthetic-secret", json!("account")),
        native("synthetic-secret\n", json!("account")),
        native("synthetic-secret", json!("")),
        native("synthetic-secret", json!(null)),
        native("a.invalid!.c", json!("account")),
        json!({"auth_mode":"siwc","tokens":{"access_token":"synthetic-secret","account_id":"account"}}),
        json!({"profiles":{"one":profile.clone(),"two":profile.clone()}}),
        json!({"profiles":{"one":profile.clone()},"lastGood":{"openai-codex":"missing"}}),
        json!({"profiles":{"one":profile},"lastGood":null}),
    ];
    for value in inputs {
        let error = ProxyServer::parse_auth_data(&value.to_string()).unwrap_err();
        let diagnostic = format!("{error:?}");
        assert!(diagnostic.contains("Codex auth:"));
        assert!(!diagnostic.contains("synthetic"));
    }
    let error = ProxyServer::parse_auth_data(r#"{"synthetic-secret": "#).unwrap_err();
    assert_eq!(error.to_string(), "Codex auth: invalid JSON");
}

#[test]
fn c4_jwt_account_and_expiry_are_consistency_checks() {
    use base64::Engine;
    use serde_json::{json, Value};
    let token = |claims: Value| {
        format!(
            "e30.{}.synthetic",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string())
        )
    };
    let auth = |token: String, account: Value| {
        json!({"tokens":{"access_token":token,"account_id":account}}).to_string()
    };
    let claims =
        json!({"https://api.openai.com/auth":{"chatgpt_account_id":"account"},"exp":4102444800u64});
    let parsed = ProxyServer::parse_auth_data(&auth(token(claims.clone()), Value::Null)).unwrap();
    assert_eq!(parsed.account_id.as_deref(), Some("account"));
    assert_eq!(format!("{parsed:?}"), "AuthData [redacted]");
    assert!(ProxyServer::parse_auth_data(&auth(token(claims), json!("different"))).is_err());
    assert!(
        ProxyServer::parse_auth_data(&auth(token(json!({"exp":1})), json!("account"))).is_err()
    );
    assert!(
        ProxyServer::parse_auth_data(&auth(token(json!({"exp":"invalid"})), json!("account")))
            .is_err()
    );
}

#[test]
fn c4_owner_precedence_and_incompatible_legacy_flows() {
    use serde_json::{json, Value};
    let tokens = json!({"access_token":"synthetic.opaque","account_id":"account"});
    for mode in [None, Some(Value::Null)] {
        let mut native = json!({"tokens":tokens});
        if let Some(mode) = mode {
            native["auth_mode"] = mode;
        }
        assert!(ProxyServer::parse_auth_data(&native.to_string()).is_ok());
        for field in [
            "OPENAI_API_KEY",
            "personal_access_token",
            "bedrock_api_key",
            "bedrock_access_keys",
        ] {
            let mut mixed = native.clone();
            mixed[field] = json!("synthetic-alternative");
            assert!(ProxyServer::parse_auth_data(&mixed.to_string()).is_err());
            mixed["auth_mode"] = json!("chatgpt");
            assert!(ProxyServer::parse_auth_data(&mixed.to_string()).is_ok());
            mixed["auth_mode"] = Value::Null;
            mixed[field] = Value::Null;
            assert!(ProxyServer::parse_auth_data(&mixed.to_string()).is_ok());
        }
    }
    for flow in ["chatgpt-token-sharing", "chatgpt-identity"] {
        let profile = json!({"type":"oauth","provider":"openai-codex","access":"synthetic.opaque","accountId":"account"});
        let mut input = json!({"profiles":{"selected":profile.clone(),"fallback":profile},"lastGood":{"openai-codex":"selected"}});
        input["profiles"]["selected"]["authFlow"] = json!(flow);
        let error = ProxyServer::parse_auth_data(&input.to_string()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Codex auth: unsupported selected profile"
        );
        input.as_object_mut().unwrap().remove("lastGood");
        assert!(ProxyServer::parse_auth_data(&input.to_string()).is_ok());
    }
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
        extra: Default::default(),
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
        extra: Default::default(),
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
        extra: Default::default(),
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
        extra: Default::default(),
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
        extra: Default::default(),
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
        extra: Default::default(),
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
        extra: Default::default(),
    };

    let (directive, route) = prepare_anthropic_ingress(&mut req);

    assert!(directive.is_none());
    assert_eq!(route, AnthropicModelRoute::Anthropic);
    assert!(req.messages.iter().all(|message| message.role != "system"));
    let encoded = serde_json::to_vec(&req).unwrap();
    let normalized = normalize_anthropic_request_model(&encoded).unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&normalized).unwrap();
    assert_eq!(payload["model"], serde_json::json!("claude-opus-5-5"));
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
        reasoning_effort: None,
        anthropic_controls: None,
        tool_choice: None,
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

            tool_calls: None,
            tool_call_id: None,
        }],
        stream: Some(false),
        tools: None,
        reasoning_effort: None,
        anthropic_controls: None,
        tool_choice: None,
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
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: serde_json::json!("hi there"),
                tool_calls: None,
                tool_call_id: None,
            },
        ],
        stream: Some(false),
        tools: None,
        reasoning_effort: None,
        anthropic_controls: None,
        tool_choice: None,
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
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_123\",\"name\":\"Edit\"}}\n\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"call_123\",\"delta\":\"{\\\"path\\\":\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_123\",\"name\":\"Edit\",\"arguments\":\"{\\\"path\\\":\\\"x\\\"}\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n",
        "data: [DONE]\n\n"
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

            tool_calls: None,
            tool_call_id: None,
        }],
        stream: Some(false),
        tools: None,
        reasoning_effort: None,
        anthropic_controls: None,
        tool_choice: None,
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
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_xyz\",\"name\":\"Edit\",\"arguments\":\"{\\\"file_path\\\":\\\"note.txt\\\"}\",\"status\":\"completed\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n",
        "data: [DONE]\n\n"
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
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_edit\",\"name\":\"Edit\",\"arguments\":\"{\\\"file_path\\\":\\\"note.txt\\\",\\\"old_string\\\":\\\"\\\",\\\"new_string\\\":\\\"TOOL_USE_OK\\\"}\",\"status\":\"completed\"}}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n",
                "data: [DONE]\n\n"
            )
        ),
        (
            "streaming_function_call_arguments",
            concat!(
                "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_read\",\"name\":\"Read\"}}\n\n",
                "data: {\"type\":\"response.function_call_arguments.delta\",\"call_id\":\"call_read\",\"delta\":\"{\\\"file_path\\\":\\\"note.txt\\\"\"}\n\n",
                "data: {\"type\":\"response.function_call_arguments.delta\",\"call_id\":\"call_read\",\"delta\":\",\\\"offset\\\":1}\"}\n\n",
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_read\",\"name\":\"Read\",\"status\":\"completed\"}}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n",
                "data: [DONE]\n\n"
            )
        ),
        (
            "bash_tool_call",
            concat!(
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_bash\",\"name\":\"Bash\",\"arguments\":\"{\\\"command\\\":\\\"pwd\\\"}\",\"status\":\"completed\"}}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n",
                "data: [DONE]\n\n"
            )
        ),
        (
            "write_tool_call",
            concat!(
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_write\",\"name\":\"Write\",\"arguments\":\"{\\\"file_path\\\":\\\"note.txt\\\",\\\"content\\\":\\\"hello\\\"}\",\"status\":\"completed\"}}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n",
                "data: [DONE]\n\n"
            )
        ),
        (
            "edit_multidelta_json",
            concat!(
                "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_edit2\",\"name\":\"Edit\"}}\n\n",
                "data: {\"type\":\"response.function_call_arguments.delta\",\"call_id\":\"call_edit2\",\"delta\":\"{\\\"file_path\\\":\\\"note.txt\\\",\"}\n\n",
                "data: {\"type\":\"response.function_call_arguments.delta\",\"call_id\":\"call_edit2\",\"delta\":\"\\\"old_string\\\":\\\"before\\\",\"}\n\n",
                "data: {\"type\":\"response.function_call_arguments.delta\",\"call_id\":\"call_edit2\",\"delta\":\"\\\"new_string\\\":\\\"after\\\"}\"}\n\n",
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_edit2\",\"name\":\"Edit\",\"status\":\"completed\"}}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n",
                "data: [DONE]\n\n"
            )
        ),
        (
            "mixed_text_then_tool_use",
            concat!(
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Let me check that.\"}\n\n",
                "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_read2\",\"name\":\"Read\"}}\n\n",
                "data: {\"type\":\"response.function_call_arguments.delta\",\"call_id\":\"call_read2\",\"delta\":\"{\\\"file_path\\\":\\\"note.txt\\\"}\"}\n\n",
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_read2\",\"name\":\"Read\",\"status\":\"completed\"}}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n",
                "data: [DONE]\n\n"
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

            tool_calls: None,
            tool_call_id: None,
        }],
        stream: Some(false),
        tools: None,
        reasoning_effort: None,
        anthropic_controls: None,
        tool_choice: None,
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
    let normalized = crate::normalize_tools_for_codex(tools, crate::CodexToolSource::OpenAi);
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
fn responses_validation_preserves_null_summary_and_rejects_field_presence() {
    for field in [
        "previous_response_id",
        "metadata",
        "truncation",
        "top_p",
        "user",
    ] {
        let mut request = serde_json::json!({"model":"gpt-5.4","input":"hello"});
        request[field] = serde_json::Value::Null;
        let error =
            crate::parse_responses_request(&serde_json::to_vec(&request).unwrap()).unwrap_err();
        assert!(
            matches!(error, ProxyError::Validation { field: Some(actual), .. } if actual == field)
        );
    }
    let req = crate::parse_responses_request(
        br#"{"model":"gpt-5.4","input":"hi","reasoning":{"effort":"low","summary":null}}"#,
    )
    .unwrap();
    assert_eq!(
        req.reasoning.unwrap(),
        serde_json::json!({"effort":"low","summary":null})
    );
}

#[test]
fn responses_url_and_temperature_policy_are_bounded() {
    for suffix in ["", "/codex", "/codex/responses", "/codex/responses///"] {
        assert_eq!(
            crate::codex_responses_url(&format!("http://127.0.0.1:1/tenant/backend-api{suffix}")),
            "http://127.0.0.1:1/tenant/backend-api/codex/responses"
        );
    }
    for model in [
        "openai/gpt-5.6",
        "gpt-5.6-terra",
        "gpt-6.2-sol",
        "gpt-daybreak-red-latest",
    ] {
        assert!(crate::responses_temperature_denied(
            &normalize_codex_model_id(model)
        ));
    }
    for model in ["future-model", "gpt-6.99-unknown", "gpt-5.4"] {
        assert!(!crate::responses_temperature_denied(model));
    }
}

#[test]
fn c3_r1_nullable_controls_and_reasoning_shapes_remain_compatible() {
    let optional = [
        "instructions",
        "prompt_cache_key",
        "service_tier",
        "store",
        "stream",
        "parallel_tool_calls",
        "temperature",
        "tools",
        "include",
        "reasoning",
        "text",
        "tool_choice",
    ];
    for field in optional {
        let mut request = serde_json::json!({"model":"gpt-5.4","input":"fixture"});
        request[field] = serde_json::Value::Null;
        crate::parse_responses_request(&serde_json::to_vec(&request).unwrap()).unwrap();
        request[field] = serde_json::json!(42);
        assert!(
            crate::parse_responses_request(&serde_json::to_vec(&request).unwrap()).is_err()
                || matches!(field, "temperature" | "tool_choice")
        );
    }
    for summary in ["off", "on", "auto"] {
        let reasoning = serde_json::json!({"effort":null,"context":"all_turns","mode":"standard","summary":summary});
        let request =
            serde_json::json!({"model":"gpt-5.4","input":"fixture","reasoning":reasoning});
        let parsed =
            crate::parse_responses_request(&serde_json::to_vec(&request).unwrap()).unwrap();
        assert_eq!(parsed.reasoning.unwrap(), reasoning);
    }
    assert!(crate::parse_responses_request(br#"{"model":null,"input":"fixture"}"#).is_err());
    assert!(crate::normalize_responses_choice(Some(serde_json::json!(42)), &[]).is_err());
}

#[test]
fn c3_r2_preserves_native_nulls_and_nested_empty_schema_compatibility() {
    let flat =
        serde_json::json!({"type":"function","name":"Read","parameters":null,"description":null});
    let tools = crate::normalize_responses_tools(vec![flat.clone()]).unwrap();
    assert!(tools[0]["parameters"].is_null());
    assert!(tools[0]["description"].is_null());
    for declaration in [
        serde_json::json!({"name":"Read"}),
        serde_json::json!({"name":"Read","parameters":null}),
    ] {
        let tools = crate::normalize_responses_tools(vec![
            serde_json::json!({"type":"function","function":declaration}),
        ])
        .unwrap();
        assert_eq!(
            tools[0]["parameters"],
            serde_json::json!({"type":"object","properties":{}})
        );
    }
    let mut invalid = flat;
    invalid["strict"] = serde_json::Value::Null;
    assert!(crate::normalize_responses_tools(vec![invalid]).is_err());
}

#[test]
fn c3_r3_temperature_identity_normalization_does_not_expand_unknown_policy() {
    for id in [
        "openai/gpt-6.2-sol-2026-11-17",
        "GPT-6.2-SOL",
        "  OPENAI/GPT-6.2-SOL  ",
    ] {
        assert!(crate::responses_temperature_denied(id));
    }
    for id in [
        "future-model-2026-11-17",
        "gpt-6.2-sol-2026-1-17",
        "gpt-6.3-sol-2026-11-17",
    ] {
        assert!(!crate::responses_temperature_denied(id));
    }
}

#[test]
fn responses_functions_preserve_schema_strict_and_validate_named_choice() {
    let tool = serde_json::json!({"type":"function","name":"arbitrary_42","parameters":{"type":"object","additionalProperties":false},"strict":true});
    let tools = crate::normalize_responses_tools(vec![tool.clone()]).unwrap();
    assert_eq!(tools, vec![tool]);
    assert!(crate::normalize_responses_choice(
        Some(serde_json::json!({"type":"function","name":"missing"})),
        &tools
    )
    .is_err());
    let tools = crate::normalize_responses_tools(vec![
        serde_json::json!({"type":"function","function":{"name":"nested","strict":false}}),
    ])
    .unwrap();
    assert_eq!(tools[0]["strict"], false);
    assert_eq!(tools[0]["name"], "nested");
    assert!(
        crate::normalize_responses_tools(vec![serde_json::json!({"type":"web_search"})]).is_err()
    );
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
        service_tier: None,
        previous_response_id: None,
        metadata: None,
        truncation: None,
        temperature: None,
        top_p: None,
        user: None,
    };

    let (model, payload) = proxy.convert_openai_responses_request(req).unwrap();
    assert_eq!(model, "gpt-5.4");
    assert_eq!(payload["model"], serde_json::json!("gpt-5.4"));
    assert!(payload.get("prompt_cache_key").is_none());
    assert_eq!(payload["store"], false);
    assert_eq!(payload["tools"][0]["strict"], false);
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

#[test]
fn new_model_catalog_has_exact_ids_and_sourced_release_dates() {
    let catalog = gateway_models_response();
    let entries = catalog["data"].as_array().unwrap();
    for id in [
        "gpt-6.2-astra",
        "gpt-6.2-sol",
        "gpt-6.1-terra",
        "gpt-6.1-sol",
        "gpt-6-astra",
        "gpt-6-luna",
        "gpt-6-sol",
        "claude-fable-5-2",
        "claude-fable-5-1",
        "claude-opus-5-5",
        "claude-sonnet-5-5",
        "claude-haiku-5-5",
    ] {
        assert_eq!(entries.iter().filter(|entry| entry["id"] == id).count(), 1);
        let expected = match id {
            "gpt-6.2-astra" | "gpt-6.2-sol" => "2026-11-17T00:00:00Z",
            "gpt-6.1-terra" => "2026-10-26T00:00:00Z",
            "gpt-6.1-sol" => "2026-09-29T00:00:00Z",
            "gpt-6-astra" => "2026-09-03T00:00:00Z",
            "gpt-6-sol" | "gpt-6-luna" => "2026-09-22T00:00:00Z",
            "claude-fable-5-2" => "2026-11-09T00:00:00Z",
            "claude-haiku-5-5" => "2026-10-07T00:00:00Z",
            _ => continue,
        };
        let entry = entries.iter().find(|entry| entry["id"] == id).unwrap();
        assert_eq!(entry["created_at"], expected);
        assert_eq!(
            entry["created"].as_i64().unwrap(),
            chrono::DateTime::parse_from_rfc3339(expected)
                .unwrap()
                .timestamp()
        );
    }
    assert!(!entries.iter().any(|entry| ["gpt-6", "gpt-6.1", "gpt-6.2"]
        .iter()
        .any(|id| entry["id"] == *id)));
}

#[test]
fn new_models_route_without_rewriting_explicit_ids_and_alias_overrides_work() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();
    for id in [
        "gpt-6.2-astra",
        "gpt-6.2-sol",
        "gpt-6.1-terra",
        "gpt-6.1-sol",
        "gpt-6-astra",
        "gpt-6-luna",
        "gpt-6-sol",
    ] {
        assert_eq!(normalize_codex_model_id(&format!("openai/{id}")), id);
        assert_eq!(route_anthropic_model(id), AnthropicModelRoute::Codex);
    }
    for id in [
        "claude-fable-5-2",
        "claude-fable-5-1",
        "claude-opus-5-5",
        "claude-sonnet-5-5",
        "claude-haiku-5-5",
    ] {
        assert_eq!(normalize_anthropic_model_id(&format!("anthropic/{id}")), id);
        assert_eq!(route_anthropic_model(id), AnthropicModelRoute::Anthropic);
    }
    std::env::set_var("CODEX_GATEWAY_ALIAS_SONNET", "openai/gpt-6.1-sol");
    assert_eq!(normalize_codex_model_id("sonnet"), "gpt-6.1-sol");
    assert_eq!(route_anthropic_model("sonnet"), AnthropicModelRoute::Codex);
    std::env::set_var("CODEX_GATEWAY_ALIAS_OPUS", "claude-fable-5-1");
    assert_eq!(normalize_anthropic_model_id("opus"), "claude-fable-5-1");
    clear_alias_env();
}

#[test]
fn anthropic_passthrough_preserves_thinking_tool_choice_and_output_config() {
    let _guard = ALIAS_ENV_LOCK.lock().unwrap();
    clear_alias_env();
    let mut request: AnthropicMessagesRequest = serde_json::from_value(serde_json::json!({
        "model":"sonnet", "messages":[{"role":"user","content":"hello"}], "max_tokens":512,
        "thinking":{"type":"between_tools"}, "tool_choice":{"type":"none"},
        "output_config":{"effort":"low"}, "temperature":1, "metadata":{"user_id":"synthetic-user"}
    }))
    .unwrap();
    let (_, route) = prepare_anthropic_ingress(&mut request);
    assert_eq!(route, AnthropicModelRoute::Anthropic);
    let serialized = serde_json::to_vec(&request).unwrap();
    let normalized = normalize_anthropic_request_model(&serialized).unwrap();
    let result: serde_json::Value = serde_json::from_slice(&normalized).unwrap();
    assert_eq!(
        result["thinking"],
        serde_json::json!({"type":"between_tools"})
    );
    assert_eq!(result["tool_choice"], serde_json::json!({"type":"none"}));
    assert_eq!(result["output_config"], serde_json::json!({"effort":"low"}));
    assert_eq!(result["metadata"]["user_id"], "synthetic-user");
}

#[test]
fn canonical_json_renderers_preserve_tool_ids_arguments_and_usage() {
    use crate::streaming::CanonicalStreamEvent;
    let events = vec![
        CanonicalStreamEvent::ToolCallDone {
            call_id: "call_synthetic".into(),
            name: "echo".into(),
            arguments: r#"{"value":"OK"}"#.into(),
        },
        CanonicalStreamEvent::Usage {
            input_tokens: 12,
            output_tokens: 7,
        },
        CanonicalStreamEvent::Completed {
            finish_reason: Some("tool_calls".into()),
        },
    ];
    let chat = crate::render_chat_json(&events, "gpt-6.1-sol").unwrap();
    assert_eq!(chat.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    let calls = chat.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(calls[0]["id"], "call_synthetic");
    assert_eq!(calls[0]["function"]["arguments"], r#"{"value":"OK"}"#);
    let message = crate::convert_chat_to_anthropic("gpt-6.1-sol", chat);
    assert_eq!(message.stop_reason.as_deref(), Some("tool_use"));
    assert_eq!(message.content[0]["id"], "call_synthetic");
    assert_eq!(message.content[0]["input"]["value"], "OK");
    assert_eq!(message.usage.input_tokens, 12);
}

#[test]
fn standard_chat_tool_history_binds_result_to_the_original_call_id() {
    let request: ChatCompletionsRequest = serde_json::from_value(serde_json::json!({
        "model":"gpt-6-astra", "reasoning_effort":"low", "tool_choice":"auto", "messages":[
            {"role":"user", "content":"Call echo"},
            {"role":"assistant", "content":null, "tool_calls":[{"id":"call_matching", "type":"function", "function":{"name":"echo", "arguments":"{}"}}]},
            {"role":"tool", "tool_call_id":"call_matching", "content":"TOOL_OK"}
        ]
    })).unwrap();
    let assistant = crate::normalize_chat_message(&request.messages[1]).unwrap();
    assert!(
        matches!(&assistant[0], crate::NormalizedMessage::AssistantToolCall { id, .. } if id == "call_matching")
    );
    let result = crate::normalize_chat_message(&request.messages[2]).unwrap();
    assert!(
        matches!(&result[0], crate::NormalizedMessage::ToolResult { tool_use_id, content } if tool_use_id == "call_matching" && content == "TOOL_OK")
    );
    let missing_id: ChatMessage =
        serde_json::from_value(serde_json::json!({"role":"tool", "content":"TOOL_OK"})).unwrap();
    assert!(crate::normalize_chat_message(&missing_id).is_err());
}

#[test]
fn canonical_chat_json_rejects_empty_success_instead_of_fabricating_content() {
    use crate::streaming::CanonicalStreamEvent;
    assert!(crate::render_chat_json(
        &[CanonicalStreamEvent::Completed {
            finish_reason: Some("stop".into())
        }],
        "gpt-6-sol"
    )
    .is_err());
}

#[test]
fn codex_item_ids_are_mapped_to_call_ids_without_duplicating_streamed_arguments() {
    let sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_first\",\"call_id\":\"call_first\",\"name\":\"Read\",\"arguments\":\"\"}}\n\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_first\",\"delta\":\"{\\\"file_path\\\":\\\"probe.txt\\\"}\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_first\",\"call_id\":\"call_first\",\"name\":\"Read\",\"arguments\":\"{\\\"file_path\\\":\\\"probe.txt\\\"}\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n"
    );
    let events = crate::streaming::parse_codex_sse_to_events(sse).unwrap();
    let deltas: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            crate::streaming::CanonicalStreamEvent::ToolCallDelta {
                call_id,
                arguments_delta,
            } => Some((call_id, arguments_delta)),
            _ => None,
        })
        .collect();
    assert_eq!(deltas.len(), 1);
    assert_eq!(deltas[0].0, "call_first");
    assert_eq!(deltas[0].1, r#"{"file_path":"probe.txt"}"#);
    let rendered = crate::streaming::render_openai_sse(&events, "gpt-6-sol");
    assert!(!rendered.contains("fc_first"));
    let json = render_openai_responses_json(&events, "gpt-6-sol");
    assert!(json["output"][0]["id"].as_str().unwrap().starts_with("fc_"));
    assert_eq!(json["output"][0]["call_id"], "call_first");
}

#[test]
fn chat_stream_parallel_tool_calls_have_distinct_stable_indices() {
    use crate::streaming::CanonicalStreamEvent;
    let events = vec![
        CanonicalStreamEvent::ToolCallStart {
            call_id: "call_a".into(),
            name: "Read".into(),
        },
        CanonicalStreamEvent::ToolCallStart {
            call_id: "call_b".into(),
            name: "Read".into(),
        },
        CanonicalStreamEvent::ToolCallDelta {
            call_id: "call_b".into(),
            arguments_delta: "{}".into(),
        },
        CanonicalStreamEvent::ToolCallDelta {
            call_id: "call_a".into(),
            arguments_delta: "{}".into(),
        },
    ];
    let rendered = crate::streaming::render_openai_sse(&events, "gpt-6-sol");
    let calls: Vec<serde_json::Value> = rendered
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let indices: Vec<_> = calls
        .iter()
        .map(|chunk| {
            chunk["choices"][0]["delta"]["tool_calls"][0]["index"]
                .as_u64()
                .unwrap()
        })
        .collect();
    assert_eq!(indices, vec![0, 1, 1, 0]);
}

#[test]
fn canonical_production_renderers_recover_final_item_text_without_duplication() {
    let final_item = concat!(
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"Fallback\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{}}\n\n"
    );
    for prefix in [
        "",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Fallback\"}\n\n",
        "data: {\"type\":\"response.output_text.done\",\"text\":\"Fallback\"}\n\n",
    ] {
        let events =
            crate::streaming::parse_codex_sse_to_events(&format!("{prefix}{final_item}")).unwrap();
        let chat = crate::render_chat_json(&events, "gpt-6-sol").unwrap();
        assert_eq!(chat.choices[0].message.content, "Fallback");
        let responses = render_openai_responses_json(&events, "gpt-6-sol");
        assert_eq!(responses["output_text"], "Fallback");
        let anthropic = crate::convert_chat_to_anthropic("gpt-6-sol", chat);
        assert_eq!(anthropic.content[0]["text"], "Fallback");
        let stream = crate::streaming::render_openai_sse(&events, "gpt-6-sol");
        assert_eq!(stream.matches("Fallback").count(), 1);
    }
}

#[test]
fn chat_sse_maps_tool_finish_reason_while_anthropic_keeps_tool_use() {
    for (item, expected) in [
        (
            serde_json::json!({"type":"function_call", "call_id":"call_read", "name":"Read", "arguments":"{}"}),
            "tool_calls",
        ),
        (
            serde_json::json!({"type":"message", "content":[{"type":"output_text", "text":"Done"}]}),
            "stop",
        ),
    ] {
        let sse = format!(
            "data: {}\n\ndata: {{\"type\":\"response.completed\",\"response\":{{}}}}\n\n",
            serde_json::json!({"type":"response.output_item.done", "item":item})
        );
        let events = crate::streaming::parse_codex_sse_to_events(&sse).unwrap();
        let rendered = crate::streaming::render_openai_sse(&events, "gpt-6-sol");
        let chunks: Vec<serde_json::Value> = rendered
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str(data).ok())
            .collect();
        assert_eq!(
            chunks.last().unwrap()["choices"][0]["finish_reason"],
            expected
        );
        assert!(rendered.ends_with("data: [DONE]\n\n"));
        assert_eq!(
            crate::render_chat_json(&events, "gpt-6-sol")
                .unwrap()
                .choices[0]
                .finish_reason
                .as_deref(),
            Some(expected)
        );
        if expected == "tool_calls" {
            assert!(crate::streaming::render_anthropic_sse(&events, "gpt-6-sol")
                .contains("\"stop_reason\":\"tool_use\""));
        }
    }
}

fn assert_canonical_text_rendering(mut upstream: Vec<serde_json::Value>, expected: &str) {
    upstream.push(serde_json::json!({"type":"response.completed", "response":{}}));
    let sse: String = upstream
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect();
    let events = crate::streaming::parse_codex_sse_to_events(&sse).unwrap();
    let chat = crate::render_chat_json(&events, "gpt-6.1-sol").unwrap();
    assert_eq!(chat.choices[0].message.content, expected);
    assert_eq!(
        render_openai_responses_json(&events, "gpt-6.1-sol")["output_text"],
        expected
    );
    let anthropic = crate::convert_chat_to_anthropic("gpt-6.1-sol", chat);
    assert_eq!(anthropic.content[0]["text"], expected);
    let chat_sse = crate::streaming::render_openai_sse(&events, "gpt-6.1-sol");
    let anthropic_sse = crate::streaming::render_anthropic_sse(&events, "gpt-6.1-sol");
    for (rendered, path) in [
        (chat_sse, "/choices/0/delta/content"),
        (anthropic_sse, "/delta/text"),
    ] {
        let text: String = rendered
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str::<serde_json::Value>(data).ok())
            .filter_map(|chunk| {
                chunk
                    .pointer(path)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        assert_eq!(text, expected);
    }
}

#[test]
fn canonical_text_keeps_multiple_final_items_with_or_without_identifiers() {
    for identity in ["none", "item_id", "output_index", "both"] {
        let upstream = ["First", "Second"]
            .iter()
            .enumerate()
            .map(|(index, text)| {
                let mut event = serde_json::json!({"type":"response.output_item.done", "item":{
                "type":"message", "content":[{"type":"output_text", "text":text}]}});
                if identity == "item_id" || identity == "both" {
                    event["item"]["id"] = serde_json::json!(format!("msg_{index}"));
                }
                if identity == "output_index" || identity == "both" {
                    event["output_index"] = serde_json::json!(index);
                }
                event
            })
            .collect();
        assert_canonical_text_rendering(upstream, "FirstSecond");
    }
}

#[test]
fn canonical_text_mixes_streamed_and_final_only_items_across_identity_fields() {
    assert_canonical_text_rendering(
        vec![
            serde_json::json!({"type":"response.output_item.added", "output_index":5, "item":{"type":"message", "id":"msg_first", "content":[]}}),
            serde_json::json!({"type":"response.output_text.delta", "output_index":5, "content_index":0, "delta":"First"}),
            serde_json::json!({"type":"response.output_text.done", "item_id":"msg_first", "content_index":0, "text":"First"}),
            serde_json::json!({"type":"response.output_item.done", "item":{"type":"message", "id":"msg_first", "content":[{"type":"output_text", "text":"First"}]}}),
            serde_json::json!({"type":"response.output_item.done", "output_index":6, "item":{"type":"message", "id":"msg_second", "content":[{"type":"output_text", "text":"Second"}]}}),
        ],
        "FirstSecond",
    );
    // Reduced upstream traces may omit output_item.added and alternate between
    // item_id and output_index before their final item joins the identities.
    assert_canonical_text_rendering(
        vec![
            serde_json::json!({"type":"response.output_text.delta", "item_id":"msg_first", "content_index":0, "delta":"First"}),
            serde_json::json!({"type":"response.output_text.done", "output_index":3, "content_index":0, "text":"First"}),
            serde_json::json!({"type":"response.output_item.done", "output_index":3, "item":{"type":"message", "id":"msg_first", "content":[{"type":"output_text", "text":"First"}]}}),
            serde_json::json!({"type":"response.output_text.delta", "output_index":4, "content_index":0, "delta":"Sec"}),
            serde_json::json!({"type":"response.output_text.done", "item_id":"msg_second", "content_index":0, "text":"Second"}),
            serde_json::json!({"type":"response.output_item.done", "output_index":4, "item":{"type":"message", "id":"msg_second", "content":[{"type":"output_text", "text":"Second"}]}}),
        ],
        "FirstSecond",
    );
}

#[test]
fn canonical_text_tracks_content_parts_including_identical_text_and_nontext_parts() {
    assert_canonical_text_rendering(
        vec![
            serde_json::json!({"type":"response.output_item.added", "output_index":0, "item":{"type":"message", "id":"msg_parts", "content":[]}}),
            serde_json::json!({"type":"response.output_text.delta", "item_id":"msg_parts", "content_index":1, "delta":"Echo"}),
            serde_json::json!({"type":"response.output_item.done", "output_index":0, "item":{"type":"message", "id":"msg_parts", "content":[{"type":"refusal", "refusal":""}, {"type":"output_text", "text":"Echo"}, {"type":"output_text", "text":"Echo"}]}}),
        ],
        "EchoEcho",
    );
}

#[test]
fn canonical_text_done_and_final_items_do_not_duplicate_per_part_or_per_item() {
    assert_canonical_text_rendering(
        vec![
            serde_json::json!({"type":"response.output_item.added", "output_index":0, "item":{"type":"message", "id":"msg_parts", "content":[]}}),
            serde_json::json!({"type":"response.output_text.done", "output_index":0, "content_index":0, "text":"First"}),
            serde_json::json!({"type":"response.output_text.done", "item_id":"msg_parts", "content_index":1, "text":"Second"}),
            serde_json::json!({"type":"response.output_item.done", "output_index":0, "item":{"type":"message", "id":"msg_parts", "content":[{"type":"output_text", "text":"First"}, {"type":"output_text", "text":"Second"}]}}),
            // Repeated terminal item with the same identity remains idempotent.
            serde_json::json!({"type":"response.output_item.done", "item":{"type":"message", "id":"msg_parts", "content":[{"type":"output_text", "text":"First"}, {"type":"output_text", "text":"Second"}]}}),
        ],
        "FirstSecond",
    );
}

#[test]
fn canonical_text_final_events_append_missing_suffix_of_partial_deltas() {
    assert_canonical_text_rendering(
        vec![
            serde_json::json!({"type":"response.output_item.added", "output_index":0, "item":{"type":"message", "id":"msg_first", "content":[]}}),
            serde_json::json!({"type":"response.output_text.delta", "item_id":"msg_first", "content_index":0, "delta":"Пр"}),
            serde_json::json!({"type":"response.output_text.done", "output_index":0, "content_index":0, "text":"Привет"}),
            serde_json::json!({"type":"response.output_item.done", "item":{"type":"message", "id":"msg_first", "content":[{"type":"output_text", "text":"Привет"}]}}),
            serde_json::json!({"type":"response.output_text.delta", "output_index":1, "item_id":"msg_second", "content_index":0, "delta":"Sec"}),
            serde_json::json!({"type":"response.output_item.done", "output_index":1, "item":{"type":"message", "id":"msg_second", "content":[{"type":"output_text", "text":"Second"}]}}),
        ],
        "ПриветSecond",
    );
}

#[test]
fn canonical_text_rejects_contradictory_final_text_instead_of_losing_streamed_content() {
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_a\",\"content_index\":0,\"delta\":\"First\"}\n\n",
        "data: {\"type\":\"response.output_text.done\",\"item_id\":\"msg_a\",\"content_index\":0,\"text\":\"Different\"}\n\n"
    );
    assert!(matches!(
        crate::streaming::parse_codex_sse_to_events(sse),
        Err(ProxyError::UpstreamProtocol { .. })
    ));
}

#[test]
fn p5_segmented_sse_matches_buffered_with_utf8_crlf_and_final_frame() {
    use crate::streaming::{parse_codex_sse_to_events, CodexSseParser};
    let wire = concat!(
        ": heartbeat\r\n\r\n",
        "event: response.output_text.delta\r\ndata: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"café\"}\r\n\r\n",
        "data: {\"type\":\"response.output_text.done\",\"output_index\":0,\"text\":\"café!\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":17,\"output_tokens\":11}}}"
    );
    let expected = parse_codex_sse_to_events(wire).unwrap();
    for size in 1..=wire.len() {
        let mut parser = CodexSseParser::default();
        let mut events = Vec::new();
        for bytes in wire.as_bytes().chunks(size) {
            events.extend(parser.feed(bytes).unwrap());
        }
        events.extend(parser.finish().unwrap());
        assert_eq!(events, expected, "chunk size {size}");
    }
}

#[test]
fn p5_strict_terminal_and_malformed_data() {
    use crate::streaming::parse_codex_sse_to_events;
    for wire in [
        "data: {broken}\n\n",
        "data: {}\n\n",
        "data: [DONE]\n\n",
        "data: {\"type\":\"response.output_text.delta\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{\"incomplete_details\":{\"reason\":\"unknown\"}}}\n\n",
        "data: {\"type\":\"response.failed\"}\n\n",
        "data: {\"type\":\"error\"}\n\n",
        "data: {\"type\":\"response.completed\"}\n\ndata: {\"type\":\"response.completed\"}\n\n",
    ] {
        assert!(parse_codex_sse_to_events(wire).is_err(), "accepted {wire}");
    }
}

#[test]
fn p5_tool_final_suffix_and_terminal_snapshot_deduplicate() {
    use crate::streaming::{parse_codex_sse_to_events, render_anthropic_sse};
    let item = serde_json::json!({"type":"function_call","id":"fc","call_id":"call","name":"Read","arguments":"{\"file_path\":\"x\"}"});
    let wire: String = [
        serde_json::json!({"type":"response.output_item.added","item":{"type":"function_call","id":"fc","call_id":"call","name":"Read"}}),
        serde_json::json!({"type":"response.function_call_arguments.delta","item_id":"fc","delta":"{\"file_path\":"}),
        serde_json::json!({"type":"response.function_call_arguments.done","item_id":"fc","arguments":"{\"file_path\":\"x\"}"}),
        serde_json::json!({"type":"response.output_item.done","item":item}),
        serde_json::json!({"type":"response.completed","response":{"output":[item],"usage":{"input_tokens":23,"output_tokens":13}}}),
    ].iter().map(|v|format!("data: {v}\n\n")).collect();
    let events = parse_codex_sse_to_events(&wire).unwrap();
    let out = render_anthropic_sse(&events, "fixture");
    assert_eq!(out.matches("event: content_block_start").count(), 1);
    assert_eq!(out.matches("event: content_block_stop").count(), 1);
    assert!(out.contains("\"input_tokens\":23,\"output_tokens\":13"));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(
                e,
                crate::streaming::CanonicalStreamEvent::ToolCallDone { .. }
            ))
            .count(),
        1
    );
    let broken = wire.replace(
        "\"arguments\":\"{\\\"file_path\\\":\\\"x\\\"}\"",
        "\"arguments\":\"{}\"",
    );
    assert!(parse_codex_sse_to_events(&broken).is_err());
}

#[test]
fn p5_terminal_snapshot_text_and_max_tokens_usage() {
    use crate::streaming::{parse_codex_sse_to_events, render_anthropic_sse};
    let wire = [
        serde_json::json!({"type":"response.output_text.delta","output_index":0,"delta":"hel"}),
        serde_json::json!({"type":"response.incomplete","response":{"output":[{"type":"message","content":[{"type":"output_text","text":"hello"}]}],"incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":1,"output_tokens":2}}}),
    ].iter().map(|v|format!("data: {v}\n\n")).collect::<String>();
    let out = render_anthropic_sse(&parse_codex_sse_to_events(&wire).unwrap(), "fixture");
    assert!(out.contains("\"text\":\"hel\""));
    assert!(out.contains("\"text\":\"lo\""));
    assert!(out.contains("\"stop_reason\":\"max_tokens\""));
    assert_eq!(out.matches("event: message_stop").count(), 1);
}

#[test]
fn p5_bounded_frames_and_invalid_utf8_fail() {
    use crate::streaming::CodexSseParser;
    assert!(CodexSseParser::default()
        .feed(&vec![b'x'; 1024 * 1024 + 1])
        .is_err());
    assert!(CodexSseParser::default().feed(b"data: \xff\n\n").is_err());
}

#[test]
fn p5_r1_unfinished_tool_requires_completion_or_max_tokens() {
    use crate::streaming::parse_codex_sse_to_events;
    let start = serde_json::json!({"type":"response.output_item.added","item":{"type":"function_call","id":"fc_r1","call_id":"call_r1","name":"Read","arguments":""}});
    let completed = serde_json::json!({"type":"response.completed","response":{"status":"completed","output":[]}});
    assert!(parse_codex_sse_to_events(&format!("data: {start}\n\ndata: {completed}\n\n")).is_err());
    let limited = serde_json::json!({"type":"response.incomplete","response":{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"}}});
    assert!(parse_codex_sse_to_events(&format!("data: {start}\n\ndata: {limited}\n\n")).is_ok());
    let finalized = serde_json::json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"function_call","id":"fc_r1","call_id":"call_r1","name":"Read","arguments":"{}"}]}});
    assert!(parse_codex_sse_to_events(&format!("data: {start}\n\ndata: {finalized}\n\n")).is_ok());
}

#[test]
fn p5_r2_finalized_arguments_are_immutable() {
    use crate::streaming::{CanonicalStreamEvent, CodexSseParser};
    let start = serde_json::json!({"type":"response.output_item.added","item":{"type":"function_call","id":"fc_r2","call_id":"call_r2","name":"Read","arguments":""}});
    let done = serde_json::json!({"type":"response.function_call_arguments.done","item_id":"fc_r2","arguments":"{}"});
    for changed in ["{} ", "{}{}", "", "{\"changed\":true}"] {
        let mut parser = CodexSseParser::default();
        parser
            .feed(format!("data: {start}\n\n").as_bytes())
            .unwrap();
        assert!(parser
            .feed(format!("data: {done}\n\n").as_bytes())
            .unwrap()
            .iter()
            .any(|e| matches!(e, CanonicalStreamEvent::ToolCallDone { .. })));
        let snapshot = serde_json::json!({"type":"response.output_item.done","item":{"type":"function_call","call_id":"call_r2","name":"Read","arguments":changed}});
        assert!(
            parser
                .feed(format!("data: {snapshot}\n\n").as_bytes())
                .is_err(),
            "accepted changed final arguments {changed:?}"
        );
    }
}
#[test]
fn c1_native_frames_preserve_bytes_and_stop_at_terminal() {
    use crate::streaming::NativeResponsesSseParser;
    let raw = concat!(
        ": opaque\r\nid: 42\r\nevent: response.output_text.delta\r\n",
        "data: {\"type\":\"response.output_text.delta\",\r\n",
        "data: \"delta\":\"café\",\"opaque\":{\"x\":1}}\r\n\r\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
    );
    for size in [1, 7, raw.len()] {
        let mut parser = NativeResponsesSseParser::default();
        let mut output = Vec::new();
        for bytes in raw.as_bytes().chunks(size) {
            output.extend(parser.feed(bytes).unwrap());
        }
        assert_eq!(output, raw.as_bytes());
        assert!(parser.terminal);
        assert!(parser.finish().is_ok());
    }
    let mut parser = NativeResponsesSseParser::default();
    assert_eq!(
        parser
            .feed(format!("{raw}data: malformed suffix\n\n").as_bytes())
            .unwrap(),
        raw.as_bytes()
    );
}

#[test]
fn c1_native_terminal_normalization_and_fail_closed() {
    use crate::streaming::NativeResponsesSseParser;
    for status in ["completed", "incomplete", "failed"] {
        let mut parser = NativeResponsesSseParser::default();
        let raw = format!("id: keep\nevent: response.done\ndata: {{\"type\":\"response.done\",\"opaque\":7,\"response\":{{\"status\":\"{status}\",\"usage\":{{\"input_tokens\":3}}}}}}\n\n");
        let output = String::from_utf8(parser.feed(raw.as_bytes()).unwrap()).unwrap();
        assert!(output.contains(&format!("event: response.{status}\n")));
        assert!(output.contains("id: keep\n"));
        let value: serde_json::Value = serde_json::from_str(
            output
                .lines()
                .find_map(|s| s.strip_prefix("data: "))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(value["type"], format!("response.{status}"));
        assert_eq!(value["opaque"], 7);
        assert_eq!(value["response"]["usage"]["input_tokens"], 3);
        assert!(parser.terminal);
    }
    for raw in [
        "data: {broken}\n\n",
        "data: {\"type\":\"response.completed\"}\n\n",
        "data: {\"type\":\"response.done\",\"response\":{\"status\":\"in_progress\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"failed\"}}\n\n",
        "event: response.failed\ndata: {\"type\":\"response.completed\"}\n\n",
        "data: [DONE]\n\n",
    ] {
        assert!(
            NativeResponsesSseParser::default()
                .feed(raw.as_bytes())
                .is_err(),
            "{raw}"
        );
    }
    assert!(NativeResponsesSseParser::default().finish().is_err());
    assert!(NativeResponsesSseParser::default()
        .feed(&vec![b'x'; 1024 * 1024 + 1])
        .is_err());
    // A valid frame preceding a broken one is emitted, then its error is explicit.
    let mut parser = NativeResponsesSseParser::default();
    assert!(!parser
        .feed(b"data: {\"type\":\"opaque\"}\n\ndata: broken\n\n")
        .unwrap()
        .is_empty());
    assert!(parser.check_error().is_err());
}

#[test]
fn c1_r2_native_eof_dispatches_only_valid_terminals() {
    use crate::streaming::NativeResponsesSseParser;
    for suffix in ["", "\n", "\r\n", "\n\r"] {
        for status in ["completed", "incomplete", "failed"] {
            for kind in [format!("response.{status}"), "response.done".into()] {
                let raw = format!("id: keep\nevent: {kind}\ndata: {{\"type\":\"{kind}\",\"response\":{{\"status\":\"{status}\",\"extension\":{{\"keep\":42}}}}}}{suffix}");
                let mut parser = NativeResponsesSseParser::default();
                for byte in raw.as_bytes().chunks(1) {
                    assert!(parser.feed(byte).unwrap().is_empty());
                }
                let output = parser.finish().unwrap();
                assert!(parser.terminal);
                assert!(output.ends_with(b"\n\n"));
                if kind != "response.done" {
                    assert!(output.starts_with(raw.as_bytes()));
                }
                let text = String::from_utf8(output).unwrap();
                let value: serde_json::Value = serde_json::from_str(
                    text.lines()
                        .find_map(|line| line.strip_prefix("data: "))
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(value["type"], format!("response.{status}"));
                assert_eq!(value["response"]["extension"]["keep"], 42);
                assert!(parser.finish().unwrap().is_empty());
            }
        }
    }
    for raw in [
        "data: {broken}",
        "data: {\"type\":\"response.completed\"}",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"failed\"}}",
        "data: {\"type\":\"opaque\"}",
        "data: {\"type\":\"response.done\",\"response\":{\"status\":\"in_progress\"}}",
    ] {
        let mut parser = NativeResponsesSseParser::default();
        assert!(parser.feed(raw.as_bytes()).unwrap().is_empty());
        assert!(parser.finish().is_err(), "accepted EOF frame {raw}");
    }
}

#[test]
fn c2_json_preserves_native_terminal_and_uses_ordered_text() {
    use crate::streaming::NativeResponsesSseParser;
    use serde_json::json;
    for native_text in [None, Some(""), Some("authoritative")] {
        let mut response = json!({"id":"native", "model":"native-model", "status":"incomplete",
            "output":[{"type":"function_call","call_id":"call","arguments":"{}","opaque":5},
                      {"type":"message","content":[{"type":"output_text","text":"first"},
                       {"type":"refusal","refusal":"keep"},{"type":"output_text","text":" second"}]}],
            "incomplete_details":{"reason":"max_output_tokens"},
            "usage":{"output_tokens_details":{"reasoning_tokens":9}}});
        if let Some(text) = native_text {
            response["output_text"] = json!(text);
        }
        let mut parser = NativeResponsesSseParser::for_json();
        let raw = format!(
            "data: {}\n\ndata: {}",
            json!({"type":"response.output_text.delta","delta":"stale"}),
            json!({"type":"response.done","response":response})
        );
        for bytes in raw.as_bytes().chunks(1) {
            parser.feed(bytes).unwrap();
        }
        parser.finish().unwrap();
        let actual = parser.into_response().unwrap();
        for (key, value) in response.as_object().unwrap() {
            assert_eq!(&actual[key], value);
        }
        assert_eq!(actual["output_text"], native_text.unwrap_or("first second"));
        assert_eq!(actual["object"], "response");
        assert!(actual["usage"].get("total_tokens").is_none());
    }
    let mut parser = NativeResponsesSseParser::for_json();
    parser.feed(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"fallback\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n").unwrap();
    let actual = parser.into_response().unwrap();
    assert_eq!(actual["output_text"], "fallback");
    assert!(actual.get("usage").is_none());
    for raw in [
        "data: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\"}}\n\n",
        "data: {\"type\":\"error\",\"message\":\"synthetic secret\"}\n\n",
    ] {
        let mut parser = NativeResponsesSseParser::for_json();
        if parser.feed(raw.as_bytes()).is_ok() {
            assert!(parser.into_response().is_err());
        }
    }
}

#[test]
fn c2_retry_after_is_single_valid_and_not_connection_nominated() {
    use crate::{responses_error_response, responses_http_error, StatusCode};
    for (values, connection, expected) in [
        (vec!["12"], None, Some("12")),
        (
            vec!["Wed, 21 Oct 2015 07:28:00 GMT"],
            None,
            Some("Wed, 21 Oct 2015 07:28:00 GMT"),
        ),
        (
            vec!["Wednesday, 21-Oct-15 07:28:00 GMT"],
            None,
            Some("Wednesday, 21-Oct-15 07:28:00 GMT"),
        ),
        (
            vec!["Wed Oct 21 07:28:00 2015"],
            None,
            Some("Wed Oct 21 07:28:00 2015"),
        ),
        (vec!["1", "2"], None, None),
        (vec!["-1"], None, None),
        (vec!["1, 2"], None, None),
        (vec!["secret"], None, None),
        (vec!["Wed, 99 Oct 2015 07:28:00 GMT"], None, None),
        (vec!["Wednesday, 21 Oct 2015 07:28:00 GMT"], None, None),
        (vec!["Wed, 21 Oct 15 07:28:00 GMT"], None, None),
        (vec!["Wed, 21 Oct 2015 07:28:00 UTC"], None, None),
        (vec![" Wed, 21 Oct 2015 07:28:00 GMT"], None, None),
        (vec!["12"], Some("keep-alive, RETRY-AFTER"), None),
    ] {
        let mut builder = warp::http::Response::builder().status(429);
        for value in values {
            builder = builder.header("retry-after", value);
        }
        if let Some(value) = connection {
            builder = builder.header("connection", value);
        }
        let upstream: reqwest::Response = builder.body("").unwrap().into();
        let ProxyError::ResponsesHttp {
            status,
            retry_after,
        } = responses_http_error(&upstream)
        else {
            panic!("wrong variant")
        };
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(retry_after.as_deref(), expected);
    }
    let timeout = ProxyError::UpstreamTimeout {
        message: "legacy diagnostic".into(),
    };
    assert!(timeout.is_retryable_codex_upstream_error());
    assert_eq!(timeout.openai_status_code(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        responses_error_response(timeout).status(),
        StatusCode::GATEWAY_TIMEOUT
    );
}

#[test]
fn c2_r1_sparse_native_finals_preserve_items_order_and_text_without_duplicates() {
    use crate::streaming::NativeResponsesSseParser;
    use serde_json::json;
    let message = json!({"id":"msg_native","type":"message","content":[{"type":"output_text","text":"final snapshot","opaque":7}]});
    let call = json!({"id":"fc_native","type":"function_call","call_id":"call_native","name":"Read","arguments":"{}","opaque":42});
    for explicit_output in [
        None,
        Some(json!([])),
        Some(
            json!([{"id":"terminal","type":"message","content":[{"type":"output_text","text":"authoritative"}]}]),
        ),
    ] {
        for explicit_text in [None, Some("")] {
            let mut parser = NativeResponsesSseParser::for_json();
            let mut response = json!({"id":"native","status":"completed"});
            if let Some(output) = &explicit_output {
                response["output"] = output.clone();
            }
            if let Some(text) = explicit_text {
                response["output_text"] = json!(text);
            }
            for event in [
                json!({"type":"response.output_item.done","output_index":1,"item":call}),
                json!({"type":"response.output_text.delta","item_id":"msg_native","delta":"final "}),
                json!({"type":"response.output_text.done","item_id":"msg_native","output_index":0,"text":"final snapshot"}),
                json!({"type":"response.output_item.done","output_index":0,"item":message}),
                json!({"type":"response.output_item.done","output_index":0,"item":message}),
                json!({"type":"response.completed","response":response}),
            ] {
                for byte in format!("data: {event}\n\n").as_bytes() {
                    parser.feed(&[*byte]).unwrap();
                }
            }
            let actual = parser.into_response().unwrap();
            assert_eq!(
                actual["output"],
                explicit_output
                    .clone()
                    .unwrap_or_else(|| json!([message, call]))
            );
            let expected_text = explicit_text.unwrap_or(
                if explicit_output
                    .as_ref()
                    .is_some_and(|output| output.as_array().is_some_and(|items| !items.is_empty()))
                {
                    "authoritative"
                } else {
                    "final snapshot"
                },
            );
            assert_eq!(actual["output_text"], expected_text);
            assert!(actual.get("usage").is_none());
        }
    }
    let mut parser = NativeResponsesSseParser::for_json();
    parser.feed(b"data: {\"type\":\"response.output_text.done\",\"text\":\"standalone\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n").unwrap();
    let actual = parser.into_response().unwrap();
    assert_eq!(actual["output_text"], "standalone");
    assert!(actual.get("output").is_none());
    assert!(actual.get("id").is_none());
}

#[test]
fn c2_r2_missing_status_uses_validated_terminal_kind_only_in_json() {
    use crate::streaming::NativeResponsesSseParser;
    use serde_json::json;
    for status in ["completed", "incomplete"] {
        let event = json!({"type":format!("response.{status}"),"response":{"id":"native","incomplete_details":{"reason":"max_output_tokens"}}});
        let wire = format!("data: {event}\n\n");
        let mut json = NativeResponsesSseParser::for_json();
        json.feed(wire.as_bytes()).unwrap();
        let actual = json.into_response().unwrap();
        assert_eq!(actual["status"], status);
        assert_eq!(actual["id"], "native");
        assert_eq!(
            actual["incomplete_details"],
            event["response"]["incomplete_details"]
        );
        let mut native = NativeResponsesSseParser::default();
        assert_eq!(native.feed(wire.as_bytes()).unwrap(), wire.as_bytes());
        for wrong in [json!(null), json!("failed"), json!("in_progress")] {
            let mut parser = NativeResponsesSseParser::for_json();
            let wire = format!(
                "data: {}\n\n",
                json!({"type":format!("response.{status}"),"response":{"status":wrong}})
            );
            assert!(parser.feed(wire.as_bytes()).is_err());
        }
    }
}
