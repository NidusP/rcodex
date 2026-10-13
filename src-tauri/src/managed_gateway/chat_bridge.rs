//! Responses request to Chat Completions, and the response back.
//!
//! Forwarding calls this module only when the route wire API is Chat
//! Completions. The Responses relay pipeline does not call it.
//!
//! Tool conversion follows the two protocol shapes. It does not branch on
//! model name. Function tools, custom tools, namespace children, and
//! tool_search become Chat function tools. Names and arguments are restored
//! on the way back. web_search has no Chat field and is dropped.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use super::protocol::ProtocolTerminal;
use super::responses_compat::normalize_routed_agent_messages;

const CHAT_TOOL_NAME_MAX: usize = 64;
const CUSTOM_INPUT_FIELD: &str = "input";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ChatToolKind {
    Function,
    Custom,
    ToolSearch,
    Namespace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ChatToolSpec {
    pub kind: ChatToolKind,
    pub name: String,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ChatToolMap {
    order: Vec<String>,
    specs: BTreeMap<String, ChatToolSpec>,
    definitions: BTreeMap<String, Value>,
    namespace_names: BTreeMap<(String, String), String>,
}

impl ChatToolMap {
    pub(super) fn spec(&self, chat_name: &str) -> Option<&ChatToolSpec> {
        self.specs.get(chat_name)
    }

    fn chat_name_for(&self, name: &str, namespace: Option<&str>) -> String {
        let Some(namespace) = namespace.filter(|value| !value.is_empty()) else {
            return name.to_string();
        };
        self.namespace_names
            .get(&(namespace.to_string(), name.to_string()))
            .cloned()
            .unwrap_or_else(|| flatten_namespace_tool_name(namespace, name))
    }

    fn definitions(&self) -> Vec<Value> {
        self.order
            .iter()
            .filter_map(|name| self.definitions.get(name).cloned())
            .collect()
    }

    fn add(&mut self, chat_name: String, spec: ChatToolSpec, definition: Value) {
        if chat_name.trim().is_empty() || self.specs.contains_key(&chat_name) {
            return;
        }
        if let Some(namespace) = spec.namespace.clone() {
            self.namespace_names
                .insert((namespace, spec.name.clone()), chat_name.clone());
        }
        self.order.push(chat_name.clone());
        self.specs.insert(chat_name.clone(), spec);
        self.definitions.insert(chat_name, definition);
    }

    fn add_response_tool(&mut self, tool: &Value) {
        match tool {
            Value::String(name) => self.add_custom_tool(&json!({"type": "custom", "name": name})),
            Value::Object(_) => match tool.get("type").and_then(Value::as_str).unwrap_or("") {
                kind if kind.contains("web_search") => {}
                "function" => self.add_function_tool(tool, None),
                "custom" | "apply_patch" => self.add_custom_tool(tool),
                "tool_search" => self.add_tool_search_tool(),
                "namespace" => self.add_namespace_tool(tool),
                _ => {}
            },
            _ => {}
        }
    }

    fn add_function_tool(&mut self, tool: &Value, namespace: Option<&str>) {
        let Some(original_name) = response_tool_name(tool) else {
            return;
        };
        let chat_name = match namespace {
            Some(namespace) => flatten_namespace_tool_name(namespace, &original_name),
            None => original_name.clone(),
        };
        let Some(definition) = chat_function_definition(tool, &chat_name) else {
            return;
        };
        self.add(
            chat_name,
            ChatToolSpec {
                kind: if namespace.is_some() {
                    ChatToolKind::Namespace
                } else {
                    ChatToolKind::Function
                },
                name: original_name,
                namespace: namespace.map(str::to_string),
            },
            definition,
        );
    }

    fn add_custom_tool(&mut self, tool: &Value) {
        let Some(name) = response_tool_name(tool) else {
            return;
        };
        let definition = json!({
            "type": "function",
            "function": {
                "name": name,
                "description": custom_tool_description(tool),
                "parameters": {
                    "type": "object",
                    "properties": {
                        CUSTOM_INPUT_FIELD: {
                            "type": "string",
                            "description": "Raw string input for the original custom tool. Preserve formatting exactly."
                        }
                    },
                    "required": [CUSTOM_INPUT_FIELD]
                }
            }
        });
        self.add(
            name.clone(),
            ChatToolSpec {
                kind: ChatToolKind::Custom,
                name,
                namespace: None,
            },
            definition,
        );
    }

    fn add_tool_search_tool(&mut self) {
        let definition = json!({
            "type": "function",
            "function": {
                "name": "tool_search",
                "description": "Search and load Codex tools, plugins, connectors, and MCP namespaces for the current task.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"},
                        "limit": {"type": "integer"}
                    },
                    "required": ["query"]
                }
            }
        });
        self.add(
            "tool_search".to_string(),
            ChatToolSpec {
                kind: ChatToolKind::ToolSearch,
                name: "tool_search".to_string(),
                namespace: None,
            },
            definition,
        );
    }

    fn add_namespace_tool(&mut self, tool: &Value) {
        let Some(namespace) = tool.get("name").and_then(Value::as_str) else {
            return;
        };
        let Some(children) = tool
            .get("tools")
            .or_else(|| tool.get("children"))
            .and_then(Value::as_array)
        else {
            return;
        };
        for child in children {
            match child.get("type").and_then(Value::as_str).unwrap_or("") {
                "function" => self.add_function_tool(child, Some(namespace)),
                "custom" | "apply_patch" => self.add_custom_tool(child),
                _ => {}
            }
        }
    }
}

