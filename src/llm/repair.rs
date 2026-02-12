use crate::llm::schema::{ActionSchema, SchemaViolation};
use crate::types::Action;
use serde_json::{Number, Value};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepairError {
    InvalidJson(String),
    SchemaViolation(Vec<SchemaViolation>),
    Deserialize(String),
}

impl fmt::Display for RepairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RepairError::InvalidJson(message) => write!(f, "invalid json: {message}"),
            RepairError::SchemaViolation(errors) => {
                let message = errors
                    .iter()
                    .map(|err| err.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                write!(f, "schema violation: {message}")
            }
            RepairError::Deserialize(message) => write!(f, "deserialize error: {message}"),
        }
    }
}

impl std::error::Error for RepairError {}

pub fn repair_action(content: &str, schema: &ActionSchema) -> Result<Action, RepairError> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(RepairError::InvalidJson("empty content".to_string()));
    }

    let value = parse_candidate(trimmed)
        .ok_or_else(|| RepairError::InvalidJson("no json object found".to_string()))?;
    let (value, _) = normalize_value(value);

    let value = match schema.validate_json(&value) {
        Ok(()) => value,
        Err(_) => {
            let (cleaned, _) = normalize_action_value(value);
            schema
                .validate_json(&cleaned)
                .map_err(RepairError::SchemaViolation)?;
            cleaned
        }
    };

    serde_json::from_value(value).map_err(|err| RepairError::Deserialize(err.to_string()))
}

fn parse_candidate(content: &str) -> Option<Value> {
    if let Ok(value) = serde_json::from_str::<Value>(content) {
        return Some(value);
    }

    if let Some(block) = extract_fenced_block(content) {
        if let Ok(value) = serde_json::from_str::<Value>(&block) {
            return Some(value);
        }
    }

    if let Some(block) = extract_balanced_json(content) {
        if let Ok(value) = serde_json::from_str::<Value>(&block) {
            return Some(value);
        }
    }

    None
}

fn extract_fenced_block(content: &str) -> Option<String> {
    let start = content.find("```")?;
    let after = &content[start + 3..];
    let end = after.find("```")?;
    let mut block = after[..end].trim().to_string();
    let lowered = block.to_lowercase();
    if lowered.starts_with("json") {
        if let Some(pos) = block.find('\n') {
            block = block[pos + 1..].to_string();
        } else {
            block = block
                .trim_start_matches(|c: char| c != '{' && c != '[')
                .to_string();
        }
    }
    Some(block.trim().to_string())
}

fn extract_balanced_json(content: &str) -> Option<String> {
    let mut start = None;
    let mut stack: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut escape = false;

    for (idx, ch) in content.char_indices() {
        if start.is_none() {
            if ch == '{' || ch == '[' {
                start = Some(idx);
                stack.push(ch);
            }
            continue;
        }

        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' | '[' => stack.push(ch),
            '}' | ']' => {
                if stack.pop().is_none() {
                    return None;
                }
                if stack.is_empty() {
                    let start_idx = start?;
                    return Some(content[start_idx..=idx].to_string());
                }
            }
            _ => {}
        }
    }

    None
}

fn normalize_value(value: Value) -> (Value, bool) {
    let mut repaired = false;
    let mut current = value;

    if let Value::String(text) = &current {
        if looks_like_json(text) {
            if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                current = parsed;
                repaired = true;
            }
        }
    }

    if let Value::Object(map) = current {
        if let Some(inner) = map.get("action").cloned() {
            if let Some(unwrapped) = unwrap_single_action(inner) {
                current = unwrapped;
                repaired = true;
            } else {
                current = Value::Object(map);
            }
        } else if let Some(inner) = map.get("actions").cloned() {
            if let Some(unwrapped) = unwrap_single_action(inner) {
                current = unwrapped;
                repaired = true;
            } else {
                current = Value::Object(map);
            }
        } else {
            current = Value::Object(map);
        }
    }

    current = match current {
        Value::Array(items) => {
            if items.len() == 1 {
                repaired = true;
                items.into_iter().next().unwrap()
            } else {
                Value::Array(items)
            }
        }
        other => other,
    };

    if let Value::Object(map) = current {
        let (value, changed) = normalize_action_value(Value::Object(map));
        current = value;
        repaired |= changed;
    }

    (current, repaired)
}

fn unwrap_single_action(value: Value) -> Option<Value> {
    match value {
        Value::Object(_) => Some(value),
        Value::Array(items) if items.len() == 1 => items.into_iter().next(),
        _ => None,
    }
}

