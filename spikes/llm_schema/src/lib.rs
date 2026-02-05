use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Click { id: String },
    Type { id: String, text: String, submit: bool },
    Select { id: String, option: String },
    Scroll { dx: i32, dy: i32 },
    Wait { ms: u64 },
    Navigate { url: String },
    Back,
    Extract { query: String },
    Done { summary: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidJson,
    MissingField,
    InvalidType,
    UnknownAction,
    UnknownField,
    ConstraintViolation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationError {
    pub code: ErrorCode,
    pub message: String,
    pub path: String,
}

#[derive(Debug, Error)]
#[error("schema validation failed")]
pub struct SchemaError {
    pub errors: Vec<ValidationError>,
}

#[derive(Debug, Clone)]
pub struct ValidationOptions {
    pub allow_insecure: bool,
}

impl Default for ValidationOptions {
    fn default() -> Self {
        Self {
            allow_insecure: false,
        }
    }
}

pub fn parse_action_str(input: &str, options: &ValidationOptions) -> Result<Action, SchemaError> {
    let value: Value = serde_json::from_str(input).map_err(|err| SchemaError {
        errors: vec![ValidationError {
            code: ErrorCode::InvalidJson,
            message: err.to_string(),
            path: "$".to_string(),
        }],
    })?;
    parse_action_value(&value, options)
}

pub fn parse_action_value(value: &Value, options: &ValidationOptions) -> Result<Action, SchemaError> {
    let mut errors = Vec::new();
    let object = match value.as_object() {
        Some(object) => object,
        None => {
            errors.push(ValidationError {
                code: ErrorCode::InvalidType,
                message: "Action must be a JSON object".to_string(),
                path: "$".to_string(),
            });
            return Err(SchemaError { errors });
        }
    };

    let action_type = match object.get("type") {
        Some(Value::String(value)) => value.as_str(),
        Some(_) => {
            errors.push(ValidationError {
                code: ErrorCode::InvalidType,
                message: "Field 'type' must be a string".to_string(),
                path: "$.type".to_string(),
            });
            return Err(SchemaError { errors });
        }
        None => {
            errors.push(ValidationError {
                code: ErrorCode::MissingField,
                message: "Field 'type' is required".to_string(),
                path: "$.type".to_string(),
            });
            return Err(SchemaError { errors });
        }
    };

    match action_type {
        "click" => parse_click(object, &mut errors),
        "type" => parse_type(object, &mut errors),
        "select" => parse_select(object, &mut errors),
        "scroll" => parse_scroll(object, &mut errors),
        "wait" => parse_wait(object, &mut errors),
        "navigate" => parse_navigate(object, options, &mut errors),
        "back" => parse_back(object, &mut errors),
        "extract" => parse_extract(object, &mut errors),
        "done" => parse_done(object, &mut errors),
        other => {
            errors.push(ValidationError {
                code: ErrorCode::UnknownAction,
                message: format!("Unknown action type '{other}'"),
                path: "$.type".to_string(),
            });
            return Err(SchemaError { errors });
        }
    }
}

fn parse_click(object: &Map<String, Value>, errors: &mut Vec<ValidationError>) -> Result<Action, SchemaError> {
    check_unknown_fields(object, &["type", "id"], errors);
    let id = require_string(object, "id", errors);
    enforce_non_empty("id", id.as_deref(), errors);
    to_action(errors, || Action::Click {
        id: id.unwrap_or_default(),
    })
}

fn parse_type(object: &Map<String, Value>, errors: &mut Vec<ValidationError>) -> Result<Action, SchemaError> {
    check_unknown_fields(object, &["type", "id", "text", "submit"], errors);
    let id = require_string(object, "id", errors);
    let text = require_string(object, "text", errors);
    let submit = optional_bool(object, "submit", errors).unwrap_or(false);
    enforce_non_empty("id", id.as_deref(), errors);
    if let Some(text) = text.as_deref() {
        let len = text.chars().count();
        if len > 2000 {
            errors.push(ValidationError {
                code: ErrorCode::ConstraintViolation,
                message: format!("Field 'text' exceeds 2000 characters (got {len})"),
                path: "$.text".to_string(),
            });
        }
    }
    to_action(errors, || Action::Type {
        id: id.unwrap_or_default(),
        text: text.unwrap_or_default(),
        submit,
    })
}

fn parse_select(object: &Map<String, Value>, errors: &mut Vec<ValidationError>) -> Result<Action, SchemaError> {
    check_unknown_fields(object, &["type", "id", "option"], errors);
    let id = require_string(object, "id", errors);
    let option = require_string(object, "option", errors);
    enforce_non_empty("id", id.as_deref(), errors);
    enforce_non_empty("option", option.as_deref(), errors);
    to_action(errors, || Action::Select {
        id: id.unwrap_or_default(),
        option: option.unwrap_or_default(),
    })
}

fn parse_scroll(object: &Map<String, Value>, errors: &mut Vec<ValidationError>) -> Result<Action, SchemaError> {
    check_unknown_fields(object, &["type", "dx", "dy"], errors);
    let dx = require_i32(object, "dx", errors);
    let dy = require_i32(object, "dy", errors);
    for (field, value) in [("dx", dx), ("dy", dy)] {
        if let Some(value) = value {
            if value < -2000 || value > 2000 {
                errors.push(ValidationError {
                    code: ErrorCode::ConstraintViolation,
                    message: format!("Field '{field}' must be between -2000 and 2000"),
                    path: format!("$.{field}"),
                });
            }
        }
    }
    to_action(errors, || Action::Scroll {
        dx: dx.unwrap_or_default(),
        dy: dy.unwrap_or_default(),
    })
}

fn parse_wait(object: &Map<String, Value>, errors: &mut Vec<ValidationError>) -> Result<Action, SchemaError> {
    check_unknown_fields(object, &["type", "ms"], errors);
    let ms = require_u64(object, "ms", errors);
    if let Some(ms) = ms {
        if ms > 30_000 {
            errors.push(ValidationError {
                code: ErrorCode::ConstraintViolation,
                message: "Field 'ms' must be <= 30000".to_string(),
                path: "$.ms".to_string(),
            });
        }
    }
    to_action(errors, || Action::Wait {
        ms: ms.unwrap_or_default(),
    })
}

fn parse_navigate(
    object: &Map<String, Value>,
    options: &ValidationOptions,
    errors: &mut Vec<ValidationError>,
) -> Result<Action, SchemaError> {
    check_unknown_fields(object, &["type", "url"], errors);
    let url = require_string(object, "url", errors);
    if let Some(url) = url.as_deref() {
        match Url::parse(url) {
            Ok(parsed) => {
                let scheme = parsed.scheme();
                if !options.allow_insecure && scheme != "http" && scheme != "https" {
                    errors.push(ValidationError {
                        code: ErrorCode::ConstraintViolation,
                        message: "URL must use http or https".to_string(),
                        path: "$.url".to_string(),
                    });
                }
            }
            Err(err) => errors.push(ValidationError {
                code: ErrorCode::ConstraintViolation,
                message: format!("Invalid URL: {err}"),
                path: "$.url".to_string(),
            }),
        }
    }
    to_action(errors, || Action::Navigate {
        url: url.unwrap_or_default(),
    })
}

fn parse_back(object: &Map<String, Value>, errors: &mut Vec<ValidationError>) -> Result<Action, SchemaError> {
    check_unknown_fields(object, &["type"], errors);
    to_action(errors, || Action::Back)
}

fn parse_extract(object: &Map<String, Value>, errors: &mut Vec<ValidationError>) -> Result<Action, SchemaError> {
    check_unknown_fields(object, &["type", "query"], errors);
    let query = require_string(object, "query", errors);
    enforce_non_empty("query", query.as_deref(), errors);
    to_action(errors, || Action::Extract {
        query: query.unwrap_or_default(),
    })
}

fn parse_done(object: &Map<String, Value>, errors: &mut Vec<ValidationError>) -> Result<Action, SchemaError> {
    check_unknown_fields(object, &["type", "summary"], errors);
    let summary = require_string(object, "summary", errors);
    enforce_non_empty("summary", summary.as_deref(), errors);
    to_action(errors, || Action::Done {
        summary: summary.unwrap_or_default(),
    })
}

fn require_string(
    object: &Map<String, Value>,
    key: &str,
    errors: &mut Vec<ValidationError>,
) -> Option<String> {
    match object.get(key) {
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            errors.push(ValidationError {
                code: ErrorCode::InvalidType,
                message: format!("Field '{key}' must be a string"),
                path: format!("$.{key}"),
            });
            None
        }
        None => {
            errors.push(ValidationError {
                code: ErrorCode::MissingField,
                message: format!("Field '{key}' is required"),
                path: format!("$.{key}"),
            });
            None
        }
    }
}