#[derive(Debug)]
pub(super) struct ChatOutbound {
    pub body: Value,
    pub tools: ChatToolMap,
}

pub(super) fn response_request_to_chat(input: &Value) -> Result<Value, &'static str> {
    Ok(prepare_chat_outbound(input)?.body)
}

pub(super) fn prepare_chat_outbound(input: &Value) -> Result<ChatOutbound, &'static str> {
    if input
        .get("previous_response_id")
        .is_some_and(|value| !value.is_null())
        || input
            .get("mcp_servers")
            .is_some_and(|value| !value.is_null())
    {
        return Err("unsupported_chat_capability");
    }
    let model = input
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("model_required")?;
    let mut working = Map::new();
    if let Some(items) = input.get("input") {
        working.insert("input".to_string(), items.clone());
    }
    normalize_routed_agent_messages(&mut working)?;

    let mut tools = ChatToolMap::default();
    if let Some(declared) = input.get("tools").and_then(Value::as_array) {
        for tool in declared {
            tools.add_response_tool(tool);
        }
    }
    if let Some(items) = working.get("input") {
        collect_declared_tools(items, &mut tools);
    }

    let mut messages = Vec::new();
    if let Some(instructions) = input
        .get("instructions")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        messages.push(json!({"role": "system", "content": instructions}));
    }
    let mut pending_calls = Vec::new();
    match working.get("input").or_else(|| input.get("input")) {
        Some(Value::String(text)) if !text.is_empty() => {
            messages.push(json!({"role": "user", "content": text}));
        }
        Some(Value::Array(items)) => {
            let mut pending_images = Vec::new();
            for item in items {
                append_history_item(
                    item,
                    &mut messages,
                    &mut pending_calls,
                    &mut pending_images,
                    &tools,
                )?;
            }
            flush_pending_images(&mut messages, &mut pending_images);
        }
        _ => return Err("unsupported_chat_input"),
    }
    flush_pending_calls(&mut messages, &mut pending_calls);
    if messages.is_empty() {
        return Err("unsupported_chat_input");
    }

    let mut chat = Map::from_iter([
        ("model".to_string(), Value::String(model.to_string())),
        ("messages".to_string(), Value::Array(messages)),
        (
            "stream".to_string(),
            Value::Bool(
                input
                    .get("stream")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
        ),
    ]);
    if chat.get("stream") == Some(&Value::Bool(true)) {
        chat.insert("stream_options".to_string(), json!({"include_usage": true}));
    }
    let definitions = tools.definitions();
    if !definitions.is_empty() {
        chat.insert("tools".to_string(), Value::Array(definitions));
    }
    if let Some(choice) = input.get("tool_choice") {
        chat.insert("tool_choice".to_string(), map_tool_choice(choice, &tools)?);
    }
    if let Some(parallel) = input.get("parallel_tool_calls").and_then(Value::as_bool) {
        chat.insert("parallel_tool_calls".to_string(), Value::Bool(parallel));
    }
    if let Some(max_tokens) = input.get("max_output_tokens").and_then(Value::as_u64) {
        chat.insert("max_tokens".to_string(), Value::Number(max_tokens.into()));
    }
    if let Some(temperature) = input.get("temperature").and_then(Value::as_f64) {
        if let Some(number) = serde_json::Number::from_f64(temperature) {
            chat.insert("temperature".to_string(), Value::Number(number));
        }
    }
    apply_chat_reasoning(input, &mut chat);
    Ok(ChatOutbound {
        body: Value::Object(chat),
        tools,
    })
}

