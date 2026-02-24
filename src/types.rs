use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const SCREENSHOT_MIME_TYPE: &str = "image/png";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScreenshotMetadata {
    /// MIME type for screenshot bytes (e.g. image/png).
    pub mime_type: String,
    /// Optional artifact reference when persisted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_ref: Option<String>,
    /// SHA-256 hex digest of screenshot bytes.
    pub sha256: String,
    /// Size of screenshot bytes.
    pub bytes: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Observation {
    /// Current page URL after any redirects.
    pub url: String,
    /// Document title text (may be empty if unavailable).
    pub title: String,
    /// Viewport size in CSS pixels: `[width, height]`.
    pub viewport: [u32; 2],
    /// Focused element id when known; null when no focused element is tracked.
    pub focused: Option<String>,
    /// Compact, trimmed visible text for context (length capped by observer).
    pub visible_text: String,
    /// Optional screenshot metadata for this observation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<ScreenshotMetadata>,
    /// Deterministic hash of the compact snapshot for progress detection.
    pub state_hash: String,
    /// Actionable elements with stable ids in observation order.
    #[serde(default)]
    pub elements: Vec<ElementRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ElementRef {
    pub id: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub bbox: [f64; 4],
    #[serde(default, skip_serializing_if = "ElementFlags::is_empty")]
    pub flags: ElementFlags,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ElementFlags {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readonly: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pressed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox_missing: Option<bool>,
}

impl ElementFlags {
    fn is_empty(&self) -> bool {
        self.disabled.is_none()
            && self.readonly.is_none()
            && self.required.is_none()
            && self.focused.is_none()
            && self.editable.is_none()
            && self.checked.is_none()
            && self.selected.is_none()
            && self.expanded.is_none()
            && self.pressed.is_none()
            && self.bbox_missing.is_none()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Click {
        id: String,
    },
    Type {
        id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        submit: Option<bool>,
    },
    Select {
        id: String,
        value: String,
    },
    Scroll {
        dx: i64,
        dy: i64,
    },
    Wait {
        ms: u64,
    },
    Navigate {
        url: String,
    },
    Back,
    Extract {
        query: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    Done {
        summary: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StepError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_code: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StepDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExtractResult {
    pub query: String,
    pub id: Option<String>,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmPayloadMode {
    TextOnly,
    Multimodal,
}

impl LlmPayloadMode {
    pub fn image_context_sent(self) -> bool {
        matches!(self, Self::Multimodal)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StepResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<StepError>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<StepDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_state_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extract: Option<ExtractResult>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn action_round_trip_click() {
        let action = Action::Click {
            id: "el_42".to_string(),
        };
        let value = serde_json::to_value(&action).expect("serialize action");
        assert_eq!(value, json!({"type": "click", "id": "el_42"}));
        let parsed: Action = serde_json::from_value(value).expect("deserialize action");
        assert_eq!(parsed, action);
    }

    #[test]
    fn action_rejects_unknown_type() {
        let value = json!({"type": "teleport", "id": "el_1"});
        let parsed: Result<Action, _> = serde_json::from_value(value);
        assert!(parsed.is_err(), "expected unknown action type to fail");
    }

    #[test]
    fn action_requires_type_tag() {
        let value = json!({"id": "el_1"});
        let parsed: Result<Action, _> = serde_json::from_value(value);
        assert!(parsed.is_err(), "expected missing type tag to fail");
    }

    #[test]
    fn observation_round_trip() {
        let observation = Observation {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            viewport: [1280, 800],
            focused: Some("el_7".to_string()),
            visible_text: "Hello".to_string(),
            screenshot: Some(ScreenshotMetadata {
                mime_type: SCREENSHOT_MIME_TYPE.to_string(),
                artifact_ref: Some("step://task_1_now/step-1/screenshot.png".to_string()),
                sha256: "deadbeef".to_string(),
                bytes: 42,
            }),
            state_hash: "ab12cd".to_string(),
            elements: vec![ElementRef {
                id: "el_7".to_string(),
                role: "textbox".to_string(),
                name: Some("Email".to_string()),
                value: None,
                bbox: [10.0, 120.0, 400.0, 36.0],
                flags: ElementFlags {
                    focused: Some(true),
                    ..ElementFlags::default()
                },
            }],
        };
        let value = serde_json::to_value(&observation).expect("serialize observation");
        assert_eq!(value.get("state_hash"), Some(&json!("ab12cd")));
        assert_eq!(value.get("focused"), Some(&json!("el_7")));
        let parsed: Observation = serde_json::from_value(value).expect("deserialize observation");
        assert_eq!(parsed, observation);
    }

    #[test]
    fn observation_deserializes_without_screenshot() {
        let value = json!({
            "url": "https://example.com",
            "title": "Example",
            "viewport": [1280, 800],
            "focused": null,
            "visible_text": "Hello",
            "state_hash": "ab12cd",
            "elements": []
        });
        let parsed: Observation = serde_json::from_value(value).expect("deserialize observation");
        assert!(parsed.screenshot.is_none());
    }

    #[test]
    fn step_result_round_trip() {
        let result = StepResult {
            ok: false,
            error: Some(StepError {
                code: "invalid_action".to_string(),
                message: "missing id".to_string(),
                validation_code: Some("missing_id".to_string()),
            }),
            diagnostics: Vec::new(),
            new_state_hash: None,
            scroll: None,
            extract: None,
        };
        let value = serde_json::to_value(&result).expect("serialize step result");
        let parsed: StepResult = serde_json::from_value(value).expect("deserialize step result");
        assert_eq!(parsed, result);
    }

    #[test]
    fn step_result_with_extract_round_trip() {
        let result = StepResult {
            ok: true,
            error: None,
            diagnostics: Vec::new(),
            new_state_hash: None,
            scroll: None,
            extract: Some(ExtractResult {
                query: "price".to_string(),
                id: Some("el_4".to_string()),
                value: "$10".to_string(),
            }),
        };
        let value = serde_json::to_value(&result).expect("serialize step result");
        let parsed: StepResult = serde_json::from_value(value).expect("deserialize step result");
        assert_eq!(parsed, result);
    }
}
