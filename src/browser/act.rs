use crate::types::{Action, StepError, StepResult};
use chromiumoxide::keys;
use chromiumoxide::layout::Point;
use chromiumoxide::page::Page;
use chromiumoxide_cdp::cdp::browser_protocol::dom::{BackendNodeId, FocusParams, GetBoxModelParams};
use chromiumoxide_cdp::cdp::browser_protocol::input::{
    DispatchKeyEventParams, DispatchKeyEventType, InsertTextParams,
};
use tokio::time::{sleep, Duration};

#[derive(Clone, Debug)]
pub struct ActionApplier;

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

    fn unsupported(action: &Action) -> Self {
        Self::new("unsupported_action", format!("action not supported: {action:?}"))
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

impl ActionApplier {
    pub fn new() -> Self {
        Self
    }

    pub async fn apply(&self, page: &Page, action: &Action) -> Result<(), ActionError> {
        match action {
            Action::Click { id } => {
                click_by_id(page, id).await?;
            }
            Action::Type { id, text, submit } => {
                type_by_id(page, id, text, submit.unwrap_or(false)).await?;
            }
            Action::Scroll { dx, dy } => {
                let script = format!("() => window.scrollBy({dx}, {dy})");
                page.evaluate(script).await?;
            }
            Action::Wait { ms } => {
                sleep(Duration::from_millis(*ms)).await;
            }
            Action::Navigate { url } => {
                page.goto(url.as_str()).await?;
            }
            Action::Back => {
                page.evaluate("() => history.back()").await?;
            }
            Action::Select { .. } | Action::Extract { .. } | Action::Done { .. } => {
                return Err(ActionError::unsupported(action));
            }
        }
        Ok(())
    }
}

pub fn action_result_ok() -> StepResult {
    StepResult {
        ok: true,
        error: None,
        new_state_hash: None,
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
    }
}

async fn click_by_id(page: &Page, id: &str) -> Result<(), ActionError> {
    let backend_id = parse_backend_node_id(id)?;
    let (x, y) = backend_node_click_point(page, backend_id).await?;
    page.click(Point::new(x, y)).await?;
    Ok(())
}

async fn type_by_id(
    page: &Page,
    id: &str,
    text: &str,
    submit: bool,
) -> Result<(), ActionError> {
    let backend_id = parse_backend_node_id(id)?;
    focus_backend_node(page, backend_id).await?;
    page.execute(InsertTextParams::new(text)).await?;
    if submit {
        press_key(page, "Enter").await?;
    }
    Ok(())
}

async fn focus_backend_node(page: &Page, backend_node_id: BackendNodeId) -> Result<(), ActionError> {
    let params = FocusParams::builder()
        .backend_node_id(backend_node_id)
        .build();
    page.execute(params).await?;
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
        .await?
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
}
