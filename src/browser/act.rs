use crate::types::{Action, ExtractResult, StepError, StepResult};
use chromiumoxide::keys;
use chromiumoxide::layout::Point;
use chromiumoxide::page::Page;
use chromiumoxide_cdp::cdp::browser_protocol::dom::{
    BackendNodeId, FocusParams, GetBoxModelParams, ResolveNodeParams,
};
use chromiumoxide_cdp::cdp::browser_protocol::input::{
    DispatchKeyEventParams, DispatchKeyEventType,
};
use chromiumoxide_cdp::cdp::js_protocol::runtime::{CallArgument, CallFunctionOnParams, RemoteObject};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use tokio::time::{sleep, Duration};

#[derive(Clone, Debug)]
pub struct ActionApplier;

#[derive(Clone, Debug)]
pub struct ApplyOutcome {
    pub extract: Option<ExtractResult>,
}

impl ApplyOutcome {
    fn none() -> Self {
        Self { extract: None }
    }
}

#[derive(Debug, Clone)]
pub struct ActionError {
    pub code: &'static str,
    pub message: String,
}

impl ActionError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ActionError {}

impl From<chromiumoxide::error::CdpError> for ActionError {
    fn from(err: chromiumoxide::error::CdpError) -> Self {
        ActionError::new("cdp_error", err.to_string())
    }
}

fn is_detached_error(err: &chromiumoxide::error::CdpError) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("node")
        && (msg.contains("not found")
            || msg.contains("no node")
            || msg.contains("could not find node")
            || msg.contains("backend node")
            || msg.contains("detached"))
}

fn map_cdp_error(err: chromiumoxide::error::CdpError, context: &'static str) -> ActionError {
    if is_detached_error(&err) {
        ActionError::new(
            "stale_element",
            format!("element detached or stale during {context}"),
        )
    } else {
        ActionError::new("cdp_error", err.to_string())
    }
}

impl ActionApplier {
    pub fn new() -> Self {
        Self
    }

    pub async fn apply(
        &self,
        page: &Page,
        action: &Action,
        element_map: Option<&HashMap<String, BackendNodeId>>,
    ) -> Result<ApplyOutcome, ActionError> {
        let outcome = match action {
            Action::Click { id } => {
                click_by_id(page, id, element_map).await?;
                ApplyOutcome::none()
            }
            Action::Type { id, text, submit } => {
                type_by_id(page, id, text, submit.unwrap_or(false), element_map).await?;
                ApplyOutcome::none()
            }
            Action::Select { id, value } => {
                select_by_id(page, id, value, element_map).await?;
                ApplyOutcome::none()
            }
            Action::Scroll { dx, dy } => {
                let script = format!("() => window.scrollBy({dx}, {dy})");
                page.evaluate(script).await?;
                ApplyOutcome::none()
            }
            Action::Wait { ms } => {
                sleep(Duration::from_millis(*ms)).await;
                ApplyOutcome::none()
            }
            Action::Navigate { url } => {
                page.goto(url.as_str()).await?;
                ApplyOutcome::none()
            }
            Action::Back => {
                page.evaluate("() => history.back()").await?;
                ApplyOutcome::none()
            }
            Action::Extract { query, id } => {
                let value = if let Some(target) = id.as_deref() {
                    extract_by_id(page, target, query, element_map).await?
                } else {
                    extract_from_page(page, query).await?
                };
                ApplyOutcome {
                    extract: Some(ExtractResult {
                        query: query.to_string(),
                        id: id.clone(),
                        value,
                    }),
                }
            }
            Action::Done { .. } => ApplyOutcome::none(),
        };
        Ok(outcome)
    }
}

pub fn action_result_ok(extract: Option<ExtractResult>) -> StepResult {
    StepResult {
        ok: true,
        error: None,
        new_state_hash: None,
        extract,
    }
}

pub fn action_result_err(error: ActionError) -> StepResult {
    StepResult {
        ok: false,
        error: Some(StepError {
            code: error.code.to_string(),
            message: error.message,
        }),
        new_state_hash: None,
        extract: None,
    }
}

async fn click_by_id(
    page: &Page,
    id: &str,
    element_map: Option<&HashMap<String, BackendNodeId>>,
) -> Result<(), ActionError> {
    let backend_id = resolve_backend_node_id(id, element_map)?;
    let (x, y) = backend_node_click_point(page, backend_id).await?;
    page.click(Point::new(x, y)).await?;
    Ok(())
}

