use anyhow::{anyhow, bail, Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::layout::Point;
use chromiumoxide::page::Page;
use chromiumoxide_cdp::cdp::browser_protocol::dom::{BackendNodeId, GetBoxModelParams, NodeId};
use futures::StreamExt;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;
use std::time::Instant;
use tracing::{info, warn};

const ACTIONABLE_SELECTOR: &str = "a[href], button, input, select, textarea, \
[role=button], [role=link], [role=checkbox], [role=radio], [role=tab], [role=combobox]";

#[derive(Debug)]
struct Args {
    url: String,
    headful: bool,
    backend_node_id: Option<i64>,
    click_index: Option<usize>,
    limit: usize,
}

#[derive(Debug)]
struct ActionableElement {
    backend_node_id: BackendNodeId,
    node_id: NodeId,
    tag: String,
    role: Option<String>,
    name: Option<String>,
    href: Option<String>,
    input_type: Option<String>,
    text: Option<String>,
    value: Option<String>,
    disabled: bool,
}

#[derive(Debug, Default, Deserialize)]
struct JsElementInfo {
    text: Option<String>,
    value: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .init();

    let args = parse_args()?;

    let mut config = BrowserConfig::builder();
    if args.headful {
        config = config.with_head();
    }
    let (mut browser, mut handler) =
        Browser::launch(config.build().map_err(|err| anyhow!(err))?).await?;
    let handler_task = tokio::spawn(async move {
        while let Some(_event) = handler.next().await {}
    });

    let page = browser.new_page("about:blank").await?;
    page.goto(args.url.as_str()).await?;

    let snapshot_start = Instant::now();
    let elements = collect_actionable(&page, args.limit).await?;
    let snapshot_ms = snapshot_start.elapsed().as_millis();
    info!(
        snapshot_ms,
        actionable_count = elements.len(),
        "snapshot complete"
    );

    print_elements(&elements);

    let target = pick_target(&elements, args.backend_node_id, args.click_index)?;
    info!(
        backend_node_id = *target.backend_node_id.inner(),
        node_id = *target.node_id.inner(),
        "clicking target"
    );

    let click_start = Instant::now();
    click_by_backend_node_id(&page, target.backend_node_id).await?;
    let click_ms = click_start.elapsed().as_millis();
    info!(
        click_ms,
        backend_node_id = *target.backend_node_id.inner(),
        "click complete"
    );

    browser.close().await?;
    handler_task.await?;
    Ok(())
}

fn parse_args() -> Result<Args> {
    let mut url: Option<String> = None;
    let mut headful = false;
    let mut backend_node_id = None;
    let mut click_index = None;
    let mut limit = 25usize;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--url" => {
                url = Some(
                    args.next()
                        .ok_or_else(|| anyhow!("--url requires a value"))?,
                );
            }
            "--headful" => headful = true,
            "--backend-node-id" => {
                let raw = args
                    .next()
                    .ok_or_else(|| anyhow!("--backend-node-id requires a value"))?;
                backend_node_id = Some(raw.parse::<i64>()?);
            }
            "--click-index" => {
                let raw = args
                    .next()
                    .ok_or_else(|| anyhow!("--click-index requires a value"))?;
                click_index = Some(raw.parse::<usize>()?);
            }
            "--limit" => {
                let raw = args
                    .next()
                    .ok_or_else(|| anyhow!("--limit requires a value"))?;
                limit = raw.parse::<usize>()?;
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            _ => bail!("unknown argument: {arg}"),
        }
    }

    let url = match url {
        Some(value) => value,
        None => demo_url()?,
    };

    Ok(Args {
        url,
        headful,
        backend_node_id,
        click_index,
        limit,
    })
}

fn demo_url() -> Result<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let demo_path = root.join("demo").join("demo.html");
    let demo_path = demo_path
        .canonicalize()
        .with_context(|| format!("missing demo file at {}", demo_path.display()))?;
    Ok(format!("file://{}", demo_path.display()))
}