fn require_i32(object: &Map<String, Value>, key: &str, errors: &mut Vec<ValidationError>) -> Option<i32> {
    match object.get(key) {
        Some(value) => match value.as_i64() {
            Some(raw) if raw >= i64::from(i32::MIN) && raw <= i64::from(i32::MAX) => {
                Some(raw as i32)
            }
            Some(_) => {
                errors.push(ValidationError {
                    code: ErrorCode::ConstraintViolation,
                    message: format!("Field '{key}' is out of range for i32"),
                    path: format!("$.{key}"),
                });
                None
            }
            None => {
                errors.push(ValidationError {
                    code: ErrorCode::InvalidType,
                    message: format!("Field '{key}' must be an integer"),
                    path: format!("$.{key}"),
                });
                None
            }
        },
        None => {
            errors.push(ValidationError {
                code: ErrorCode::MissingField,
                message: format!("Field '{key}' is required"),
                path: format!("$.{key}"),
            });
            None
        }
    }
}

fn require_u64(object: &Map<String, Value>, key: &str, errors: &mut Vec<ValidationError>) -> Option<u64> {
    match object.get(key) {
        Some(value) => match value.as_u64() {
            Some(raw) => Some(raw),
            None => {
                errors.push(ValidationError {
                    code: ErrorCode::InvalidType,
                    message: format!("Field '{key}' must be an unsigned integer"),
                    path: format!("$.{key}"),
                });
                None
            }
        },
        None => {
            errors.push(ValidationError {
                code: ErrorCode::MissingField,
                message: format!("Field '{key}' is required"),
                path: format!("$.{key}"),
            });
            None
        }
    }
}