/// Map Codex Responses reasoning.effort onto the Chat Completions shape.
///
/// Chat uses a top-level reasoning_effort string. none is not one of those
/// values; the OpenAI-format control turns thinking off with
/// thinking.type = disabled instead. Other names pass through unchanged so
/// the upstream can apply its own aliases.
fn apply_chat_reasoning(input: &Value, chat: &mut Map<String, Value>) {
    let Some(effort) = chat_reasoning_effort(input) else {
        return;
    };
    if effort == "none" {
        chat.insert("thinking".to_string(), json!({"type": "disabled"}));
        return;
    }
    chat.insert(
        "reasoning_effort".to_string(),
        Value::String(effort.to_string()),
    );
}

fn chat_reasoning_effort(input: &Value) -> Option<&str> {
    input
        .get("reasoning")
        .and_then(|reasoning| reasoning.get("effort"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|effort| !effort.is_empty())
        .or_else(|| {
            input
                .get("reasoning_effort")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|effort| !effort.is_empty())
        })
}

fn collect_declared_tools(value: &Value, tools: &mut ChatToolMap) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_declared_tools(item, tools);
            }
        }
        Value::Object(object) => {
            if matches!(
                object.get("type").and_then(Value::as_str),
                Some("tool_search_output" | "additional_tools")
            ) {
                if let Some(declared) = object.get("tools").and_then(Value::as_array) {
                    for tool in declared {
                        tools.add_response_tool(tool);
                    }
                }
            }
            for child in object.values() {
                collect_declared_tools(child, tools);
            }
        }
        _ => {}
    }
}

