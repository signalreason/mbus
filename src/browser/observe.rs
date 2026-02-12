use crate::browser::{BrowserError, BrowserResult};
use crate::types::{ElementFlags, ElementRef, Observation};
use chromiumoxide::page::Page;
use chromiumoxide_cdp::cdp::browser_protocol::accessibility::{
    AxNode, AxProperty, AxPropertyName, AxValue, EnableParams, GetFullAxTreeParams,
};
use chromiumoxide_cdp::cdp::browser_protocol::dom::{BackendNodeId, GetBoxModelParams};
use std::collections::{HashMap, HashSet};

const ACTIONABLE_ROLES: &[&str] = &[
    "button",
    "link",
    "checkbox",
    "radio",
    "textbox",
    "searchbox",
    "combobox",
    "listbox",
    "option",
    "tab",
    "switch",
    "slider",
    "spinbutton",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
];

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
        let state_hash = compute_state_hash(&url, &title, &collected.elements);

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
    flags: ElementFlags,
    signature: String,
}

#[derive(Debug)]
struct CollectedElements {
    elements: Vec<ElementRef>,
    element_map: HashMap<String, BackendNodeId>,
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
            const MAX_INTERACTIVE = 40;
            const MAX_NEARBY = 20;
            const MAX_HEADINGS = 12;
            const MAX_STATUS = 8;
            const MAX_CHUNK = 160;
            const INTERACTIVE_SELECTOR = [
                "button",
                "a[href]",
                "input",
                "select",
                "textarea",
                "[role='button']",
                "[role='link']",
                "[role='checkbox']",
                "[role='radio']",
                "[role='tab']",
                "[role='switch']",
                "[role='menuitem']",
                "[role='menuitemcheckbox']",
                "[role='menuitemradio']",
                "[role='option']",
                "[role='combobox']",
                "[role='listbox']",
                "[role='slider']",
                "[role='spinbutton']",
                "[contenteditable='true']",
                "[tabindex]"
            ].join(",");
            const STATUS_SELECTOR = [
                "[role='status']",
                "[role='alert']",
                "[role='log']",
                "[aria-live]"
            ].join(",");

            const disallowedTags = new Set([
                "INPUT",
                "TEXTAREA",
                "SELECT",
                "OPTION",
                "SCRIPT",
                "STYLE",
                "NOSCRIPT"
            ]);

            const normalize = (text) => text.replace(/\s+/g, " ").trim();

            const isVisible = (el) => {
                if (!el) return false;
                const style = window.getComputedStyle(el);
                if (!style || style.display === "none" || style.visibility === "hidden") return false;
                if (parseFloat(style.opacity || "1") === 0) return false;
                const rect = el.getBoundingClientRect();
                if (rect.width < 2 || rect.height < 2) return false;
                const vw = window.innerWidth || document.documentElement.clientWidth || 0;
                const vh = window.innerHeight || document.documentElement.clientHeight || 0;
                const margin = 8;
                return rect.bottom > margin && rect.right > margin && rect.top < vh - margin && rect.left < vw - margin;
            };

            const textFromNode = (node) => {
                if (!node) return "";
                const walker = document.createTreeWalker(
                    node,
                    NodeFilter.SHOW_TEXT,
                    {
                        acceptNode(textNode) {
                            const parent = textNode.parentElement;
                            if (!parent) return NodeFilter.FILTER_REJECT;
                            if (parent.isContentEditable) return NodeFilter.FILTER_REJECT;
                            if (disallowedTags.has(parent.tagName)) return NodeFilter.FILTER_REJECT;
                            return NodeFilter.FILTER_ACCEPT;
                        }
                    }
                );
                let out = "";
                while (walker.nextNode()) {
                    const value = walker.currentNode.nodeValue;
                    if (value) {
                        out += value + " ";
                    }
                }
                return out;
            };