fn optional_bool(
    object: &Map<String, Value>,
    key: &str,
    errors: &mut Vec<ValidationError>,
) -> Option<bool> {
    match object.get(key) {
        Some(Value::Bool(value)) => Some(*value),
        Some(_) => {
            errors.push(ValidationError {
                code: ErrorCode::InvalidType,
                message: format!("Field '{key}' must be a boolean"),
                path: format!("$.{key}"),
            });
            None
        }
        None => None,
    }
}

fn enforce_non_empty(field: &str, value: Option<&str>, errors: &mut Vec<ValidationError>) {
    if let Some(value) = value {
        if value.trim().is_empty() {
            errors.push(ValidationError {
                code: ErrorCode::ConstraintViolation,
                message: format!("Field '{field}' must be non-empty"),
                path: format!("$.{field}"),
            });
        }
    }
}

fn check_unknown_fields(
    object: &Map<String, Value>,
    allowed: &[&str],
    errors: &mut Vec<ValidationError>,
) {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            errors.push(ValidationError {
                code: ErrorCode::UnknownField,
                message: format!("Unknown field '{key}'"),
                path: format!("$.{key}"),
            });
        }
    }
}

fn to_action<F>(errors: &[ValidationError], builder: F) -> Result<Action, SchemaError>
where
    F: FnOnce() -> Action,
{
    if errors.is_empty() {
        Ok(builder())
    } else {
        Err(SchemaError {
            errors: errors.to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(input: &str) -> Action {
        parse_action_str(input, &ValidationOptions::default()).expect("expected valid action")
    }

    fn parse_err(input: &str) -> SchemaError {
        parse_action_str(input, &ValidationOptions::default()).expect_err("expected error")
    }

    #[test]
    fn parses_click() {
        let action = parse_ok(r#"{ "type": "click", "id": "el_1" }"#);
        assert_eq!(action, Action::Click { id: "el_1".to_string() });
    }

    #[test]
    fn parses_type() {
        let action = parse_ok(r#"{ "type": "type", "id": "el_2", "text": "hello", "submit": true }"#);
        assert_eq!(
            action,
            Action::Type {
                id: "el_2".to_string(),
                text: "hello".to_string(),
                submit: true,
            }
        );
    }

    #[test]
    fn parses_select() {
        let action = parse_ok(r#"{ "type": "select", "id": "el_3", "option": "Two" }"#);
        assert_eq!(
            action,
            Action::Select {
                id: "el_3".to_string(),
                option: "Two".to_string(),
            }
        );
    }

    #[test]
    fn parses_scroll() {
        let action = parse_ok(r#"{ "type": "scroll", "dx": 0, "dy": 120 }"#);
        assert_eq!(action, Action::Scroll { dx: 0, dy: 120 });
    }

    #[test]
    fn parses_wait() {
        let action = parse_ok(r#"{ "type": "wait", "ms": 1500 }"#);
        assert_eq!(action, Action::Wait { ms: 1500 });
    }

    #[test]
    fn parses_navigate() {
        let action = parse_ok(r#"{ "type": "navigate", "url": "https://example.com" }"#);
        assert_eq!(
            action,
            Action::Navigate {
                url: "https://example.com".to_string(),
            }
        );
    }

    #[test]
    fn parses_back() {
        let action = parse_ok(r#"{ "type": "back" }"#);
        assert_eq!(action, Action::Back);
    }

    #[test]
    fn parses_extract() {
        let action = parse_ok(r#"{ "type": "extract", "query": "price" }"#);
        assert_eq!(
            action,
            Action::Extract {
                query: "price".to_string(),
            }
        );
    }

    #[test]
    fn parses_done() {
        let action = parse_ok(r#"{ "type": "done", "summary": "Finished" }"#);
        assert_eq!(
            action,
            Action::Done {
                summary: "Finished".to_string(),
            }
        );
    }

    #[test]
    fn rejects_unknown_type() {
        let err = parse_err(r#"{ "type": "explode" }"#);
        assert_eq!(err.errors[0].code, ErrorCode::UnknownAction);
    }

    #[test]
    fn rejects_missing_field() {
        let err = parse_err(r#"{ "type": "click" }"#);
        assert_eq!(err.errors[0].code, ErrorCode::MissingField);
        assert_eq!(err.errors[0].path, "$.id");
    }

    #[test]
    fn rejects_wrong_type() {
        let err = parse_err(r#"{ "type": "click", "id": 4 }"#);
        assert_eq!(err.errors[0].code, ErrorCode::InvalidType);
        assert_eq!(err.errors[0].path, "$.id");
    }

    #[test]
    fn rejects_text_too_long() {
        let long_text = "a".repeat(2001);
        let payload = format!(r#"{{
            "type": "type",
            "id": "el_9",
            "text": "{long_text}"
        }}"#);
        let err = parse_err(&payload);
        assert_eq!(err.errors[0].code, ErrorCode::ConstraintViolation);
        assert_eq!(err.errors[0].path, "$.text");
    }

    #[test]
    fn rejects_wait_too_long() {
        let err = parse_err(r#"{ "type": "wait", "ms": 40000 }"#);
        assert_eq!(err.errors[0].code, ErrorCode::ConstraintViolation);
        assert_eq!(err.errors[0].path, "$.ms");
    }

    #[test]
    fn rejects_scroll_bounds() {
        let err = parse_err(r#"{ "type": "scroll", "dx": 5000, "dy": 0 }"#);
        assert_eq!(err.errors[0].code, ErrorCode::ConstraintViolation);
        assert_eq!(err.errors[0].path, "$.dx");
    }

    #[test]
    fn rejects_non_http_url() {
        let err = parse_err(r#"{ "type": "navigate", "url": "file:///etc/passwd" }"#);
        assert_eq!(err.errors[0].code, ErrorCode::ConstraintViolation);
        assert_eq!(err.errors[0].path, "$.url");
    }

    #[test]
    fn rejects_unknown_field() {
        let err = parse_err(r#"{ "type": "back", "extra": 1 }"#);
        assert_eq!(err.errors[0].code, ErrorCode::UnknownField);
        assert_eq!(err.errors[0].path, "$.extra");
    }
}