fn append_history_item(
    item: &Value,
    messages: &mut Vec<Value>,
    pending_calls: &mut Vec<Value>,
    pending_images: &mut Vec<Value>,
    tools: &ChatToolMap,
) -> Result<(), &'static str> {
    let kind = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    match kind {
        "reasoning" | "additional_tools" => Ok(()),
        "compaction_trigger" => Err("unsupported_chat_history_item"),
        "message" => {
            flush_pending_calls(messages, pending_calls);
            flush_pending_images(messages, pending_images);
            let role = item
                .get("role")
                .and_then(Value::as_str)
                .ok_or("unsupported_chat_history_item")?;
            let role = match role {
                "developer" => "system",
                "system" | "user" | "assistant" => role,
                _ => return Err("unsupported_chat_history_item"),
            };
            let content = chat_message_content(item.get("content"))?;
            if content_is_empty(&content) {
                return Ok(());
            }
            messages.push(json!({"role": role, "content": content}));
            Ok(())
        }
        "function_call" => {
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .ok_or("unsupported_chat_history_item")?;
            let chat_name =
                tools.chat_name_for(name, item.get("namespace").and_then(Value::as_str));
            pending_calls.push(chat_tool_call(
                call_id_of(item),
                &chat_name,
                &function_arguments_text(item.get("arguments")),
            ));
            Ok(())
        }
        "custom_tool_call" => {
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .ok_or("unsupported_chat_history_item")?;
            let input_text = item.get("input").and_then(Value::as_str).unwrap_or("");
            let arguments = serde_json::to_string(&json!({"input": input_text}))
                .unwrap_or_else(|_| "{}".to_string());
            pending_calls.push(chat_tool_call(call_id_of(item), name, &arguments));
            Ok(())
        }
        "tool_search_call" => {
            pending_calls.push(chat_tool_call(
                call_id_of(item),
                "tool_search",
                &function_arguments_text(item.get("arguments")),
            ));
            Ok(())
        }
        "function_call_output" | "custom_tool_call_output" | "tool_search_output" => {
            flush_pending_calls(messages, pending_calls);
            let call_id = item
                .get("call_id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or("unsupported_chat_history_item")?;
            let split = match item.get("output") {
                None => SplitToolOutput {
                    text: item.to_string(),
                    images: Vec::new(),
                },
                output => split_tool_output(output)?,
            };
            pending_images.extend(split.images);
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": split.text
            }));
            Ok(())
        }
        _ => Err("unsupported_chat_history_item"),
    }
}

fn call_id_of(item: &Value) -> &str {
    item.get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("call_unknown")
}

struct SplitToolOutput {
    text: String,
    images: Vec<Value>,
}

fn split_tool_output(output: Option<&Value>) -> Result<SplitToolOutput, &'static str> {
    match output {
        None => Ok(SplitToolOutput {
            text: String::new(),
            images: Vec::new(),
        }),
        Some(Value::String(text)) => Ok(SplitToolOutput {
            text: text.clone(),
            images: Vec::new(),
        }),
        Some(Value::Array(parts)) => split_tool_parts(parts),
        Some(part @ Value::Object(_))
            if part
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| {
                    matches!(
                        kind,
                        "input_text" | "output_text" | "text" | "input_image" | "image_url"
                    )
                }) =>
        {
            split_tool_parts(std::slice::from_ref(part))
        }
        Some(other) => Ok(SplitToolOutput {
            text: other.to_string(),
            images: Vec::new(),
        }),
    }
}

fn split_tool_parts(parts: &[Value]) -> Result<SplitToolOutput, &'static str> {
    let mut text = String::new();
    let mut images = Vec::new();
    for part in parts {
        let kind = part.get("type").and_then(Value::as_str).unwrap_or("");
        if matches!(kind, "input_text" | "output_text" | "text") {
            let part_text = part.get("text").and_then(Value::as_str).unwrap_or("");
            if !text.is_empty() && !part_text.is_empty() {
                text.push('\n');
            }
            text.push_str(part_text);
        } else if matches!(kind, "input_image" | "image_url") {
            images.push(chat_image_part(part)?);
        } else if !kind.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&part.to_string());
        }
    }
    if !images.is_empty() {
        let note = if images.len() == 1 {
            "Image from this tool result follows."
        } else {
            "Images from this tool result follow."
        };
        if text.is_empty() {
            text.push_str(note);
        } else {
            text.push('\n');
            text.push_str(note);
        }
    }
    Ok(SplitToolOutput { text, images })
}

fn chat_image_part(part: &Value) -> Result<Value, &'static str> {
    let mut image = Map::new();
    image.insert("url".to_string(), Value::String(image_url_string(part)?));
    if let Some(detail) = part.get("detail").and_then(Value::as_str) {
        if matches!(detail, "auto" | "low" | "high") {
            image.insert("detail".to_string(), Value::String(detail.to_string()));
        }
    }
    Ok(json!({"type": "image_url", "image_url": Value::Object(image)}))
}