            const labelFromAria = (el) => {
                const aria = el.getAttribute("aria-label");
                if (aria) return aria;
                const labelledBy = el.getAttribute("aria-labelledby");
                if (labelledBy) {
                    const ids = labelledBy.split(/\s+/).filter(Boolean);
                    const parts = [];
                    for (const id of ids) {
                        const target = document.getElementById(id);
                        if (target) {
                            const text = normalize(textFromNode(target));
                            if (text) parts.push(text);
                        }
                    }
                    if (parts.length) return parts.join(" ");
                }
                return "";
            };

            const labelFromLabels = (el) => {
                if (!el.labels || !el.labels.length) return "";
                const parts = [];
                for (const label of el.labels) {
                    const text = normalize(textFromNode(label));
                    if (text) parts.push(text);
                }
                return parts.join(" ");
            };

            const labelFromInput = (el) => {
                const tag = el.tagName;
                if (tag === "INPUT" || tag === "TEXTAREA") {
                    const placeholder = el.getAttribute("placeholder");
                    if (placeholder) return placeholder;
                    const name = el.getAttribute("name");
                    if (name) return name;
                }
                if (tag === "SELECT") {
                    const name = el.getAttribute("name");
                    if (name) return name;
                }
                return "";
            };

            const labelFromText = (el) => {
                const tag = el.tagName;
                if (tag === "BUTTON" || tag === "A") {
                    return normalize(textFromNode(el));
                }
                return "";
            };

            const labelFromValue = (el) => {
                if (el.tagName !== "INPUT") return "";
                const type = (el.getAttribute("type") || "text").toLowerCase();
                if (type === "button" || type === "submit" || type === "reset") {
                    return el.value || "";
                }
                return "";
            };

            const describeInteractive = (el) => {
                const role = (el.getAttribute("role") || el.tagName || "").toLowerCase();
                let label = labelFromAria(el);
                if (!label) label = labelFromLabels(el);
                if (!label) label = labelFromText(el);
                if (!label) label = labelFromInput(el);
                if (!label) label = labelFromValue(el);
                if (!label) {
                    const type = el.getAttribute("type");
                    if (type) label = type;
                }
                label = normalize(label || "");
                if (label) return `${role}: ${label}`;
                return role || "";
            };

            const closestNearbyText = (el) => {
                let node = el.parentElement;
                while (node && node !== document.body && node !== document.documentElement) {
                    if (node.matches && node.matches("form,section,article,main,fieldset,div,li")) {
                        const text = normalize(textFromNode(node));
                        if (text.length >= 20 && text.length <= 200) return text;
                        if (text.length > 200 && text.length <= 400) return text.slice(0, 200) + "...";
                    }
                    node = node.parentElement;
                }
                return "";
            };

            const chunks = [];
            const seen = new Set();
            const addChunk = (text) => {
                let normalized = normalize(text || "");
                if (!normalized) return;
                if (normalized.length > MAX_CHUNK) {
                    normalized = normalized.slice(0, MAX_CHUNK) + "...";
                }
                if (seen.has(normalized)) return;
                seen.add(normalized);
                chunks.push(normalized);
            };

            const interactive = Array.from(document.querySelectorAll(INTERACTIVE_SELECTOR))
                .filter((el) => isVisible(el) && el.getAttribute("tabindex") !== "-1");

            for (const el of interactive.slice(0, MAX_INTERACTIVE)) {
                const desc = describeInteractive(el);
                if (desc) addChunk(desc);
            }

            let nearbyCount = 0;
            for (const el of interactive) {
                if (nearbyCount >= MAX_NEARBY) break;
                const text = closestNearbyText(el);
                if (text) {
                    addChunk(text);
                    nearbyCount += 1;
                }
            }

            const statusNodes = Array.from(document.querySelectorAll(STATUS_SELECTOR))
                .filter(isVisible);
            for (const el of statusNodes.slice(0, MAX_STATUS)) {
                const text = normalize(textFromNode(el));
                if (text) addChunk(text);
            }

