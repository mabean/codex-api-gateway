use std::fs;

use tempfile::NamedTempFile;

use crate::{
    extract_response_content, flatten_message_content, AnthropicMessage, AnthropicMessagesRequest,
    ChatCompletionsRequest, ChatMessage, ProxyError, ProxyServer,
};

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

    let proxy = ProxyServer::new(file.path().to_str().unwrap(), "https://example.test")
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

    let proxy = ProxyServer::new(file.path().to_str().unwrap(), "https://example.test")
        .await
        .unwrap();
    assert_eq!(proxy.auth_data.access_token.as_deref(), Some(jwt));
    assert_eq!(proxy.auth_data.account_id.as_deref(), Some("acc_openclaw"));
}

#[tokio::test]
async fn rejects_file_without_usable_credentials() {
    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), r#"{"tokens": {}}"#).unwrap();

    let result = ProxyServer::new(file.path().to_str().unwrap(), "https://example.test").await;
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
        client: reqwest::Client::new(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
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
        client: reqwest::Client::new(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
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
        client: reqwest::Client::new(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
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
        client: reqwest::Client::new(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
    };

    let req = AnthropicMessagesRequest {
        model: "claude-test".to_string(),
        messages: vec![AnthropicMessage {
            role: "system".to_string(),
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
fn chat_request_requires_messages() {
    let proxy = ProxyServer {
        client: reqwest::Client::new(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
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
        client: reqwest::Client::new(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
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
        client: reqwest::Client::new(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
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
    assert_eq!(input.len(), 2);
    assert_eq!(
        input[1].get("type").and_then(|v| v.as_str()),
        Some("message")
    );
    assert_eq!(
        input[1].get("role").and_then(|v| v.as_str()),
        Some("assistant")
    );
    assert_eq!(
        input[1].get("status").and_then(|v| v.as_str()),
        Some("completed")
    );
    assert_eq!(input[1]["content"][0]["type"].as_str(), Some("output_text"));
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
        crate::streaming::CanonicalStreamEvent::Completed { finish_reason: Some("tool_use".to_string()) },
    ];
    let out = crate::streaming::render_anthropic_sse(&events, "gpt-5.4");
    assert!(out.contains("tool_use"));
    assert!(out.contains("input_json_delta"));
    assert!(out.contains("call_123"));
}

#[test]
fn anthropic_tool_result_blocks_become_function_call_output_items() {
    let proxy = ProxyServer {
        client: reqwest::Client::new(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
    };
    let req = ChatCompletionsRequest {
        model: "claude-sonnet-4-5".to_string(),
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
        crate::streaming::CanonicalStreamEvent::Completed { finish_reason: Some("tool_use".to_string()) },
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
        assert!(out.contains("tool_use"), "fixture {name} must render Anthropic tool_use block");
        assert!(out.contains("\"stop_reason\":\"tool_use\""), "fixture {name} must stop with tool_use");
        assert!(out.contains("event: message_stop\n"), "fixture {name} must include message_stop");
        assert!(out.contains("event: content_block_start\n"), "fixture {name} must include content_block_start");
        assert!(out.contains("event: content_block_stop\n"), "fixture {name} must include content_block_stop");
    }
}

#[test]
fn anthropic_assistant_tool_use_blocks_become_codex_function_call_items() {
    let proxy = ProxyServer {
        client: reqwest::Client::new(),
        auth_data: crate::AuthData {
            api_key: None,
            access_token: None,
            account_id: None,
        },
        upstream_base_url: "https://example.test".to_string(),
    };
    let req = ChatCompletionsRequest {
        model: "claude-sonnet-4-5".to_string(),
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
    assert_eq!(tool["parameters"]["required"], serde_json::json!(["file_path", "old_string", "new_string"]));
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
    assert!(normalized["properties"]["isolation"].get("required").is_none());
}
