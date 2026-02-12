use mbus::types::Action;
use serde_json::Value;

fn assert_golden(payload: &str, path: &str) {
    let value: Value = serde_json::from_str(payload).expect("parse golden json");
    let action: Action = serde_json::from_value(value.clone()).expect("deserialize action");
    let serialized = serde_json::to_value(&action).expect("serialize action");
    assert_eq!(serialized, value, "round-trip mismatch for {path}");
}

#[test]
fn golden_click_round_trip() {
    assert_golden(
        include_str!("fixtures/actions/click.json"),
        "fixtures/actions/click.json",
    );
}

#[test]
fn golden_type_round_trip() {
    assert_golden(
        include_str!("fixtures/actions/type.json"),
        "fixtures/actions/type.json",
    );
}

#[test]
fn golden_select_round_trip() {
    assert_golden(
        include_str!("fixtures/actions/select.json"),
        "fixtures/actions/select.json",
    );
}

#[test]
fn golden_scroll_round_trip() {
    assert_golden(
        include_str!("fixtures/actions/scroll.json"),
        "fixtures/actions/scroll.json",
    );
}

#[test]
fn golden_wait_round_trip() {
    assert_golden(
        include_str!("fixtures/actions/wait.json"),
        "fixtures/actions/wait.json",
    );
}

#[test]
fn golden_navigate_round_trip() {
    assert_golden(
        include_str!("fixtures/actions/navigate.json"),
        "fixtures/actions/navigate.json",
    );
}

#[test]
fn golden_back_round_trip() {
    assert_golden(
        include_str!("fixtures/actions/back.json"),
        "fixtures/actions/back.json",
    );
}

#[test]
fn golden_extract_round_trip() {
    assert_golden(
        include_str!("fixtures/actions/extract.json"),
        "fixtures/actions/extract.json",
    );
}

#[test]
fn golden_done_round_trip() {
    assert_golden(
        include_str!("fixtures/actions/done.json"),
        "fixtures/actions/done.json",
    );
}