async fn type_by_id(
    page: &Page,
    id: &str,
    text: &str,
    submit: bool,
    element_map: Option<&HashMap<String, BackendNodeId>>,
) -> Result<(), ActionError> {
    const TYPE_FN: &str = r#"
        function(text) {
            if (!this) {
                return { ok: false, error: "missing_element" };
            }
            try {
                if ("value" in this) {
                    this.value = text;
                } else if (this.isContentEditable) {
                    this.textContent = text;
                } else {
                    return { ok: false, error: "not_editable" };
                }
            } catch (err) {
                return { ok: false, error: "set_value_failed" };
            }
            this.dispatchEvent(new Event("input", { bubbles: true }));
            this.dispatchEvent(new Event("change", { bubbles: true }));
            return { ok: true, value: String(this.value ?? this.textContent ?? "") };
        }
    "#;

    let backend_id = resolve_backend_node_id(id, element_map)?;
    focus_backend_node(page, backend_id).await?;
    let value = call_function_on_node(
        page,
        backend_id,
        TYPE_FN,
        vec![Value::String(text.to_string())],
    )
    .await?;
    parse_js_action_result(value, "type_failed")?;
    if submit {
        press_key(page, "Enter").await?;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct JsActionResult {
    ok: bool,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

fn parse_js_action_result(
    value: Value,
    failure_code: &'static str,
) -> Result<Option<String>, ActionError> {
    let parsed: JsActionResult = serde_json::from_value(value).map_err(|err| {
        ActionError::new("js_error", format!("invalid js result: {err}"))
    })?;
    if parsed.ok {
        Ok(parsed.value)
    } else {
        Err(ActionError::new(
            failure_code,
            parsed
                .error
                .unwrap_or_else(|| "js action failed".to_string()),
        ))
    }
}

async fn call_function_on_node(
    page: &Page,
    backend_node_id: BackendNodeId,
    function_declaration: &str,
    arguments: Vec<Value>,
) -> Result<Value, ActionError> {
    let resolved = page
        .execute(
            ResolveNodeParams::builder()
                .backend_node_id(backend_node_id)
                .build(),
        )
        .await
        .map_err(|err| map_cdp_error(err, "resolve_node"))?;
    let object_id = resolved
        .result
        .object
        .object_id
        .ok_or_else(|| ActionError::new("missing_object_id", "resolve node returned no object"))?;

    let mut builder = CallFunctionOnParams::builder()
        .function_declaration(function_declaration)
        .object_id(object_id)
        .return_by_value(true);
    for argument in arguments {
        builder = builder.argument(CallArgument::builder().value(argument).build());
    }
    let params = builder
        .build()
        .map_err(|err| ActionError::new("js_error", err))?;
    let response = page
        .execute(params)
        .await
        .map_err(|err| map_cdp_error(err, "call_function"))?;
    let call_result = response.result;
    if let Some(details) = call_result.exception_details {
        return Err(ActionError::new(
            "js_error",
            format!("js exception: {details:?}"),
        ));
    }
    let remote: RemoteObject = call_result.result;
    remote
        .value
        .ok_or_else(|| ActionError::new("js_error", "missing js return value"))
}

async fn select_by_id(
    page: &Page,
    id: &str,
    value: &str,
    element_map: Option<&HashMap<String, BackendNodeId>>,
) -> Result<(), ActionError> {
    const SELECT_FN: &str = r#"
        function(value) {
            if (!this) {
                return { ok: false, error: "missing_element" };
            }
            const tag = this.tagName ? this.tagName.toLowerCase() : "";
            if (tag !== "select" && !("value" in this)) {
                return { ok: false, error: "not_select" };
            }
            let nextValue = value;
            if (tag === "select" && this.options) {
                const options = Array.from(this.options);
                const match = options.find(
                    (opt) => opt.value === value || opt.text === value
                );
                if (match) {
                    nextValue = match.value;
                }
            }
            try {
                this.value = nextValue;
            } catch (err) {
                return { ok: false, error: "set_value_failed" };
            }
            this.dispatchEvent(new Event("input", { bubbles: true }));
            this.dispatchEvent(new Event("change", { bubbles: true }));
            return { ok: true, value: String(this.value ?? "") };
        }
    "#;
    let backend_id = resolve_backend_node_id(id, element_map)?;
    let value = call_function_on_node(
        page,
        backend_id,
        SELECT_FN,
        vec![Value::String(value.to_string())],
    )
    .await?;
    parse_js_action_result(value, "select_failed")?;
    Ok(())
}

async fn extract_by_id(
    page: &Page,
    id: &str,
    query: &str,
    element_map: Option<&HashMap<String, BackendNodeId>>,
) -> Result<String, ActionError> {
    let backend_id = resolve_backend_node_id(id, element_map)?;
    let value = call_function_on_node(
        page,
        backend_id,
        extract_function(),
        vec![Value::String(query.to_string())],
    )
    .await?;
    Ok(parse_js_action_result(value, "extract_failed")?.unwrap_or_default())
}

async fn extract_from_page(page: &Page, query: &str) -> Result<String, ActionError> {
    let query_literal =
        serde_json::to_string(query).map_err(|err| ActionError::new("js_error", err.to_string()))?;
    let script = format!("({})({})", extract_function(), query_literal);
    let result = page.evaluate(script).await?;
    let value = result
        .into_value()
        .map_err(|err| ActionError::new("js_error", format!("extract: {err}")))?;
    Ok(parse_js_action_result(value, "extract_failed")?.unwrap_or_default())
}

fn extract_function() -> &'static str {
    r#"
        function(query) {
            const root = (this && this.querySelectorAll) ? this : document;
            if (!query) {
                return { ok: false, error: "missing_query" };
            }
            let target = null;
            if (query === "self" && root !== document) {
                target = root;
            }
            if (!target) {
                try {
                    if (root.querySelector) {
                        target = root.querySelector(query);
                    }
                } catch (err) {
                    target = null;
                }
            }
            if (!target) {
                try {
                    const needle = String(query).toLowerCase();
                    const nodes = root.querySelectorAll ? root.querySelectorAll("*") : [];
                    for (let i = 0; i < nodes.length && i < 2000; i += 1) {
                        const text = (nodes[i].innerText || nodes[i].textContent || "").trim();
                        if (text && text.toLowerCase().includes(needle)) {
                            target = nodes[i];
                            break;
                        }
                    }
                } catch (err) {
                    target = null;
                }
            }
            if (!target) {
                return { ok: false, error: "not_found" };
            }
            let value = "";
            if ("value" in target) {
                try {
                    value = String(target.value ?? "");
                } catch (err) {
                    value = "";
                }
            }
            if (!value) {
                value = (target.innerText || target.textContent || "").trim();
            }
            return { ok: true, value };
        }
    "#
}

async fn focus_backend_node(page: &Page, backend_node_id: BackendNodeId) -> Result<(), ActionError> {
    let params = FocusParams::builder()
        .backend_node_id(backend_node_id)
        .build();
    page.execute(params)
        .await
        .map_err(|err| map_cdp_error(err, "focus"))?;
    Ok(())
}

async fn backend_node_click_point(
    page: &Page,
    backend_node_id: BackendNodeId,
) -> Result<(f64, f64), ActionError> {
    let model = page
        .execute(
            GetBoxModelParams::builder()
                .backend_node_id(backend_node_id)
                .build(),
        )
        .await
        .map_err(|err| map_cdp_error(err, "get_box_model"))?
        .result
        .model;
    quad_center(model.border.inner())
        .ok_or_else(|| ActionError::new("invalid_box_model", "invalid element box model"))
}

fn quad_center(quad: &[f64]) -> Option<(f64, f64)> {
    if quad.len() != 8 {
        return None;
    }
    let xs = [quad[0], quad[2], quad[4], quad[6]];
    let ys = [quad[1], quad[3], quad[5], quad[7]];
    let (min_x, max_x) = xs.iter().fold((xs[0], xs[0]), |acc, x| {
        (acc.0.min(*x), acc.1.max(*x))
    });
    let (min_y, max_y) = ys.iter().fold((ys[0], ys[0]), |acc, y| {
        (acc.0.min(*y), acc.1.max(*y))
    });
    Some(((min_x + max_x) / 2.0, (min_y + max_y) / 2.0))
}

fn parse_backend_node_id(id: &str) -> Result<BackendNodeId, ActionError> {
    let raw = id
        .strip_prefix("el_")
        .ok_or_else(|| ActionError::new("invalid_element_id", "missing el_ prefix"))?;
    let parsed = raw
        .parse::<i64>()
        .map_err(|_| ActionError::new("invalid_element_id", "invalid backend node id"))?;
    Ok(BackendNodeId::new(parsed))
}

fn resolve_backend_node_id(
    id: &str,
    element_map: Option<&HashMap<String, BackendNodeId>>,
) -> Result<BackendNodeId, ActionError> {
    if let Some(map) = element_map {
        return map.get(id).cloned().ok_or_else(|| {
            ActionError::new(
                "stale_element",
                format!("id {id} not found in latest observation"),
            )
        });
    }
    parse_backend_node_id(id)
}

async fn press_key(page: &Page, key: &str) -> Result<(), ActionError> {
    let key_definition = keys::get_key_definition(key)
        .ok_or_else(|| ActionError::new("invalid_key", format!("key not found: {key}")))?;
    let mut cmd = DispatchKeyEventParams::builder();
    let key_down_event_type = if let Some(txt) = key_definition.text {
        cmd = cmd.text(txt);
        DispatchKeyEventType::KeyDown
    } else if key_definition.key.len() == 1 {
        cmd = cmd.text(key_definition.key);
        DispatchKeyEventType::KeyDown
    } else {
        DispatchKeyEventType::RawKeyDown
    };

    cmd = cmd
        .r#type(DispatchKeyEventType::KeyDown)
        .key(key_definition.key)
        .code(key_definition.code)
        .windows_virtual_key_code(key_definition.key_code)
        .native_virtual_key_code(key_definition.key_code);

    page.execute(cmd.clone().r#type(key_down_event_type).build().unwrap())
        .await?;
    page.execute(cmd.r#type(DispatchKeyEventType::KeyUp).build().unwrap())
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use serde_json::json;

    #[test]
    fn parse_backend_node_id_accepts_el_prefix() {
        let id = parse_backend_node_id("el_42").expect("parse id");
        assert_eq!(*id.inner(), 42);
    }

    #[test]
    fn parse_backend_node_id_rejects_invalid() {
        assert!(parse_backend_node_id("42").is_err());
        assert!(parse_backend_node_id("el_nope").is_err());
    }

    #[test]
    fn quad_center_returns_midpoint() {
        let quad = vec![0.0, 0.0, 10.0, 0.0, 10.0, 20.0, 0.0, 20.0];
        let center = quad_center(&quad).unwrap();
        assert_eq!(center.0, 5.0);
        assert_eq!(center.1, 10.0);
    }

    #[test]
    fn resolve_backend_node_id_prefers_map() {
        let mut map = HashMap::new();
        map.insert("el_deadbeef_1".to_string(), BackendNodeId::new(7));
        let resolved =
            resolve_backend_node_id("el_deadbeef_1", Some(&map)).expect("resolve id");
        assert_eq!(*resolved.inner(), 7);
    }

    #[test]
    fn resolve_backend_node_id_requires_latest_map_entry() {
        let map: HashMap<String, BackendNodeId> = HashMap::new();
        let err = resolve_backend_node_id("el_missing_1", Some(&map)).unwrap_err();
        assert_eq!(err.code, "stale_element");
    }

    #[test]
    fn resolve_backend_node_id_falls_back_to_parse() {
        let map = HashMap::new();
        let resolved = resolve_backend_node_id("el_42", Some(&map)).expect("resolve id");
        assert_eq!(*resolved.inner(), 42);
    }

    #[test]
    fn parse_js_action_result_accepts_ok() {
        let value = json!({"ok": true, "value": "done"});
        let parsed = parse_js_action_result(value, "select_failed").expect("parse result");
        assert_eq!(parsed, Some("done".to_string()));
    }

    #[test]
    fn parse_js_action_result_reports_failure() {
        let value = json!({"ok": false, "error": "not_found"});
        let err = parse_js_action_result(value, "extract_failed").expect_err("expect error");
        assert_eq!(err.code, "extract_failed");
        assert_eq!(err.message, "not_found");
    }

    #[test]
    fn parse_js_action_result_rejects_invalid() {
        let value = json!(true);
        let err = parse_js_action_result(value, "extract_failed").expect_err("expect error");
        assert_eq!(err.code, "js_error");
    }
}