fn flush_pending_images(messages: &mut Vec<Value>, images: &mut Vec<Value>) {
    if images.is_empty() {
        return;
    }
    let mut content =
        vec![json!({"type": "text", "text": "Images returned by the preceding tool results."})];
    content.append(images);
    messages.push(json!({"role": "user", "content": Value::Array(content)}));
}

fn flush_pending_calls(messages: &mut Vec<Value>, pending_calls: &mut Vec<Value>) {
    if pending_calls.is_empty() {
        return;
    }
    messages.push(json!({
        "role": "assistant",
        "content": Value::Null,
        "tool_calls": std::mem::take(pending_calls),
    }));
}

fn chat_tool_call(id: &str, name: &str, arguments: &str) -> Value {
    json!({
        "id": id,
        "type": "function",
        "function": {"name": name, "arguments": arguments}
    })
}

fn response_tool_name(tool: &Value) -> Option<String> {
    tool.get("function")
        .and_then(|function| function.get("name"))
        .or_else(|| tool.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn chat_function_definition(tool: &Value, chat_name: &str) -> Option<Value> {
    if let Some(function) = tool.get("function").and_then(Value::as_object) {
        let mut function = function.clone();
        function.insert("name".to_string(), Value::String(chat_name.to_string()));
        let parameters = normalize_function_parameters(function.get("parameters"));
        function.insert("parameters".to_string(), parameters);
        if let Some(strict) = tool.get("strict").filter(|value| !value.is_null()) {
            function
                .entry("strict".to_string())
                .or_insert(strict.clone());
        }
        return Some(json!({"type": "function", "function": Value::Object(function)}));
    }
    let mut function = Map::new();
    function.insert("name".to_string(), Value::String(chat_name.to_string()));
    if let Some(description) = tool.get("description").filter(|value| !value.is_null()) {
        function.insert("description".to_string(), description.clone());
    }
    function.insert(
        "parameters".to_string(),
        normalize_function_parameters(tool.get("parameters")),
    );
    if let Some(strict) = tool.get("strict").filter(|value| !value.is_null()) {
        function.insert("strict".to_string(), strict.clone());
    }
    Some(json!({"type": "function", "function": Value::Object(function)}))
}

fn normalize_function_parameters(params: Option<&Value>) -> Value {
    let mut params = match params {
        Some(Value::Object(object)) => Value::Object(object.clone()),
        _ => json!({"type": "object", "properties": {}}),
    };
    if let Some(object) = params.as_object_mut() {
        if object.get("type").and_then(Value::as_str) != Some("object") {
            object.insert("type".to_string(), json!("object"));
        }
    }
    params
}

fn custom_tool_description(tool: &Value) -> String {
    format!("Original tool definition:\n```json\n{tool}\n```")
}

fn flatten_namespace_tool_name(namespace: &str, name: &str) -> String {
    let full_name = format!("{namespace}__{name}");
    if full_name.len() <= CHAT_TOOL_NAME_MAX {
        return full_name;
    }
    let suffix = format!("__{}", short_hash(full_name.as_bytes()));
    let prefix_len = CHAT_TOOL_NAME_MAX.saturating_sub(suffix.len());
    let mut prefix = String::new();
    for ch in full_name.chars() {
        if prefix.len() + ch.len_utf8() > prefix_len {
            break;
        }
        prefix.push(ch);
    }
    format!("{prefix}{suffix}")
}

fn short_hash(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn map_tool_choice(choice: &Value, tools: &ChatToolMap) -> Result<Value, &'static str> {
    match choice {
        Value::String(value) if matches!(value.as_str(), "none" | "auto" | "required") => {
            Ok(choice.clone())
        }
        Value::String(value) => named_tool_choice(value),
        Value::Object(object) => {
            let kind = object.get("type").and_then(Value::as_str).unwrap_or("");
            if kind.contains("web_search")
                || kind == "image_generation"
                || kind == "code_interpreter"
            {
                return Err("unsupported_tools");
            }
            if kind == "tool_search" {
                return named_tool_choice("tool_search");
            }
            let name = object
                .get("name")
                .and_then(Value::as_str)
                .or_else(|| {
                    object
                        .get("function")
                        .and_then(Value::as_object)
                        .and_then(|function| function.get("name"))
                        .and_then(Value::as_str)
                })
                .ok_or("unsupported_tools")?;
            if kind == "function" {
                let chat_name =
                    tools.chat_name_for(name, object.get("namespace").and_then(Value::as_str));
                return named_tool_choice(&chat_name);
            }
            named_tool_choice(name)
        }
        _ => Err("unsupported_tools"),
    }
}

fn named_tool_choice(name: &str) -> Result<Value, &'static str> {
    if name.contains("web_search") || name == "image_generation" || name == "code_interpreter" {
        return Err("unsupported_tools");
    }
    Ok(json!({"type": "function", "function": {"name": name}}))
}

fn chat_message_content(content: Option<&Value>) -> Result<Value, &'static str> {
    match content {
        Some(Value::String(text)) => Ok(Value::String(text.clone())),
        Some(Value::Array(parts)) => {
            let mut text = String::new();
            let mut rich = Vec::new();
            let mut saw_image = false;
            for part in parts {
                let kind = part.get("type").and_then(Value::as_str).unwrap_or("");
                if matches!(kind, "input_text" | "output_text" | "text") {
                    let part_text = part.get("text").and_then(Value::as_str).unwrap_or("");
                    if !text.is_empty() && !part_text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(part_text);
                    if !part_text.is_empty() {
                        rich.push(json!({"type": "text", "text": part_text}));
                    }
                } else if matches!(kind, "input_image" | "image_url") {
                    saw_image = true;
                    rich.push(
                        json!({"type": "image_url", "image_url": {"url": image_url_string(part)?}}),
                    );
                } else if !kind.is_empty() {
                    return Err("unsupported_chat_input");
                }
            }
            if saw_image {
                Ok(Value::Array(rich))
            } else {
                Ok(Value::String(text))
            }
        }
        None | Some(Value::Null) => Ok(Value::String(String::new())),
        _ => Err("unsupported_chat_input"),
    }
}

