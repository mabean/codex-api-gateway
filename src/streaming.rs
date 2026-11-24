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

// Text identity is local to an output item and content part. The maps join
// item_id-only events to output_index-only events using their shared item events.
#[derive(Default)]
struct CodexTextState {
    emitted: std::collections::HashMap<(usize, u64), String>,
    item_ids: std::collections::HashMap<String, usize>,
    output_indices: std::collections::HashMap<u64, usize>,
    current_item: usize,
    next_item: usize,
}

impl CodexTextState {
    fn fresh_item(&mut self) -> usize {
        self.next_item += 1;
        self.next_item
    }

    fn item(&mut self, event: &Value, item: Option<&Value>) -> usize {
        let id = event
            .get("item_id")
            .and_then(Value::as_str)
            .or_else(|| item.and_then(|item| item.get("id")).and_then(Value::as_str));
        let index = event.get("output_index").and_then(Value::as_u64);
        let known = id
            .and_then(|id| self.item_ids.get(id))
            .copied()
            .or_else(|| {
                index
                    .and_then(|index| self.output_indices.get(&index))
                    .copied()
            });
        let key = known.unwrap_or_else(|| {
            // A newly supplied identity of the other kind can name the current
            // item even when an added event was omitted. A different identity
            // of the same kind starts a distinct item.
            let different_item = (id.is_some()
                && self.item_ids.values().any(|key| *key == self.current_item))
                || (index.is_some()
                    && self
                        .output_indices
                        .values()
                        .any(|key| *key == self.current_item));
            if different_item {
                self.fresh_item()
            } else {
                self.current_item
            }
        });
        if let Some(id) = id {
            self.item_ids.insert(id.to_string(), key);
        }
        if let Some(index) = index {
            self.output_indices.insert(index, key);
        }
        self.current_item = key;
        key
    }

    fn delta(&mut self, item: usize, part: u64, delta: &str) {
        self.emitted
            .entry((item, part))
            .or_default()
            .push_str(delta);
    }

    fn finish(
        &mut self,
        item: usize,
        part: u64,
        full_text: &str,
    ) -> Result<Option<CanonicalStreamEvent>, ProxyError> {
        let emitted = self.emitted.entry((item, part)).or_default();
        let suffix = full_text.strip_prefix(emitted.as_str()).ok_or_else(|| {
            ProxyError::UpstreamProtocol {
                message: "upstream final text contradicts previously streamed text for the same content part".to_string(),
            }
        })?;
        let event = (!suffix.is_empty()).then(|| CanonicalStreamEvent::TextDelta {
            text: suffix.to_string(),
        });
        *emitted = full_text.to_string();
        Ok(event)
    }
}

fn protocol(message: &str) -> ProxyError {
    ProxyError::UpstreamProtocol {
        message: message.into(),
    }
}

// Both native forwarding and canonical translation wait for complete byte lines.
// Decode only afterwards, so transport chunk boundaries never split UTF-8.
fn take_sse_line(pending: &mut Vec<u8>) -> Option<Vec<u8>> {
    let end = pending.iter().position(|b| *b == b'\n')?;
    Some(pending.drain(..=end).collect())
}

// JSON-only fallback snapshots; the streaming parser never allocates this state.
#[derive(Default)]
struct NativeResponsesJsonItem {
    index: Option<u64>,
    final_item: Option<Value>,
    text: std::collections::BTreeMap<u64, String>,
}

#[derive(Default)]
struct NativeResponsesJsonState {
    identities: CodexTextState,
    items: std::collections::BTreeMap<usize, NativeResponsesJsonItem>,
}

