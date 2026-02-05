use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Observation {
    pub url: String,
    pub title: String,
    pub viewport: [u32; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused: Option<String>,
    pub visible_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_hash: Option<String>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Click { id: String },
    Type {
        id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        submit: Option<bool>,
    },
    Select { id: String, value: String },
    Scroll { dx: i64, dy: i64 },
    Wait { ms: u64 },
    Navigate { url: String },
    Back,
    Extract {
        query: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    Done { summary: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StepError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StepResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<StepError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_state_hash: Option<String>,
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
    fn observation_round_trip() {
        let observation = Observation {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            viewport: [1280, 800],
            focused: Some("el_7".to_string()),
            visible_text: "Hello".to_string(),
            state_hash: Some("ab12cd".to_string()),
            elements: vec![ElementRef {
                id: "el_7".to_string(),
                role: "textbox".to_string(),
                name: Some("Email".to_string()),
                value: None,
                bbox: [10.0, 120.0, 400.0, 36.0],
                flags: vec!["focusable".to_string()],
            }],
        };
        let value = serde_json::to_value(&observation).expect("serialize observation");
        let parsed: Observation =
            serde_json::from_value(value).expect("deserialize observation");
        assert_eq!(parsed, observation);
    }

    #[test]
    fn step_result_round_trip() {
        let result = StepResult {
            ok: false,
            error: Some(StepError {
                code: "invalid_action".to_string(),
                message: "missing id".to_string(),
            }),
            new_state_hash: None,
        };
        let value = serde_json::to_value(&result).expect("serialize step result");
        let parsed: StepResult = serde_json::from_value(value).expect("deserialize step result");
        assert_eq!(parsed, result);
    }
}
