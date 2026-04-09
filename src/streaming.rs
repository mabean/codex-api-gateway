use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

use crate::ProxyError;

#[derive(Debug, Clone, PartialEq)]
pub enum CanonicalStreamEvent {
    MessageStart,
    TextDelta {
        text: String,
    },
    TextDone {
        full_text: String,
    },
    ToolCallStart {
        call_id: String,
        name: String,
    },
    ToolCallDelta {
        call_id: String,
        arguments_delta: String,
    },
    ToolCallDone {
        call_id: String,
        name: String,
        arguments: String,
    },
    Usage {
        input_tokens: u32,
        output_tokens: u32,
    },
    Completed {
        finish_reason: Option<String>,
    },
}

pub fn parse_codex_sse_to_events(
    response_text: &str,
) -> Result<Vec<CanonicalStreamEvent>, ProxyError> {
    let mut events = Vec::new();
    let mut saw_message_start = false;
    let mut accumulated_text = String::new();
    let mut tool_state: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();
    let mut saw_tool_use = false;

    for line in response_text.lines() {
        let Some(json_data) = line.strip_prefix("data: ") else {
            continue;
        };
        if json_data == "[DONE]" {
            break;
        }
        let Ok(event) = serde_json::from_str::<Value>(json_data) else {
            continue;
        };
        let Some(event_type) = event.get("type").and_then(Value::as_str) else {
            continue;
        };
        match event_type {
            "response.output_item.added" | "response.output_item.done" => {
                if let Some(item) = event.get("item") {
                    match item.get("type").and_then(Value::as_str) {
                        Some("message") => {
                            if !saw_message_start {
                                saw_message_start = true;
                                events.push(CanonicalStreamEvent::MessageStart);
                            }
                        }
                        Some("function_call") => {
                            saw_tool_use = true;
                            let call_id = item
                                .get("call_id")
                                .or_else(|| item.get("id"))
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            let name = item
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            let arguments = item
                                .get("arguments")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            let status = item
                                .get("status")
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            let is_new = !tool_state.contains_key(&call_id);
                            tool_state
                                .entry(call_id.clone())
                                .or_insert((name.clone(), String::new()));
                            if is_new {
                                events.push(CanonicalStreamEvent::ToolCallStart {
                                    call_id: call_id.clone(),
                                    name: name.clone(),
                                });
                            }
                            if !arguments.is_empty() {
                                if let Some((_name, args)) = tool_state.get_mut(&call_id) {
                                    if args.is_empty() {
                                        *args = arguments.clone();
                                        events.push(CanonicalStreamEvent::ToolCallDelta {
                                            call_id: call_id.clone(),
                                            arguments_delta: arguments.clone(),
                                        });
                                    }
                                }
                            }
                            if status == "completed" || event_type == "response.output_item.done" {
                                let final_arguments = tool_state
                                    .get(&call_id)
                                    .map(|(_, args)| if args.is_empty() { arguments.clone() } else { args.clone() })
                                    .unwrap_or_else(|| arguments.clone());
                                events.push(CanonicalStreamEvent::ToolCallDone {
                                    call_id,
                                    name,
                                    arguments: final_arguments,
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                let call_id = event
                    .get("item_id")
                    .or_else(|| event.get("call_id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let delta = event
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if let Some((_name, args)) = tool_state.get_mut(&call_id) {
                    args.push_str(&delta);
                }
                if !delta.is_empty() {
                    saw_tool_use = true;
                    events.push(CanonicalStreamEvent::ToolCallDelta {
                        call_id,
                        arguments_delta: delta,
                    });
                }
            }
            "response.output_text.delta" => {
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    accumulated_text.push_str(delta);
                    events.push(CanonicalStreamEvent::TextDelta {
                        text: delta.to_string(),
                    });
                }
            }
            "response.output_text.done" => {
                if let Some(text) = event.get("text").and_then(Value::as_str) {
                    if accumulated_text.is_empty() {
                        accumulated_text.push_str(text);
                    }
                    events.push(CanonicalStreamEvent::TextDone {
                        full_text: text.to_string(),
                    });
                }
            }
            "response.completed" => {
                if let Some(usage) = event.get("response").and_then(|r| r.get("usage")) {
                    let input_tokens = usage
                        .get("input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32;
                    let output_tokens = usage
                        .get("output_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32;
                    events.push(CanonicalStreamEvent::Usage {
                        input_tokens,
                        output_tokens,
                    });
                }
                events.push(CanonicalStreamEvent::Completed {
                    finish_reason: Some(if saw_tool_use {
                        "tool_use".to_string()
                    } else {
                        "stop".to_string()
                    }),
                });
            }
            "response.failed" => {
                let message = event
                    .get("response")
                    .and_then(|r| r.get("error"))
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("Codex response failed")
                    .to_string();
                return Err(ProxyError::UpstreamProtocol { message });
            }
            "error" => {
                let message = event
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Codex upstream error")
                    .to_string();
                return Err(ProxyError::UpstreamProtocol { message });
            }
            _ => {}
        }
    }
    if events.is_empty() {
        return Err(ProxyError::UpstreamProtocol {
            message: "upstream returned success but no parsable stream events".to_string(),
        });
    }
    Ok(events)
}

#[derive(Serialize)]
struct OpenAiChunkChoiceDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCallDelta>>,
}

#[derive(Serialize)]
struct OpenAiToolCallDelta {
    index: u32,
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OpenAiFunctionDelta,
}

#[derive(Serialize)]
struct OpenAiFunctionDelta {
    name: String,
    arguments: String,
}

#[derive(Serialize)]
struct OpenAiChunkChoice {
    index: u32,
    delta: OpenAiChunkChoiceDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
}

#[derive(Serialize)]
struct OpenAiChunk {
    id: String,
    object: String,
    created: i64,
    model: String,
    choices: Vec<OpenAiChunkChoice>,
}

pub fn render_openai_sse(events: &[CanonicalStreamEvent], model: &str) -> String {
    let id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
    let created = chrono::Utc::now().timestamp();
    let mut out = String::new();
    for event in events {
        match event {
            CanonicalStreamEvent::MessageStart => {
                let chunk = OpenAiChunk {
                    id: id.clone(),
                    object: "chat.completion.chunk".to_string(),
                    created,
                    model: model.to_string(),
                    choices: vec![OpenAiChunkChoice {
                        index: 0,
                        delta: OpenAiChunkChoiceDelta {
                            role: Some("assistant".to_string()),
                            content: None,
                            tool_calls: None,
                        },
                        finish_reason: None,
                    }],
                };
                out.push_str("data: ");
                out.push_str(&serde_json::to_string(&chunk).unwrap());
                out.push_str("\n\n");
            }
            CanonicalStreamEvent::TextDelta { text } => {
                let chunk = OpenAiChunk {
                    id: id.clone(),
                    object: "chat.completion.chunk".to_string(),
                    created,
                    model: model.to_string(),
                    choices: vec![OpenAiChunkChoice {
                        index: 0,
                        delta: OpenAiChunkChoiceDelta {
                            role: None,
                            content: Some(text.clone()),
                            tool_calls: None,
                        },
                        finish_reason: None,
                    }],
                };
                out.push_str("data: ");
                out.push_str(&serde_json::to_string(&chunk).unwrap());
                out.push_str("\n\n");
            }
            CanonicalStreamEvent::ToolCallStart { call_id, name } => {
                let chunk = OpenAiChunk {
                    id: id.clone(),
                    object: "chat.completion.chunk".to_string(),
                    created,
                    model: model.to_string(),
                    choices: vec![OpenAiChunkChoice {
                        index: 0,
                        delta: OpenAiChunkChoiceDelta {
                            role: None,
                            content: None,
                            tool_calls: Some(vec![OpenAiToolCallDelta {
                                index: 0,
                                id: call_id.clone(),
                                call_type: "function".to_string(),
                                function: OpenAiFunctionDelta {
                                    name: name.clone(),
                                    arguments: String::new(),
                                },
                            }]),
                        },
                        finish_reason: None,
                    }],
                };
                out.push_str("data: ");
                out.push_str(&serde_json::to_string(&chunk).unwrap());
                out.push_str("\n\n");
            }
            CanonicalStreamEvent::ToolCallDelta {
                call_id,
                arguments_delta,
            } => {
                let chunk = OpenAiChunk {
                    id: id.clone(),
                    object: "chat.completion.chunk".to_string(),
                    created,
                    model: model.to_string(),
                    choices: vec![OpenAiChunkChoice {
                        index: 0,
                        delta: OpenAiChunkChoiceDelta {
                            role: None,
                            content: None,
                            tool_calls: Some(vec![OpenAiToolCallDelta {
                                index: 0,
                                id: call_id.clone(),
                                call_type: "function".to_string(),
                                function: OpenAiFunctionDelta {
                                    name: String::new(),
                                    arguments: arguments_delta.clone(),
                                },
                            }]),
                        },
                        finish_reason: None,
                    }],
                };
                out.push_str("data: ");
                out.push_str(&serde_json::to_string(&chunk).unwrap());
                out.push_str("\n\n");
            }
            CanonicalStreamEvent::Completed { finish_reason } => {
                let chunk = OpenAiChunk {
                    id: id.clone(),
                    object: "chat.completion.chunk".to_string(),
                    created,
                    model: model.to_string(),
                    choices: vec![OpenAiChunkChoice {
                        index: 0,
                        delta: OpenAiChunkChoiceDelta {
                            role: None,
                            content: None,
                            tool_calls: None,
                        },
                        finish_reason: Some(finish_reason.clone().unwrap_or_else(|| "stop".to_string())),
                    }],
                };
                out.push_str("data: ");
                out.push_str(&serde_json::to_string(&chunk).unwrap());
                out.push_str("\n\n");
                out.push_str("data: [DONE]\n\n");
            }
            _ => {}
        }
    }
    out
}

fn verbose_tracing_enabled() -> bool {
    matches!(std::env::var("CODEX_PROXY_VERBOSE").as_deref(), Ok("1") | Ok("true") | Ok("TRUE"))
}

pub fn render_anthropic_sse(events: &[CanonicalStreamEvent], model: &str) -> String {
    let mut text_deltas = 0usize;
    let mut tool_call_starts = 0usize;
    let mut tool_call_deltas = 0usize;
    let mut tool_call_dones = 0usize;
    let mut completed_finish_reason: Option<String> = None;
    for event in events {
        match event {
            CanonicalStreamEvent::TextDelta { .. } => text_deltas += 1,
            CanonicalStreamEvent::ToolCallStart { .. } => tool_call_starts += 1,
            CanonicalStreamEvent::ToolCallDelta { .. } => tool_call_deltas += 1,
            CanonicalStreamEvent::ToolCallDone { .. } => tool_call_dones += 1,
            CanonicalStreamEvent::Completed { finish_reason } => completed_finish_reason = finish_reason.clone(),
            _ => {}
        }
    }
    eprintln!(
        "[anthropic-render-summary] {}",
        serde_json::json!({
            "model": model,
            "text_deltas": text_deltas,
            "tool_call_starts": tool_call_starts,
            "tool_call_deltas": tool_call_deltas,
            "tool_call_dones": tool_call_dones,
            "completed_finish_reason": completed_finish_reason,
        })
    );
    let message_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
    let mut out = String::new();
    let mut final_stop_reason = "end_turn";
    let mut message_stop_seen = false;
    out.push_str("event: message_start
");
    out.push_str("data: ");
    out.push_str(&json_string(&serde_json::json!({
        "type":"message_start",
        "message":{
            "id":message_id,
            "type":"message",
            "role":"assistant",
            "content":[],
            "model":model,
            "stop_reason":null,
            "stop_sequence":null,
            "usage":{"input_tokens":0,"output_tokens":0}
        }
    })));
    out.push_str("

");

    let mut current_block_index: Option<u32> = None;
    let mut next_block_index = 0u32;
    let mut saw_tool_use = false;
    let mut tool_block_indices: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

    for event in events {
        match event {
            CanonicalStreamEvent::TextDelta { text } => {
                if current_block_index.is_none() {
                    let block_index = next_block_index;
                    next_block_index += 1;
                    current_block_index = Some(block_index);
                    out.push_str("event: content_block_start
");
                    out.push_str("data: ");
                    out.push_str(&json_string(&serde_json::json!({
                        "type":"content_block_start",
                        "index":block_index,
                        "content_block":{"type":"text","text":""}
                    })));
                    out.push_str("

");
                }
                let block_index = current_block_index.unwrap();
                out.push_str("event: content_block_delta
");
                out.push_str("data: ");
                out.push_str(&json_string(&serde_json::json!({
                    "type":"content_block_delta",
                    "index":block_index,
                    "delta":{"type":"text_delta","text":text}
                })));
                out.push_str("

");
            }
            CanonicalStreamEvent::ToolCallStart { call_id, name } => {
                saw_tool_use = true;
                if let Some(open_index) = current_block_index.take() {
                    out.push_str("event: content_block_stop
");
                    out.push_str("data: ");
                    out.push_str(&json_string(&serde_json::json!({
                        "type":"content_block_stop",
                        "index":open_index
                    })));
                    out.push_str("

");
                }
                let block_index = next_block_index;
                next_block_index += 1;
                tool_block_indices.insert(call_id.clone(), block_index);
                current_block_index = Some(block_index);
                out.push_str("event: content_block_start
");
                out.push_str("data: ");
                out.push_str(&json_string(&serde_json::json!({
                    "type":"content_block_start",
                    "index":block_index,
                    "content_block":{"type":"tool_use","id":call_id,"name":name,"input":{}}
                })));
                out.push_str("

");
            }
            CanonicalStreamEvent::ToolCallDelta {
                call_id,
                arguments_delta,
            } => {
                if let Some(block_index) = tool_block_indices.get(call_id) {
                    out.push_str("event: content_block_delta
");
                    out.push_str("data: ");
                    out.push_str(&json_string(&serde_json::json!({
                        "type":"content_block_delta",
                        "index":block_index,
                        "delta":{"type":"input_json_delta","partial_json":arguments_delta}
                    })));
                    out.push_str("

");
                }
            }
            CanonicalStreamEvent::ToolCallDone { call_id, .. } => {
                if let Some(block_index) = tool_block_indices.get(call_id) {
                    out.push_str("event: content_block_stop
");
                    out.push_str("data: ");
                    out.push_str(&json_string(&serde_json::json!({
                        "type":"content_block_stop",
                        "index":block_index
                    })));
                    out.push_str("

");
                    if current_block_index == Some(*block_index) {
                        current_block_index = None;
                    }
                }
            }
            CanonicalStreamEvent::Completed { finish_reason } => {
                if let Some(open_index) = current_block_index.take() {
                    out.push_str("event: content_block_stop
");
                    out.push_str("data: ");
                    out.push_str(&json_string(&serde_json::json!({
                        "type":"content_block_stop",
                        "index":open_index
                    })));
                    out.push_str("

");
                }
                let stop_reason = match finish_reason.as_deref() {
                    Some("tool_use") => "tool_use",
                    Some("max_tokens") => "max_tokens",
                    Some("stop_sequence") => "stop_sequence",
                    _ => {
                        if saw_tool_use {
                            "tool_use"
                        } else {
                            "end_turn"
                        }
                    }
                };
                final_stop_reason = stop_reason;
                out.push_str("event: message_delta
");
                out.push_str("data: ");
                out.push_str(&json_string(&serde_json::json!({
                    "type":"message_delta",
                    "delta":{"stop_reason":stop_reason,"stop_sequence":null},
                    "usage":{"output_tokens":0}
                })));
                out.push_str("\n\n");
                out.push_str("event: message_stop\n");
                out.push_str("data: {\"type\":\"message_stop\"}\n\n");
                message_stop_seen = true;
            }
            _ => {}
        }
    }
    eprintln!(
        "[anthropic-wire-summary] {}",
        serde_json::json!({
            "text_blocks": if text_deltas > 0 { 1 } else { 0 },
            "tool_use_blocks": tool_call_starts,
            "input_json_delta_count": tool_call_deltas,
            "final_stop_reason": final_stop_reason,
            "message_stop_seen": message_stop_seen,
        })
    );
    if tool_call_starts > 0 {
        eprintln!("[tool-path-stage] anthropic_tool_use_rendered");
    } else {
        eprintln!("[tool-path-stage] anthropic_end_turn_rendered");
    }
    out
}

fn json_string(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap()
}
