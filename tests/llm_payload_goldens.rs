use mbus::llm::client::LlmContext;
use mbus::llm::request::build_request;
use mbus::types::{ElementFlags, ElementRef, Observation, ScreenshotMetadata};
use serde_json::{Value, json};
use std::collections::VecDeque;

fn assert_golden(payload: &Value, path: &str) {
    let expected: Value = serde_json::from_str(include_str!("fixtures/llm/multimodal.json"))
        .expect("parse golden json");
    assert_eq!(payload, &expected, "round-trip mismatch for {path}");
}

#[test]
fn golden_multimodal_payload_shape() {
    let observation = Observation {
        url: "https://example.com".to_string(),
        title: "Example".to_string(),
        viewport: [1280, 800],
        focused: Some("el_1".to_string()),
        visible_text: "Search results".to_string(),
        screenshot: Some(ScreenshotMetadata {
            mime_type: "image/png".to_string(),
            artifact_ref: Some("step://run/step-1/screenshot.png".to_string()),
            sha256: "deadbeef".to_string(),
            bytes: 4,
        }),
        state_hash: "hash-1".to_string(),
        elements: vec![ElementRef {
            id: "el_1".to_string(),
            role: "textbox".to_string(),
            name: Some("From".to_string()),
            value: None,
            bbox: [10.5, 20.5, 300.5, 40.5],
            flags: ElementFlags {
                focused: Some(true),
                ..ElementFlags::default()
            },
        }],
    };
    let mut observations = VecDeque::new();
    observations.push_back(observation.clone());

    let schema_json = json!({
        "type": "object",
        "properties": {"type": {"const": "click"}},
        "required": ["type"]
    });

    let context = LlmContext {
        task: "Book a flight",
        plan: Some("Find the cheapest direct flight"),
        observation: &observation,
        observations: &observations,
        observation_screenshot: Some(&[0, 1, 2, 3]),
        history: &[],
        steps: &[],
    };

    let request = build_request(&context, &schema_json).expect("build request");

    let payload = serde_json::to_value(&request).expect("serialize request");
    assert_golden(&payload, "fixtures/llm/multimodal.json");
}
