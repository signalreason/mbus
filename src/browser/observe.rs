use crate::browser::{BrowserError, BrowserResult};
use crate::types::{ElementRef, Observation};
use chromiumoxide::element::Element;
use chromiumoxide::page::Page;
use chromiumoxide_cdp::cdp::browser_protocol::dom::BackendNodeId;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

const ACTIONABLE_SELECTOR: &str = "a[href], button, input, select, textarea, \
[role=button], [role=link], [role=checkbox], [role=radio], [role=tab], [role=combobox]";

#[derive(Clone, Debug)]
pub struct Observer {
    config: ObserverConfig,
}

#[derive(Clone, Debug)]
pub struct ObserverConfig {
    pub max_elements: usize,
    pub max_text_len: usize,
}

impl Default for ObserverConfig {
    fn default() -> Self {
        Self {
            max_elements: 50,
            max_text_len: 4000,
        }
    }
}

impl Observer {
    pub fn new(config: ObserverConfig) -> Self {
        Self { config }
    }

    pub async fn snapshot(&self, page: &Page) -> BrowserResult<Observation> {
        let url = page
            .url()
            .await?
            .ok_or_else(|| BrowserError::new("missing_url", "page url not available"))?;
        let title = page.get_title().await?.unwrap_or_default();
        let viewport = viewport(page).await?;
        let visible_text = visible_text(page, self.config.max_text_len).await?;
        let elements = collect_actionable(page, self.config.max_elements).await?;

        Ok(Observation {
            url,
            title,
            viewport,
            focused: None,
            visible_text,
            state_hash: None,
            elements,
        })
    }
}

#[derive(Debug)]
struct ActionableElement {
    id: String,
    role: String,
    name: Option<String>,
    value: Option<String>,
    bbox: [f64; 4],
    flags: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct JsElementInfo {
    text: Option<String>,
    value: Option<String>,
}

async fn viewport(page: &Page) -> BrowserResult<[u32; 2]> {
    let metrics = page.layout_metrics().await?;
    let width = metrics.css_layout_viewport.client_width.max(0) as u32;
    let height = metrics.css_layout_viewport.client_height.max(0) as u32;
    Ok([width, height])
}

async fn visible_text(page: &Page, max_len: usize) -> BrowserResult<String> {
    let eval = r#"
        () => {
            const body = document.body;
            const text = body ? (body.innerText || body.textContent || '') : '';
            return text;
        }
    "#;
    let result = page.evaluate(eval).await?;
    let text: String = result
        .into_value()
        .map_err(|err| BrowserError::new("js_error", format!("visible text: {err}")))?;
    let trimmed = text.trim();
    Ok(truncate_text(trimmed, max_len))
}

async fn collect_actionable(page: &Page, limit: usize) -> BrowserResult<Vec<ElementRef>> {
    let candidates = page.find_elements(ACTIONABLE_SELECTOR).await?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for element in candidates {
        let backend_id = element.backend_node_id;
        if !seen.insert(*backend_id.inner()) {
            continue;
        }

        let node = element.description().await?;
        let attrs = attrs_to_map(node.attributes);
        let mut flags = Vec::new();

        let js_info = match fetch_js_info(&element).await {
            Ok(info) => info,
            Err(_err) => {
                flags.push("js_info_missing".to_string());
                JsElementInfo::default()
            }
        };

        let disabled = attrs.contains_key("disabled")
            || attrs
                .get("aria-disabled")
                .map(|value| value == "true")
                .unwrap_or(false);
        if disabled {
            flags.push("disabled".to_string());
        }

        let name = derive_name(&attrs, &js_info);
        let role = attrs
            .get("role")
            .cloned()
            .unwrap_or_else(|| node.node_name.to_lowercase());

        let value = js_info
            .value
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        let mut bbox = [0.0, 0.0, 0.0, 0.0];
        match element.bounding_box().await {
            Ok(bounds) => {
                bbox = [bounds.x, bounds.y, bounds.width, bounds.height];
            }
            Err(_) => {
                flags.push("bbox_missing".to_string());
            }
        }

        out.push(ActionableElement {
            id: element_id(backend_id),
            role,
            name,
            value,
            bbox,
            flags,
        });

        if out.len() >= limit {
            break;
        }
    }

    Ok(out
        .into_iter()
        .map(|element| ElementRef {
            id: element.id,
            role: element.role,
            name: element.name,
            value: element.value,
            bbox: element.bbox,
            flags: element.flags,
        })
        .collect())
}

fn attrs_to_map(attrs: Option<Vec<String>>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(attrs) = attrs {
        for chunk in attrs.chunks(2) {
            if let [name, value] = chunk {
                map.insert(name.to_string(), value.to_string());
            }
        }
    }
    map
}

async fn fetch_js_info(element: &Element) -> BrowserResult<JsElementInfo> {
    let result = element
        .call_js_fn(
            "function() {\n\
                const text = (this.innerText || this.textContent || '').trim();\n\
                let value = '';\n\
                if ('value' in this) {\n\
                    try { value = String(this.value); } catch (_) { value = ''; }\n\
                }\n\
                return { text, value };\n\
            }",
            false,
        )
        .await?;
    let Some(value) = result.result.value else {
        return Ok(JsElementInfo::default());
    };
    serde_json::from_value(value)
        .map_err(|err| BrowserError::new("js_error", format!("element info: {err}")))
}

fn derive_name(attrs: &HashMap<String, String>, js_info: &JsElementInfo) -> Option<String> {
    let candidate = attrs
        .get("aria-label")
        .or_else(|| attrs.get("title"))
        .or_else(|| attrs.get("alt"))
        .or_else(|| attrs.get("name"))
        .or_else(|| attrs.get("id"))
        .or_else(|| attrs.get("placeholder"))
        .cloned();

    if let Some(candidate) = candidate {
        if !candidate.trim().is_empty() {
            return Some(candidate);
        }
    }

    js_info
        .text
        .as_ref()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn element_id(backend_node_id: BackendNodeId) -> String {
    format!("el_{}", backend_node_id.inner())
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out = String::new();
    let keep = max_chars.saturating_sub(3);
    for (idx, ch) in value.chars().enumerate() {
        if idx >= keep {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attrs_to_map_pairs_values() {
        let attrs = Some(vec![
            "role".to_string(),
            "button".to_string(),
            "aria-label".to_string(),
            "Save".to_string(),
        ]);
        let map = attrs_to_map(attrs);
        assert_eq!(map.get("role").unwrap(), "button");
        assert_eq!(map.get("aria-label").unwrap(), "Save");
    }

    #[test]
    fn derive_name_prefers_aria_label() {
        let mut attrs = HashMap::new();
        attrs.insert("aria-label".to_string(), "Add".to_string());
        attrs.insert("title".to_string(), "Other".to_string());
        let info = JsElementInfo {
            text: Some("Fallback".to_string()),
            value: None,
        };
        assert_eq!(derive_name(&attrs, &info), Some("Add".to_string()));
    }

    #[test]
    fn truncate_text_limits_length() {
        let text = "abcdefghij";
        let truncated = truncate_text(text, 6);
        assert_eq!(truncated, "abc...");
    }
}