impl NativeResponsesJsonState {
    fn event(&mut self, kind: &str, event: &Value) {
        if !matches!(
            kind,
            "response.output_item.added"
                | "response.output_item.done"
                | "response.output_text.delta"
                | "response.output_text.done"
        ) {
            return;
        }
        let native_item = event.get("item").filter(|item| item.is_object());
        let key = self.identities.item(event, native_item);
        let item = self.items.entry(key).or_default();
        if let Some(index) = event.get("output_index").and_then(Value::as_u64) {
            item.index = Some(index);
        }
        let part = event
            .get("content_index")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        match kind {
            "response.output_text.delta" => {
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    item.text.entry(part).or_default().push_str(delta);
                }
            }
            "response.output_text.done" => {
                if let Some(text) = event.get("text").and_then(Value::as_str) {
                    item.text.insert(part, text.into());
                }
            }
            "response.output_item.done" => {
                if let Some(snapshot) = native_item {
                    item.final_item = Some(snapshot.clone());
                    if let Some(content) = snapshot.get("content").and_then(Value::as_array) {
                        item.text.clear();
                        for (part, block) in content.iter().enumerate() {
                            if block["type"] == "output_text" {
                                if let Some(text) = block["text"].as_str() {
                                    item.text.insert(part as u64, text.into());
                                }
                            }
                        }
                    }
                }
            }
            _ => (),
        }
    }

    fn fallback(self) -> (Vec<Value>, String) {
        let mut items: Vec<_> = self.items.into_iter().collect();
        items.sort_by_key(|(key, item)| (item.index.unwrap_or(*key as u64), *key));
        let mut output = Vec::new();
        let mut text = String::new();
        for (_, item) in items {
            if let Some(snapshot) = item.final_item {
                output.push(snapshot);
            }
            for part in item.text.into_values() {
                text.push_str(&part);
            }
        }
        (output, text)
    }
}

#[derive(Default)]
/// Validate framing and terminal outcomes without projecting away native fields.
pub struct NativeResponsesSseParser {
    pending: Vec<u8>,
    frame: Vec<u8>,
    data: Vec<String>,
    event: Option<String>,
    pub terminal: bool,
    pending_error: Option<ProxyError>,
    json_state: Option<NativeResponsesJsonState>,
    json_response: Option<Value>,
}

impl NativeResponsesSseParser {
    pub fn for_json() -> Self {
        Self {
            json_state: Some(NativeResponsesJsonState::default()),
            ..Self::default()
        }
    }

    pub fn into_response(mut self) -> Result<Value, ProxyError> {
        self.check_error()?;
        let mut response = self
            .json_response
            .take()
            .ok_or_else(|| protocol("upstream did not complete a response"))?;
        let object = response.as_object_mut().expect("validated native response");
        let native_output = object.contains_key("output");
        let (fallback_output, fallback_text) = self.json_state.unwrap_or_default().fallback();
        if !object.contains_key("output") && !fallback_output.is_empty() {
            object.insert("output".into(), Value::Array(fallback_output));
        }
        object
            .entry("object")
            .or_insert_with(|| Value::String("response".into()));
        if !object.contains_key("output_text") {
            let mut text = String::new();
            let mut found = false;
            if let Some(output) = object
                .get("output")
                .filter(|_| native_output)
                .and_then(Value::as_array)
            {
                for item in output {
                    if let Some(content) = item.get("content").and_then(Value::as_array) {
                        for block in content {
                            if block["type"] == "output_text" {
                                if let Some(value) = block["text"].as_str() {
                                    found = true;
                                    text.push_str(value);
                                }
                            }
                        }
                    }
                }
            }
            object.insert(
                "output_text".into(),
                Value::String(if found { text } else { fallback_text }),
            );
        }
        Ok(response)
    }

    pub fn check_error(&mut self) -> Result<(), ProxyError> {
        self.pending_error.take().map_or(Ok(()), Err)
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<u8>, ProxyError> {
        self.check_error()?;
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        while !self.terminal {
            let Some(raw) = take_sse_line(&mut self.pending) else {
                break;
            };
            let result = self.line(&raw);
            match result {
                Ok(Some(frame)) => out.extend(frame),
                Ok(None) => (),
                Err(error) if out.is_empty() => return Err(error),
                Err(error) => {
                    self.pending_error = Some(error);
                    return Ok(out);
                }
            }
        }
        if !self.terminal && self.frame.len() + self.pending.len() > 1024 * 1024 {
            let error = protocol("upstream SSE frame exceeds 1 MiB limit");
            if out.is_empty() {
                return Err(error);
            }
            self.pending_error = Some(error);
        }
        Ok(out)
    }

    fn line(&mut self, raw: &[u8]) -> Result<Option<Vec<u8>>, ProxyError> {
        self.frame.extend_from_slice(raw);
        if self.frame.len() > 1024 * 1024 {
            return Err(protocol("upstream SSE frame exceeds 1 MiB limit"));
        }
        let line = std::str::from_utf8(raw)
            .map_err(|_| protocol("invalid upstream UTF-8"))?
            .trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            return self.dispatch().map(Some);
        }
        if let Some(data) = line.strip_prefix("data:") {
            self.data
                .push(data.strip_prefix(' ').unwrap_or(data).into());
        } else if let Some(event) = line.strip_prefix("event:") {
            self.event = Some(event.trim_start_matches(' ').into());
        } else if !line.starts_with(':') && !line.starts_with("id:") && !line.starts_with("retry:")
        {
            return Err(protocol("malformed upstream SSE field"));
        }
        Ok(None)
    }