            const headings = Array.from(document.querySelectorAll("h1,h2,h3,legend"))
                .filter(isVisible);
            for (const el of headings.slice(0, MAX_HEADINGS)) {
                const text = normalize(textFromNode(el));
                if (text) addChunk(text);
            }

            if (!chunks.length && document.body) {
                const fallback = normalize(textFromNode(document.body));
                if (fallback) addChunk(fallback);
            }

            return chunks.join("\n");
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
    let nodes = fetch_ax_nodes(page).await?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for node in nodes {
        if node.ignored {
            continue;
        }

        let Some(backend_id) = node.backend_dom_node_id else {
            continue;
        };

        if !seen.insert(*backend_id.inner()) {
            continue;
        }

        let Some(role) = ax_value_string(node.role.as_ref()) else {
            continue;
        };
        let role = role.to_lowercase();
        if !is_actionable_role(&role) {
            continue;
        }

        let mut flags = ElementFlags::default();
        add_ax_flags(&node.properties, &mut flags);

        let name = ax_value_string(node.name.as_ref())
            .or_else(|| ax_value_string(node.description.as_ref()))
            .or_else(|| ax_value_string(node.value.as_ref()));
        let value = ax_value_string(node.value.as_ref());

        let mut bbox = [0.0, 0.0, 0.0, 0.0];
        match backend_node_bbox(page, backend_id).await {
            Ok(Some(bounds)) => bbox = bounds,
            Ok(None) => flags.bbox_missing = Some(true),
            Err(_) => flags.bbox_missing = Some(true),
        }

        let signature =
            stable_signature(&role, &name, node.node_id.inner(), node.frame_id.as_ref());

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

    let mut elements = Vec::new();
    let mut element_map = HashMap::new();

    for element in out {
        let id = stable_element_id(&element.signature);
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

    Ok(CollectedElements {
        elements,
        element_map,
    })
}

async fn fetch_ax_nodes(page: &Page) -> BrowserResult<Vec<AxNode>> {
    page.execute(EnableParams::default()).await?;
    let response = page.execute(GetFullAxTreeParams::builder().build()).await?;
    Ok(response.result.nodes)
}

fn is_actionable_role(role: &str) -> bool {
    ACTIONABLE_ROLES.iter().any(|value| role == *value)
}

fn ax_value_string(value: Option<&AxValue>) -> Option<String> {
    let value = value?.value.as_ref()?;
    let raw = match value {
        serde_json::Value::String(text) => text.trim().to_string(),
        serde_json::Value::Number(number) => number.to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        _ => return None,
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn ax_value_truthy(value: &AxValue) -> bool {
    match value.value.as_ref() {
        Some(serde_json::Value::Bool(value)) => *value,
        Some(serde_json::Value::Number(number)) => number.as_i64().unwrap_or(0) != 0,
        Some(serde_json::Value::String(text)) => {
            matches!(text.to_lowercase().as_str(), "true" | "mixed" | "on")
        }
        _ => false,
    }
}

fn ax_property_truthy(properties: &Option<Vec<AxProperty>>, name: AxPropertyName) -> bool {
    let Some(properties) = properties else {
        return false;
    };
    properties
        .iter()
        .find(|prop| prop.name == name)
        .map(|prop| ax_value_truthy(&prop.value))
        .unwrap_or(false)
}

fn add_ax_flags(properties: &Option<Vec<AxProperty>>, flags: &mut ElementFlags) {
    if ax_property_truthy(properties, AxPropertyName::Disabled) {
        flags.disabled = Some(true);
    }
    if ax_property_truthy(properties, AxPropertyName::Readonly) {
        flags.readonly = Some(true);
    }
    if ax_property_truthy(properties, AxPropertyName::Required) {
        flags.required = Some(true);
    }
    if ax_property_truthy(properties, AxPropertyName::Focused) {
        flags.focused = Some(true);
    }
    if ax_property_truthy(properties, AxPropertyName::Editable) {
        flags.editable = Some(true);
    }
    if ax_property_truthy(properties, AxPropertyName::Checked) {
        flags.checked = Some(true);
    }
    if ax_property_truthy(properties, AxPropertyName::Selected) {
        flags.selected = Some(true);
    }
    if ax_property_truthy(properties, AxPropertyName::Expanded) {
        flags.expanded = Some(true);
    }
    if ax_property_truthy(properties, AxPropertyName::Pressed) {
        flags.pressed = Some(true);
    }
}

async fn backend_node_bbox(
    page: &Page,
    backend_node_id: BackendNodeId,
) -> BrowserResult<Option<[f64; 4]>> {
    let model = page
        .execute(
            GetBoxModelParams::builder()
                .backend_node_id(backend_node_id)
                .build(),
        )
        .await?
        .result
        .model;
    Ok(quad_bbox(model.border.inner()))
}

fn quad_bbox(quad: &[f64]) -> Option<[f64; 4]> {
    if quad.len() != 8 {
        return None;
    }
    let xs = [quad[0], quad[2], quad[4], quad[6]];
    let ys = [quad[1], quad[3], quad[5], quad[7]];
    let (min_x, max_x) = xs
        .iter()
        .fold((xs[0], xs[0]), |acc, x| (acc.0.min(*x), acc.1.max(*x)));
    let (min_y, max_y) = ys
        .iter()
        .fold((ys[0], ys[0]), |acc, y| (acc.0.min(*y), acc.1.max(*y)));
    Some([min_x, min_y, max_x - min_x, max_y - min_y])
}

fn stable_signature(
    role: &str,
    name: &Option<String>,
    ax_node_id: &str,
    frame_id: Option<&chromiumoxide_cdp::cdp::browser_protocol::page::FrameId>,
) -> String {
    let mut parts = Vec::new();
    parts.push("ax".to_string());
    parts.push(role.trim().to_lowercase());
    if let Some(name) = name
        .as_ref()
        .map(|value| value.trim())
        .filter(|v| !v.is_empty())
    {
        parts.push(name.to_string());
    }
    if let Some(frame_id) = frame_id {
        parts.push(format!("frame={}", frame_id.inner()));
    }
    parts.push(format!("node={}", ax_node_id));
    parts.join("|")
}

fn stable_element_id(signature: &str) -> String {
    format!("el_{}", hash_hex(signature))
}

fn compute_state_hash(url: &str, title: &str, elements: &[ElementRef]) -> String {
    const TOP_ELEMENTS: usize = 20;
    let mut signature = String::new();
    signature.push_str("url=");
    signature.push_str(&normalize_text(url));
    signature.push('\n');
    signature.push_str("title=");
    signature.push_str(&normalize_text(title));

    let mut element_signatures: Vec<String> =
        elements.iter().map(element_signature_for_hash).collect();
    element_signatures.sort();

    for element_signature in element_signatures.into_iter().take(TOP_ELEMENTS) {
        signature.push('\n');
        signature.push_str(&element_signature);
    }

    hash_hex(&signature)
}

fn element_signature_for_hash(element: &ElementRef) -> String {
    let mut out = String::new();
    out.push_str("role=");
    out.push_str(&normalize_text(&element.role));

    if let Some(name) = element
        .name
        .as_ref()
        .map(|value| normalize_text(value))
        .filter(|value| !value.is_empty())
    {
        out.push('|');
        out.push_str("name=");
        out.push_str(&name);
    }

    if let Some(value) = element
        .value
        .as_ref()
        .map(|value| normalize_text(value))
        .filter(|value| !value.is_empty())
    {
        out.push('|');
        out.push_str("value=");
        out.push_str(&value);
    }

    let mut flags = Vec::new();
    if element.flags.disabled.unwrap_or(false) {
        flags.push("disabled");
    }
    if element.flags.readonly.unwrap_or(false) {
        flags.push("readonly");
    }
    if element.flags.required.unwrap_or(false) {
        flags.push("required");
    }
    if element.flags.focused.unwrap_or(false) {
        flags.push("focused");
    }
    if element.flags.editable.unwrap_or(false) {
        flags.push("editable");
    }
    if element.flags.checked.unwrap_or(false) {
        flags.push("checked");
    }
    if element.flags.selected.unwrap_or(false) {
        flags.push("selected");
    }
    if element.flags.expanded.unwrap_or(false) {
        flags.push("expanded");
    }
    if element.flags.pressed.unwrap_or(false) {
        flags.push("pressed");
    }
    if element.flags.bbox_missing.unwrap_or(false) {
        flags.push("bbox_missing");
    }

    if !flags.is_empty() {
        out.push('|');
        out.push_str("flags=");
        out.push_str(&flags.join(","));
    }

    out
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

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ax_value_string_reads_value() {
        let value = AxValue::builder()
            .r#type(chromiumoxide_cdp::cdp::browser_protocol::accessibility::AxValueType::String)
            .value("Save")
            .build()
            .expect("value");
        assert_eq!(ax_value_string(Some(&value)), Some("Save".to_string()));
    }

    #[test]
    fn ax_property_truthy_detects_flags() {
        let value = AxValue::builder()
            .r#type(chromiumoxide_cdp::cdp::browser_protocol::accessibility::AxValueType::String)
            .value(true)
            .build()
            .expect("value");
        let prop = AxProperty::builder()
            .name(AxPropertyName::Disabled)
            .value(value)
            .build()
            .expect("prop");
        let props = Some(vec![prop]);
        assert!(ax_property_truthy(&props, AxPropertyName::Disabled));
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
        let id_a = stable_element_id(signature);
        let id_b = stable_element_id(signature);
        assert_eq!(id_a, id_b);
    }

    #[test]
    fn stable_element_id_distinguishes_signature() {
        let signature = "button|button|Save|id=save-btn";
        let id_a = stable_element_id(signature);
        let id_b = stable_element_id("button|button|Cancel|id=cancel-btn");
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
            flags: ElementFlags::default(),
        }];
        let hash_a = compute_state_hash("https://a.example", "Title", &elements);
        let hash_b = compute_state_hash("https://b.example", "Title", &elements);
        assert_ne!(hash_a, hash_b);
    }

    #[test]
    fn state_hash_is_order_insensitive() {
        let element_a = ElementRef {
            id: "el_a".to_string(),
            role: "button".to_string(),
            name: Some("Save".to_string()),
            value: None,
            bbox: [0.0, 0.0, 10.0, 10.0],
            flags: ElementFlags::default(),
        };
        let element_b = ElementRef {
            id: "el_b".to_string(),
            role: "textbox".to_string(),
            name: Some("Email".to_string()),
            value: Some("me@example.com".to_string()),
            bbox: [0.0, 0.0, 10.0, 10.0],
            flags: ElementFlags::default(),
        };
        let hash_a = compute_state_hash(
            "https://a.example",
            "Title",
            &[element_a.clone(), element_b.clone()],
        );
        let hash_b = compute_state_hash("https://a.example", "Title", &[element_b, element_a]);
        assert_eq!(hash_a, hash_b);
    }

    #[test]
    fn state_hash_changes_with_value_change() {
        let element = ElementRef {
            id: "el_a".to_string(),
            role: "textbox".to_string(),
            name: Some("Email".to_string()),
            value: Some("me@example.com".to_string()),
            bbox: [0.0, 0.0, 10.0, 10.0],
            flags: ElementFlags::default(),
        };
        let mut updated = element.clone();
        updated.value = Some("me+alt@example.com".to_string());
        let hash_a = compute_state_hash("https://a.example", "Title", &[element]);
        let hash_b = compute_state_hash("https://a.example", "Title", &[updated]);
        assert_ne!(hash_a, hash_b);
    }
}