fn print_usage() {
    println!(
        "Usage: cdp_snapshot [--url <url>] [--headful] [--backend-node-id <id>] [--click-index <n>] [--limit <n>]"
    );
}

async fn collect_actionable(page: &Page, limit: usize) -> Result<Vec<ActionableElement>> {
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
        let js_info = fetch_js_info(&element).await.unwrap_or_default();

        let disabled = attrs.contains_key("disabled")
            || attrs
                .get("aria-disabled")
                .map(|value| value == "true")
                .unwrap_or(false);

        let name = derive_name(&attrs, &js_info);

        out.push(ActionableElement {
            backend_node_id: backend_id,
            node_id: node.node_id,
            tag: node.node_name.to_lowercase(),
            role: attrs.get("role").cloned(),
            name,
            href: attrs.get("href").cloned(),
            input_type: attrs.get("type").cloned(),
            text: js_info.text,
            value: js_info.value,
            disabled,
        });

        if out.len() >= limit {
            break;
        }
    }

    Ok(out)
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

async fn fetch_js_info(element: &chromiumoxide::element::Element) -> Result<JsElementInfo> {
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
    serde_json::from_value(value).map_err(|err| err.into())
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

fn print_elements(elements: &[ActionableElement]) {
    println!("Actionable elements ({}):", elements.len());
    for (idx, element) in elements.iter().enumerate() {
        println!(
            "[{idx:02}] backend_node_id={} node_id={} tag={} role={} name={} text={} value={} href={} type={} disabled={}",
            element.backend_node_id.inner(),
            element.node_id.inner(),
            element.tag,
            element.role.as_deref().unwrap_or("-"),
            element.name.as_deref().map(|v| truncate(v, 40)).unwrap_or("-".to_string()),
            element.text.as_deref().map(|v| truncate(v, 40)).unwrap_or("-".to_string()),
            element.value.as_deref().map(|v| truncate(v, 40)).unwrap_or("-".to_string()),
            element.href.as_deref().map(|v| truncate(v, 40)).unwrap_or("-".to_string()),
            element.input_type.as_deref().unwrap_or("-"),
            element.disabled
        );
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let mut out = String::new();
    let keep = max.saturating_sub(3);
    for (idx, ch) in value.chars().enumerate() {
        if idx >= keep {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}

fn pick_target(
    elements: &[ActionableElement],
    backend_node_id: Option<i64>,
    click_index: Option<usize>,
) -> Result<&ActionableElement> {
    if elements.is_empty() {
        bail!("no actionable elements found");
    }

    if let Some(id) = backend_node_id {
        let found = elements
            .iter()
            .find(|element| *element.backend_node_id.inner() == id);
        return found.ok_or_else(|| anyhow!("backend node id not found: {id}"));
    }

    if let Some(index) = click_index {
        return elements
            .get(index)
            .ok_or_else(|| anyhow!("click index out of bounds: {index}"));
    }

    if elements[0].disabled {
        warn!("first element is disabled; consider choosing another with --click-index");
    }
    Ok(&elements[0])
}

async fn click_by_backend_node_id(page: &Page, backend_node_id: BackendNodeId) -> Result<()> {
    let model = page
        .execute(
            GetBoxModelParams::builder()
                .backend_node_id(backend_node_id)
                .build(),
        )
        .await?
        .result
        .model;
    let (x, y) = quad_center(model.border.inner())
        .ok_or_else(|| anyhow!("invalid box model for backend node id"))?;
    page.click(Point::new(x, y)).await?;
    Ok(())
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
    fn quad_center_returns_midpoint() {
        let quad = vec![0.0, 0.0, 10.0, 0.0, 10.0, 20.0, 0.0, 20.0];
        let center = quad_center(&quad).unwrap();
        assert_eq!(center.0, 5.0);
        assert_eq!(center.1, 10.0);
    }
}