    fn dispatch(&mut self) -> Result<Vec<u8>, ProxyError> {
        let raw = std::mem::take(&mut self.frame);
        let event_name = self.event.take();
        if self.data.is_empty() {
            return Ok(raw);
        }
        let data = std::mem::take(&mut self.data).join("\n");
        let mut event: Value =
            serde_json::from_str(&data).map_err(|_| protocol("malformed upstream SSE JSON"))?;
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol("upstream event missing type"))?
            .to_owned();
        if event_name.as_deref().is_some_and(|name| name != kind) {
            return Err(protocol("upstream SSE event/type mismatch"));
        }
        if let Some(state) = &mut self.json_state {
            state.event(&kind, &event);
        }
        let expected = match kind.as_str() {
            "response.completed" => Some("completed"),
            "response.incomplete" => Some("incomplete"),
            "response.failed" => Some("failed"),
            "response.done" => Some(
                event
                    .pointer("/response/status")
                    .and_then(Value::as_str)
                    .filter(|s| matches!(*s, "completed" | "incomplete" | "failed"))
                    .ok_or_else(|| protocol("legacy terminal missing valid status"))?,
            ),
            _ => None,
        };
        if let Some(status) = expected {
            if !event.get("response").is_some_and(Value::is_object) {
                return Err(protocol("upstream terminal missing response object"));
            }
            if event
                .pointer("/response/status")
                .is_some_and(|s| s.as_str() != Some(status))
            {
                return Err(protocol("contradictory upstream terminal status"));
            }
            self.terminal = true;
            if self.json_state.is_some() {
                if status == "failed" {
                    return Err(protocol("upstream response failed"));
                }
                let mut response = event["response"].clone();
                response
                    .as_object_mut()
                    .expect("validated response")
                    .entry("status")
                    .or_insert_with(|| Value::String(status.into()));
                self.json_response = Some(response);
            }
            if kind == "response.done" {
                let normalized = format!("response.{status}");
                event["type"] = Value::String(normalized.clone());
                // Preserve ancillary SSE fields while replacing every event line.
                let text = std::str::from_utf8(&raw).expect("validated SSE UTF-8");
                let mut prefix = String::new();
                for line in text.lines() {
                    if !line.is_empty() && !line.starts_with("data:") && !line.starts_with("event:")
                    {
                        prefix.push_str(line);
                        prefix.push('\n');
                    }
                }
                return Ok(format!("{prefix}event: {normalized}\ndata: {event}\n\n").into_bytes());
            }
        } else if kind == "error" {
            self.terminal = true;
        }
        Ok(raw)
    }

    pub fn finish(&mut self) -> Result<Vec<u8>, ProxyError> {
        self.check_error()?;
        if self.terminal {
            return Ok(Vec::new());
        }
        // Like the canonical parser, dispatch a final complete JSON event at
        // EOF even when the upstream omits its final line/frame delimiters.
        let pending = std::mem::take(&mut self.pending);
        let mut frame = Vec::new();
        if !pending.is_empty() {
            if let Some(emitted) = self.line(&pending)? {
                frame = emitted;
            }
        }
        if !self.terminal {
            frame = self.dispatch()?;
        }
        if !self.terminal {
            return Err(protocol("upstream stream ended without terminal"));
        }
        // Ordinary SSE consumers need a blank line to decode this event. Keep
        // all original bytes and append only the missing boundary.
        if !frame.ends_with(b"\n\n") && !frame.ends_with(b"\r\n\r\n") {
            if !frame.ends_with(b"\n") {
                frame.push(b'\n');
            }
            frame.push(b'\n');
        }
        Ok(frame)
    }
}

