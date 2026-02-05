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

#[derive(Clone, Debug)]
pub struct ObservedSnapshot {
    pub observation: Observation,
    pub element_map: HashMap<String, BackendNodeId>,
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

    pub async fn snapshot(&self, page: &Page) -> BrowserResult<ObservedSnapshot> {
        let url = page
            .url()
            .await?
            .ok_or_else(|| BrowserError::new("missing_url", "page url not available"))?;
        let title = page.get_title().await?.unwrap_or_default();
        let viewport = viewport(page).await?;
        let visible_text = visible_text(page, self.config.max_text_len).await?;
        let collected = collect_actionable(page, self.config.max_elements).await?;
        let state_hash = Some(compute_state_hash(&url, &title, &collected.elements));

        Ok(ObservedSnapshot {
            observation: Observation {
                url,
                title,
                viewport,
                focused: None,
                visible_text,
                state_hash,
                elements: collected.elements,
            },
            element_map: collected.element_map,
        })
    }
}

#[derive(Debug)]
struct ActionableElement {
    backend_id: BackendNodeId,
    role: String,
    name: Option<String>,
    value: Option<String>,
    bbox: [f64; 4],
    flags: Vec<String>,
    signature: String,
}

#[derive(Debug)]
struct CollectedElements {
    elements: Vec<ElementRef>,
    element_map: HashMap<String, BackendNodeId>,
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

async fn collect_actionable(page: &Page, limit: usize) -> BrowserResult<CollectedElements> {
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

        let signature = stable_signature(&role, &name, &node.node_name, &attrs);

        out.push(ActionableElement {
            backend_id,
            role,
            name,
            value,
            bbox,
            flags,
            signature,
        });

        if out.len() >= limit {
            break;
        }
    }

    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut elements = Vec::new();
    let mut element_map = HashMap::new();

    for element in out {
        let count = counts.entry(element.signature.clone()).or_insert(0);
        *count += 1;
        let id = stable_element_id(&element.signature, *count);
        element_map.insert(id.clone(), element.backend_id);
        elements.push(ElementRef {
            id,
            role: element.role,
            name: element.name,
            value: element.value,
            bbox: element.bbox,
            flags: element.flags,
        });
    }

    Ok(CollectedElements { elements, element_map })
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
        let trimmed = candidate.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    js_info
        .text
        .as_ref()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn stable_signature(
    role: &str,
    name: &Option<String>,
    node_name: &str,
    attrs: &HashMap<String, String>,
) -> String {
    let mut parts = Vec::new();
    parts.push(role.trim().to_lowercase());
    parts.push(node_name.trim().to_lowercase());

    if let Some(name) = name.as_ref().map(|value| value.trim()).filter(|v| !v.is_empty()) {
        parts.push(name.to_string());
    }

    for key in [
        "id",
        "name",
        "type",
        "href",
        "aria-label",
        "title",
        "alt",
        "placeholder",
    ] {
        if let Some(value) = attrs.get(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                parts.push(format!("{key}={trimmed}"));
            }
        }
    }

    parts.join("|")
}

fn stable_element_id(signature: &str, occurrence: usize) -> String {
    format!("el_{}_{}", hash_hex(signature), occurrence)
}

fn compute_state_hash(url: &str, title: &str, elements: &[ElementRef]) -> String {
    const TOP_ELEMENTS: usize = 10;
    let mut signature = String::new();
    signature.push_str(url.trim());
    signature.push('\n');
    signature.push_str(title.trim());

    for element in elements.iter().take(TOP_ELEMENTS) {
        signature.push('\n');
        signature.push_str(&element.id);
        signature.push('|');
        signature.push_str(&element.role);
        if let Some(name) = element.name.as_ref() {
            signature.push('|');
            signature.push_str(name);
        }
    }

    hash_hex(&signature)
}

fn hash_hex(input: &str) -> String {
    format!("{:016x}", fnv1a_64(input.as_bytes()))
}

fn fnv1a_64(input: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001b3;
    let mut hash = FNV_OFFSET;
    for byte in input {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
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

    #[test]
    fn stable_element_id_is_deterministic() {
        let signature = "button|button|Save|id=save-btn";
        let id_a = stable_element_id(signature, 1);
        let id_b = stable_element_id(signature, 1);
        assert_eq!(id_a, id_b);
    }

    #[test]
    fn stable_element_id_distinguishes_occurrence() {
        let signature = "button|button|Save|id=save-btn";
        let id_a = stable_element_id(signature, 1);
        let id_b = stable_element_id(signature, 2);
        assert_ne!(id_a, id_b);
    }

    #[test]
    fn state_hash_changes_with_url() {
        let elements = vec![ElementRef {
            id: "el_deadbeef_1".to_string(),
            role: "button".to_string(),
            name: Some("Save".to_string()),
            value: None,
            bbox: [0.0, 0.0, 10.0, 10.0],
            flags: Vec::new(),
        }];
        let hash_a = compute_state_hash("https://a.example", "Title", &elements);
        let hash_b = compute_state_hash("https://b.example", "Title", &elements);
        assert_ne!(hash_a, hash_b);
    }
}