fn image_url_string(part: &Value) -> Result<String, &'static str> {
    if let Some(url) = part.get("image_url").and_then(Value::as_str) {
        return Ok(url.to_string());
    }
    if let Some(url) = part.pointer("/image_url/url").and_then(Value::as_str) {
        return Ok(url.to_string());
    }
    Err("unsupported_chat_input")
}

fn content_is_empty(content: &Value) -> bool {
    match content {
        Value::String(text) => text.is_empty(),
        Value::Array(parts) => parts.is_empty(),
        _ => false,
    }
}

fn function_arguments_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) if text.trim().is_empty() => "{}".to_string(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => serde_json::to_string(other).unwrap_or_else(|_| "{}".to_string()),
        None => "{}".to_string(),
    }
}

fn custom_input_from_arguments(arguments: &str) -> String {
    if arguments.trim().is_empty() {
        return String::new();
    }
    match serde_json::from_str::<Value>(arguments) {
        Ok(Value::Object(object)) => object
            .get(CUSTOM_INPUT_FIELD)
            .and_then(Value::as_str)
            .unwrap_or(arguments)
            .to_string(),
        _ => arguments.to_string(),
    }
}

fn tool_search_arguments(arguments: &str) -> Value {
    if arguments.trim().is_empty() {
        return json!({});
    }
    serde_json::from_str::<Value>(arguments)
        .ok()
        .filter(|value| value.is_object())
        .unwrap_or_else(|| json!({"query": arguments}))
}

pub(super) fn looks_like_unstructured_tool_text(text: &str) -> bool {
    text.contains("DSML")
}