/// Incremental SSE framing retains bytes until complete lines, including split UTF-8.
/// A single frame and the complete canonical response each have explicit limits.
#[derive(Default)]
pub struct CodexSseParser {
    pending: Vec<u8>,
    data: Vec<String>,
    frame_bytes: usize,
    total_bytes: usize,
    terminal: bool,
    saw_message_start: bool,
    text_state: CodexTextState,
    tool_state: std::collections::HashMap<String, (String, String)>,
    item_call_ids: std::collections::HashMap<String, String>,
    saw_tool_use: bool,
    finished_tools: std::collections::HashSet<String>,
    pending_error: Option<ProxyError>,
}
impl CodexSseParser {
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<CanonicalStreamEvent>, ProxyError> {
        self.check_error()?;
        self.total_bytes += bytes.len();
        if self.total_bytes > 16 * 1024 * 1024 {
            return Err(protocol("upstream stream exceeds 16 MiB limit"));
        }
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(raw) = take_sse_line(&mut self.pending) {
            let line = std::str::from_utf8(&raw)
                .map_err(|_| protocol("invalid upstream UTF-8"))?
                .trim_end_matches(['\r', '\n']);
            if let Err(error) = self.line(line, &mut out) {
                if out.is_empty() {
                    return Err(error);
                }
                self.pending_error = Some(error);
                return Ok(out);
            }
        }
        if self.pending.len() + self.frame_bytes > 1024 * 1024 {
            return Err(protocol("upstream SSE frame exceeds 1 MiB limit"));
        }
        Ok(out)
    }
    fn line(&mut self, line: &str, out: &mut Vec<CanonicalStreamEvent>) -> Result<(), ProxyError> {
        if line.is_empty() {
            return self.dispatch(out);
        }
        if let Some(data) = line.strip_prefix("data:") {
            self.frame_bytes += data.len();
            if self.frame_bytes > 1024 * 1024 {
                return Err(protocol("upstream SSE frame exceeds 1 MiB limit"));
            }
            self.data
                .push(data.strip_prefix(' ').unwrap_or(data).to_string());
        } else if !line.starts_with(':')
            && !line.starts_with("event:")
            && !line.starts_with("id:")
            && !line.starts_with("retry:")
        {
            return Err(protocol("malformed upstream SSE field"));
        }
        Ok(())
    }
    fn dispatch(&mut self, out: &mut Vec<CanonicalStreamEvent>) -> Result<(), ProxyError> {
        if self.data.is_empty() {
            return Ok(());
        }
        let data = self.data.join("\n");
        self.data.clear();
        self.frame_bytes = 0;
        if data == "[DONE]" {
            return if self.terminal {
                Ok(())
            } else {
                Err(protocol("upstream DONE without terminal"))
            };
        }
        if self.terminal {
            return Err(protocol("upstream event after terminal"));
        }
        let event: Value =
            serde_json::from_str(&data).map_err(|_| protocol("malformed upstream SSE JSON"))?;
        out.extend(self.event(event)?);
        Ok(())
    }
    pub fn finish(&mut self) -> Result<Vec<CanonicalStreamEvent>, ProxyError> {
        self.check_error()?;
        let raw = std::mem::take(&mut self.pending);
        if !raw.is_empty() {
            let line = std::str::from_utf8(&raw)
                .map_err(|_| protocol("invalid upstream UTF-8"))?
                .trim_end_matches('\r');
            self.line(line, &mut Vec::new())?;
        }
        let mut out = Vec::new();
        self.dispatch(&mut out)?;
        if !self.terminal {
            return Err(protocol("upstream stream ended without terminal"));
        }
        Ok(out)
    }
    pub fn check_error(&mut self) -> Result<(), ProxyError> {
        self.pending_error.take().map_or(Ok(()), Err)
    }
    fn event(&mut self, event: Value) -> Result<Vec<CanonicalStreamEvent>, ProxyError> {
        let mut events = Vec::new();
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol("upstream event missing type"))?;
        for (kind, field) in [
            ("response.output_text.delta", "delta"),
            ("response.output_text.done", "text"),
            ("response.function_call_arguments.delta", "delta"),
            ("response.function_call_arguments.done", "arguments"),
        ] {
            if event_type == kind && event.get(field).and_then(Value::as_str).is_none() {
                return Err(protocol("upstream event missing required string"));
            }
        }
        match event_type {
            "response.output_item.added" | "response.output_item.done" => {
                if let Some(item) = event.get("item") {
                    match item.get("type").and_then(Value::as_str) {
                        Some("message") => {
                            if !self.saw_message_start {
                                self.saw_message_start = true;
                                events.push(CanonicalStreamEvent::MessageStart);
                            }
                            let text_item = self.text_state.item(&event, Some(item));
                            if event_type == "response.output_item.done" {
                                if let Some(content) = item.get("content").and_then(Value::as_array)
                                {
                                    for (part_index, part) in content.iter().enumerate() {
                                        if let Some(text) = part.get("text").and_then(Value::as_str)
                                        {
                                            if let Some(delta) = self.text_state.finish(
                                                text_item,
                                                part_index as u64,
                                                text,
                                            )? {
                                                events.push(delta);
                                            }
                                        }
                                    }
                                }
                                // Identifier-free legacy events belong to the next item after done.
                                self.text_state.current_item = self.text_state.fresh_item();
                            }
                        }
                        Some("function_call") => {
                            self.saw_tool_use = true;
                            let call_id = item
                                .get("call_id")
                                .or_else(|| item.get("id"))
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            if let Some(item_id) = item.get("id").and_then(Value::as_str) {
                                self.item_call_ids
                                    .insert(item_id.to_string(), call_id.clone());
                            }
                            let name = item
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            if call_id.is_empty() || name.is_empty() {
                                return Err(protocol(
                                    "upstream tool item missing call identity or name",
                                ));
                            }
                            if self
                                .tool_state
                                .get(&call_id)
                                .is_some_and(|(existing, _)| existing != &name)
                            {
                                return Err(protocol(
                                    "upstream final tool name contradicts streamed name",
                                ));
                            }
                            let arguments = item
                                .get("arguments")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            let status = item
                                .get("status")
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            let is_new = !self.tool_state.contains_key(&call_id);
                            self.tool_state
                                .entry(call_id.clone())
                                .or_insert((name.clone(), String::new()));
                            if is_new {
                                events.push(CanonicalStreamEvent::ToolCallStart {
                                    call_id: call_id.clone(),
                                    name: name.clone(),
                                });
                            }
                            if item.get("arguments").and_then(Value::as_str).is_some() {
                                let (_, args) = self.tool_state.get_mut(&call_id).unwrap();
                                if self.finished_tools.contains(&call_id) && *args != arguments {
                                    return Err(protocol(
                                        "upstream changed finalized tool arguments",
                                    ));
                                }
                                let suffix = arguments.strip_prefix(args.as_str()).ok_or_else(|| protocol("upstream final tool arguments contradict streamed arguments"))?;
                                if !suffix.is_empty() {
                                    events.push(CanonicalStreamEvent::ToolCallDelta {
                                        call_id: call_id.clone(),
                                        arguments_delta: suffix.to_string(),
                                    });
                                }
                                *args = arguments.clone();
                            }
                            if (status == "completed" || event_type == "response.output_item.done")
                                && self.finished_tools.insert(call_id.clone())
                            {
                                let final_arguments = self
                                    .tool_state
                                    .get(&call_id)
                                    .map(|(_, args)| {
                                        if args.is_empty() {
                                            arguments.clone()
                                        } else {
                                            args.clone()
                                        }
                                    })
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
                    .get("call_id")
                    .or_else(|| event.get("item_id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let call_id = self.item_call_ids.get(&call_id).cloned().unwrap_or(call_id);
                let delta = event
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let (_, args) = self
                    .tool_state
                    .get_mut(&call_id)
                    .ok_or_else(|| protocol("tool delta before tool start"))?;
                if self.finished_tools.contains(&call_id) {
                    return Err(protocol("tool delta after tool done"));
                }
                args.push_str(&delta);
                if !delta.is_empty() {
                    self.saw_tool_use = true;
                    events.push(CanonicalStreamEvent::ToolCallDelta {
                        call_id,
                        arguments_delta: delta,
                    });
                }
            }
            "response.function_call_arguments.done" => {
                let id = event
                    .get("call_id")
                    .or_else(|| event.get("item_id"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| protocol("tool done missing identity"))?;
                let call_id = self.item_call_ids.get(id).map(String::as_str).unwrap_or(id);
                let (name, _) = self
                    .tool_state
                    .get(call_id)
                    .ok_or_else(|| protocol("tool done before tool start"))?;
                events.extend(self.event(serde_json::json!({"type":"response.output_item.done","item":{"type":"function_call","call_id":call_id,"name":name,"arguments":event["arguments"]}}))?);
            }
            "response.output_text.delta" => {
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    let item = self.text_state.item(&event, None);
                    let part = event
                        .get("content_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    self.text_state.delta(item, part, delta);
                    events.push(CanonicalStreamEvent::TextDelta {
                        text: delta.to_string(),
                    });
                }
            }
            "response.output_text.done" => {
                if let Some(text) = event.get("text").and_then(Value::as_str) {
                    let item = self.text_state.item(&event, None);
                    let part = event
                        .get("content_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    if let Some(delta) = self.text_state.finish(item, part, text)? {
                        events.push(delta);
                    }
                    events.push(CanonicalStreamEvent::TextDone {
                        full_text: text.to_string(),
                    });
                }
            }
            "response.completed" | "response.incomplete" => {
                if self.terminal {
                    return Err(protocol("duplicate upstream terminal"));
                }
                self.terminal = true;
                let limited = event_type == "response.incomplete";
                if let Some(status) = event.pointer("/response/status").and_then(Value::as_str) {
                    if status != if limited { "incomplete" } else { "completed" } {
                        return Err(protocol("upstream terminal status contradicts event type"));
                    }
                }
                if limited
                    && event
                        .pointer("/response/incomplete_details/reason")
                        .and_then(Value::as_str)
                        != Some("max_output_tokens")
                {
                    return Err(protocol("unsupported incomplete reason"));
                }
                if let Some(output) = event.pointer("/response/output").and_then(Value::as_array) {
                    for (index, item) in output.iter().enumerate() {
                        events.extend(self.event(serde_json::json!({"type":"response.output_item.done","output_index":index,"item":item}))?);
                    }
                }
                if !limited
                    && self
                        .tool_state
                        .keys()
                        .any(|id| !self.finished_tools.contains(id))
                {
                    return Err(protocol("upstream completed with unfinished tool call"));
                }
                if let Some(usage) = event.get("response").and_then(|r| r.get("usage")) {
                    let input_tokens = usage
                        .get("input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0)
                        .try_into()
                        .map_err(|_| protocol("input usage exceeds supported counter range"))?;
                    let output_tokens = usage
                        .get("output_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0)
                        .try_into()
                        .map_err(|_| protocol("output usage exceeds supported counter range"))?;
                    events.push(CanonicalStreamEvent::Usage {
                        input_tokens,
                        output_tokens,
                    });
                }
                events.push(CanonicalStreamEvent::Completed {
                    finish_reason: Some(if limited {
                        "max_tokens".to_string()
                    } else if self.saw_tool_use {
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
        Ok(events)
    }
}
pub fn parse_codex_sse_to_events(
    response_text: &str,
) -> Result<Vec<CanonicalStreamEvent>, ProxyError> {
    let mut parser = CodexSseParser::default();
    let mut out = parser.feed(response_text.as_bytes())?;
    out.extend(parser.finish()?);
    Ok(out)
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
    let mut tool_indices = std::collections::HashMap::<String, u32>::new();
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
                let next_index = tool_indices.len() as u32;
                let tool_index = *tool_indices.entry(call_id.clone()).or_insert(next_index);
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
                                index: tool_index,
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
                let next_index = tool_indices.len() as u32;
                let tool_index = *tool_indices.entry(call_id.clone()).or_insert(next_index);
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
                                index: tool_index,
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
                        finish_reason: Some(match finish_reason.as_deref() {
                            Some("tool_use") => "tool_calls".to_string(),
                            Some(reason) => reason.to_string(),
                            None => "stop".to_string(),
                        }),
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

#[allow(dead_code)]
fn verbose_tracing_enabled() -> bool {
    matches!(
        std::env::var("CODEX_PROXY_VERBOSE").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

/// Stateful renderer shared by buffered callers and the pull-driven bridge.
pub struct AnthropicSseEncoder {
    model: String,
    started: bool,
    next: u32,
    text: Option<u32>,
    tools: std::collections::HashMap<String, u32>,
    open: std::collections::BTreeSet<u32>,
    input: u32,
    output: u32,
}
fn frame(value: Value) -> String {
    format!(
        "event: {}\ndata: {}\n\n",
        value["type"].as_str().unwrap(),
        value
    )
}
impl AnthropicSseEncoder {
    pub fn new(model: &str) -> Self {
        Self {
            model: model.into(),
            started: false,
            next: 0,
            text: None,
            tools: Default::default(),
            open: Default::default(),
            input: 0,
            output: 0,
        }
    }
    pub fn encode(&mut self, events: &[CanonicalStreamEvent]) -> String {
        use serde_json::json;
        let mut out = String::new();
        if !events.is_empty() && !self.started {
            self.started = true;
            out += &frame(
                json!({"type":"message_start","message":{"id":format!("msg_{}",uuid::Uuid::new_v4().simple()),"type":"message","role":"assistant","content":[],"model":self.model,"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":0,"output_tokens":0}}}),
            );
        }
        for event in events {
            match event {
                CanonicalStreamEvent::TextDelta { text } => {
                    let index = if let Some(index) = self.text {
                        index
                    } else {
                        let index = self.next;
                        self.next += 1;
                        self.text = Some(index);
                        self.open.insert(index);
                        out += &frame(
                            json!({"type":"content_block_start","index":index,"content_block":{"type":"text","text":""}}),
                        );
                        index
                    };
                    out += &frame(
                        json!({"type":"content_block_delta","index":index,"delta":{"type":"text_delta","text":text}}),
                    );
                }
                CanonicalStreamEvent::ToolCallStart { call_id, name } => {
                    if self.tools.contains_key(call_id) {
                        continue;
                    }
                    if let Some(index) = self.text.take() {
                        if self.open.remove(&index) {
                            out += &frame(json!({"type":"content_block_stop","index":index}));
                        }
                    }
                    let index = self.next;
                    self.next += 1;
                    self.tools.insert(call_id.clone(), index);
                    self.open.insert(index);
                    out += &frame(
                        json!({"type":"content_block_start","index":index,"content_block":{"type":"tool_use","id":call_id,"name":name,"input":{}}}),
                    );
                }
                CanonicalStreamEvent::ToolCallDelta {
                    call_id,
                    arguments_delta,
                } => {
                    if let Some(index) = self.tools.get(call_id) {
                        out += &frame(
                            json!({"type":"content_block_delta","index":index,"delta":{"type":"input_json_delta","partial_json":arguments_delta}}),
                        );
                    }
                }
                CanonicalStreamEvent::ToolCallDone { call_id, .. } => {
                    if let Some(index) = self.tools.get(call_id) {
                        if self.open.remove(index) {
                            out += &frame(json!({"type":"content_block_stop","index":index}));
                        }
                    }
                }
                CanonicalStreamEvent::Usage {
                    input_tokens,
                    output_tokens,
                } => {
                    self.input = *input_tokens;
                    self.output = *output_tokens;
                }
                CanonicalStreamEvent::Completed { finish_reason } => {
                    for index in std::mem::take(&mut self.open) {
                        out += &frame(json!({"type":"content_block_stop","index":index}));
                    }
                    let reason = match finish_reason.as_deref() {
                        Some("tool_use") => "tool_use",
                        Some("max_tokens") => "max_tokens",
                        _ => "end_turn",
                    };
                    out += &frame(
                        json!({"type":"message_delta","delta":{"stop_reason":reason,"stop_sequence":null},"usage":{"input_tokens":self.input,"output_tokens":self.output}}),
                    );
                    out += &frame(json!({"type":"message_stop"}));
                }
                _ => {}
            }
        }
        out
    }
}
pub fn render_anthropic_sse(events: &[CanonicalStreamEvent], model: &str) -> String {
    AnthropicSseEncoder::new(model).encode(events)
}