fn normalize_action_value(value: Value) -> (Value, bool) {
    let Value::Object(mut map) = value else {
        return (value, false);
    };
    let mut repaired = false;

    if !map.contains_key("type") {
        if let Some(Value::String(action_type)) = map.remove("action_type") {
            map.insert("type".to_string(), Value::String(action_type));
            repaired = true;
        }
    }

    let action_type = match map.get("type") {
        Some(Value::String(value)) => value.to_string(),
        _ => return (Value::Object(map), repaired),
    };

    if action_type == "select" && !map.contains_key("value") {
        if let Some(value) = map.remove("option") {
            map.insert("value".to_string(), value);
            repaired = true;
        }
    }

    if action_type == "type" {
        if let Some(value) = map.get_mut("submit") {
            if !matches!(value, Value::Bool(_)) {
                if let Some(parsed) = coerce_bool(value) {
                    *value = Value::Bool(parsed);
                    repaired = true;
                }
            }
        }
    }

    if matches!(
        action_type.as_str(),
        "click" | "type" | "select" | "extract"
    ) {
        if let Some(value) = map.get_mut("id") {
            if !matches!(value, Value::String(_)) {
                if let Some(parsed) = coerce_string(value) {
                    *value = Value::String(parsed);
                    repaired = true;
                }
            }
        }
    }

    if action_type == "scroll" {
        for field in ["dx", "dy"] {
            if let Some(value) = map.get_mut(field) {
                if let Some(parsed) = coerce_i64(value) {
                    *value = Value::Number(Number::from(parsed));
                    repaired = true;
                }
            }
        }
    }

    if action_type == "wait" {
        if let Some(value) = map.get_mut("ms") {
            if let Some(parsed) = coerce_u64(value) {
                *value = Value::Number(Number::from(parsed));
                repaired = true;
            }
        }
    }

    let allowed = allowed_fields(&action_type);
    let keys: Vec<String> = map.keys().cloned().collect();
    for key in keys {
        if !allowed.contains(&key.as_str()) {
            map.remove(&key);
            repaired = true;
        }
    }

    (Value::Object(map), repaired)
}

fn allowed_fields(action_type: &str) -> &'static [&'static str] {
    match action_type {
        "click" => &["type", "id"],
        "type" => &["type", "id", "text", "submit"],
        "select" => &["type", "id", "value"],
        "scroll" => &["type", "dx", "dy"],
        "wait" => &["type", "ms"],
        "navigate" => &["type", "url"],
        "back" => &["type"],
        "extract" => &["type", "query", "id"],
        "done" => &["type", "summary"],
        _ => &["type"],
    }
}

fn looks_like_json(value: &str) -> bool {
    let trimmed = value.trim_start();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}

fn coerce_string(value: &Value) -> Option<String> {
    match value {
        Value::String(_) => None,
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn coerce_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(_) => None,
        Value::String(text) => match text.trim().to_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        Value::Number(number) => {
            if number == &Number::from(1) {
                Some(true)
            } else if number == &Number::from(0) {
                Some(false)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn coerce_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => {
            if number.as_i64().is_some() {
                None
            } else {
                number.as_f64().and_then(|num| {
                    if num.fract() == 0.0 {
                        Some(num as i64)
                    } else {
                        None
                    }
                })
            }
        }
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn coerce_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => {
            if number.as_u64().is_some() {
                None
            } else {
                number.as_f64().and_then(|num| {
                    if num.fract() == 0.0 && num >= 0.0 {
                        Some(num as u64)
                    } else {
                        None
                    }
                })
            }
        }
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> ActionSchema {
        ActionSchema::new()
    }

    #[test]
    fn repairs_code_fence() {
        let payload = "```json\n{\"type\":\"click\",\"id\":\"el_1\"}\n```";
        let action = repair_action(payload, &schema()).expect("repair action");
        assert_eq!(
            action,
            Action::Click {
                id: "el_1".to_string()
            }
        );
    }

    #[test]
    fn repairs_action_wrapper() {
        let payload = "{\"action\": {\"type\":\"scroll\",\"dx\": 0, \"dy\": 100}}";
        let action = repair_action(payload, &schema()).expect("repair action");
        assert_eq!(action, Action::Scroll { dx: 0, dy: 100 });
    }

    #[test]
    fn repairs_array_payload() {
        let payload = "[{\"type\":\"done\",\"summary\":\"ok\"}]";
        let action = repair_action(payload, &schema()).expect("repair action");
        assert_eq!(
            action,
            Action::Done {
                summary: "ok".to_string()
            }
        );
    }

    #[test]
    fn rejects_multi_action_array() {
        let payload =
            "[{\"type\":\"done\",\"summary\":\"one\"},{\"type\":\"done\",\"summary\":\"two\"}]";
        let err = repair_action(payload, &schema()).expect_err("expected error");
        match err {
            RepairError::SchemaViolation(_) => {}
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn repairs_extra_fields_and_option() {
        let payload = "{\"type\":\"select\",\"id\":\"el_2\",\"option\":\"blue\",\"reason\":\"\"}";
        let action = repair_action(payload, &schema()).expect("repair action");
        assert_eq!(
            action,
            Action::Select {
                id: "el_2".to_string(),
                value: "blue".to_string()
            }
        );
    }

    #[test]
    fn repairs_action_type_alias_and_submit() {
        let payload =
            "{\"action_type\":\"type\",\"id\":\"el_9\",\"text\":\"hi\",\"submit\":\"true\"}";
        let action = repair_action(payload, &schema()).expect("repair action");
        assert_eq!(
            action,
            Action::Type {
                id: "el_9".to_string(),
                text: "hi".to_string(),
                submit: Some(true),
            }
        );
    }

    #[test]
    fn repairs_embedded_json() {
        let payload = "Sure: {\"type\":\"click\",\"id\":\"el_3\"} thanks.";
        let action = repair_action(payload, &schema()).expect("repair action");
        assert_eq!(
            action,
            Action::Click {
                id: "el_3".to_string()
            }
        );
    }

    #[test]
    fn rejects_unrepairable_payload() {
        let payload = "{\"type\":\"click\"}";
        let err = repair_action(payload, &schema()).expect_err("expected error");
        match err {
            RepairError::SchemaViolation(_) => {}
            other => panic!("unexpected error: {other}"),
        }
    }
}