pub(super) fn output_items_from_chat_message(
    message: &Value,
    tools: &ChatToolMap,
) -> Result<Vec<Value>, &'static str> {
    let content = message_text(message);
    let calls = message.get("tool_calls").and_then(Value::as_array);
    let legacy = message
        .get("function_call")
        .filter(|value| !value.is_null());
    let has_calls = calls.is_some_and(|calls| !calls.is_empty()) || legacy.is_some();
    if !has_calls && looks_like_unstructured_tool_text(&content) {
        return Err("chat_tool_call_not_structured");
    }
    let mut items = Vec::new();
    if !content.is_empty() {
        items.push(json!({
            "id": "msg_chat",
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{"type": "output_text", "text": content, "annotations": [], "logprobs": []}]
        }));
    }
    if let Some(calls) = calls {
        for call in calls {
            items.push(response_tool_call(call, tools)?);
        }
    } else if let Some(function_call) = legacy {
        items.push(response_tool_call(function_call, tools)?);
    }
    if items.is_empty() {
        return Err("invalid_upstream_response");
    }
    Ok(items)
}

fn message_text(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

pub(super) fn response_tool_call(call: &Value, tools: &ChatToolMap) -> Result<Value, &'static str> {
    let call_id = call
        .get("id")
        .or_else(|| call.get("call_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("call_unknown");
    let name = call
        .pointer("/function/name")
        .or_else(|| call.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or("invalid_upstream_response")?;
    let arguments = function_arguments_text(
        call.pointer("/function/arguments")
            .or_else(|| call.get("arguments")),
    );
    match tools.spec(name).map(|spec| &spec.kind) {
        Some(ChatToolKind::Custom) => Ok(json!({
            "id": format!("ctc_{call_id}"),
            "type": "custom_tool_call",
            "status": "completed",
            "call_id": call_id,
            "name": name,
            "input": custom_input_from_arguments(&arguments)
        })),
        Some(ChatToolKind::ToolSearch) => Ok(json!({
            "type": "tool_search_call",
            "call_id": call_id,
            "status": "completed",
            "execution": "client",
            "arguments": tool_search_arguments(&arguments)
        })),
        Some(ChatToolKind::Namespace) => {
            let spec = tools.spec(name).ok_or("invalid_upstream_response")?;
            let mut item = json!({
                "id": format!("fc_{call_id}"),
                "type": "function_call",
                "status": "completed",
                "call_id": call_id,
                "name": spec.name,
                "arguments": arguments
            });
            if let Some(namespace) = &spec.namespace {
                item["namespace"] = Value::String(namespace.clone());
            }
            Ok(item)
        }
        _ => Ok(json!({
            "id": format!("fc_{call_id}"),
            "type": "function_call",
            "status": "completed",
            "call_id": call_id,
            "name": name,
            "arguments": arguments
        })),
    }
}

pub(super) fn chat_finish_reason(reason: &str) -> Result<ProtocolTerminal, &'static str> {
    match reason {
        "stop" => Ok(ProtocolTerminal::Completed),
        "length" | "content_filter" => Ok(ProtocolTerminal::Incomplete),
        "tool_calls" | "function_call" => Ok(ProtocolTerminal::Completed),
        _ => Err("invalid_upstream_finish_reason"),
    }
}

pub(super) fn chat_usage_as_responses(value: Option<&Value>) -> Value {
    let Some(value) = value.filter(|value| value.is_object()) else {
        return Value::Null;
    };
    let mut output = value.clone();
    let object = output.as_object_mut().unwrap();
    for (from, to) in [
        ("prompt_tokens", "input_tokens"),
        ("completion_tokens", "output_tokens"),
        ("prompt_tokens_details", "input_tokens_details"),
        ("completion_tokens_details", "output_tokens_details"),
    ] {
        if let Some(value) = object.remove(from) {
            object.insert(to.to_string(), value);
        }
    }
    output
}
