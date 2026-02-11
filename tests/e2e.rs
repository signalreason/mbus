use mbus::browser::{Browser, CdpBrowser, CdpConfig};
use mbus::types::{Action, ElementRef, Observation};
use mbus::verify::Validator;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};

const HARNESS_HTML: &str = r##"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>mbus e2e</title>
  <style>
    .grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; }
    .card { padding: 6px; border: 1px solid #ddd; }
  </style>
</head>
<body>
  <h1>Harness</h1>
  <div id="status" role="status">ready</div>
  <button id="click-btn" aria-label="Click Button">Click Me</button>
  <label for="name-input">Name</label>
  <input id="name-input" aria-label="Name" type="text" />
  <label for="choice-select">Choice</label>
  <select id="choice-select" aria-label="Choice">
    <option value="">Pick</option>
    <option value="alpha">Alpha</option>
    <option value="beta">Beta</option>
  </select>
  <div class="grid">
    <button id="extra-btn-1">Extra 1</button>
    <button id="extra-btn-2">Extra 2</button>
    <button id="extra-btn-3">Extra 3</button>
    <a id="extra-link-1" href="#">Link One</a>
    <a id="extra-link-2" href="#">Link Two</a>
    <label class="card"><input type="checkbox" aria-label="Agree" /> Agree</label>
    <label class="card"><input type="radio" name="group" aria-label="Pick A" /> Pick A</label>
    <label class="card"><input type="radio" name="group" aria-label="Pick B" /> Pick B</label>
  </div>
  <script>
    const status = document.getElementById('status');
    document.getElementById('click-btn').addEventListener('click', () => {
      status.textContent = 'clicked';
    });
    document.getElementById('name-input').addEventListener('input', (event) => {
      status.textContent = `typed:${event.target.value}`;
    });
    document.getElementById('choice-select').addEventListener('change', (event) => {
      status.textContent = `selected:${event.target.value}`;
    });
  </script>
</body>
</html>
"##;

struct TestServer {
    addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl TestServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("server addr");
        let (shutdown, mut shutdown_rx) = oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        break;
                    }
                    accept = listener.accept() => {
                        match accept {
                            Ok((socket, _)) => {
                                tokio::spawn(async move {
                                    let _ = handle_connection(socket).await;
                                });
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
        });

        Self {
            addr,
            shutdown: Some(shutdown),
            handle: Some(handle),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    async fn shutdown(mut self) {
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

async fn handle_connection(mut socket: TcpStream) -> std::io::Result<()> {
    let mut buffer = [0u8; 8192];
    let read = socket.read(&mut buffer).await?;
    if read == 0 {
        return Ok(());
    }
    let request = String::from_utf8_lossy(&buffer[..read]);
    let mut parts = request.lines().next().unwrap_or_default().split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let raw_path = parts.next().unwrap_or("/");
    let path = raw_path.split('?').next().unwrap_or("/");

    let (status, body, content_type) = route_request(method, path);
    let body_bytes = body.as_bytes();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body_bytes.len(),
        body
    );
    socket.write_all(response.as_bytes()).await?;
    Ok(())
}

fn route_request(method: &str, path: &str) -> (&'static str, &'static str, &'static str) {
    if method != "GET" {
        return ("405 Method Not Allowed", "method not allowed", "text/plain");
    }
    match path {
        "/" | "/harness" => ("200 OK", HARNESS_HTML, "text/html; charset=utf-8"),
        "/favicon.ico" => ("404 Not Found", "not found", "text/plain"),
        _ => ("404 Not Found", "not found", "text/plain"),
    }
}

fn find_element<'a>(
    observation: &'a Observation,
    role: &str,
    name: &str,
) -> &'a ElementRef {
    observation
        .elements
        .iter()
        .find(|element| {
            element.role.eq_ignore_ascii_case(role)
                && element
                    .name
                    .as_deref()
                    .map(|value| value.eq_ignore_ascii_case(name))
                    .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("missing element role={role} name={name}"))
}

fn find_element_by_roles<'a>(
    observation: &'a Observation,
    roles: &[&str],
    name: &str,
) -> &'a ElementRef {
    observation
        .elements
        .iter()
        .find(|element| {
            roles.iter().any(|role| element.role.eq_ignore_ascii_case(role))
                && element
                    .name
                    .as_deref()
                    .map(|value| value.eq_ignore_ascii_case(name))
                    .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("missing element roles={roles:?} name={name}"))
}

async fn wait_for_visible_text(
    browser: &CdpBrowser,
    needle: &str,
) -> Observation {
    for _ in 0..20 {
        let snapshot = browser.snapshot().await.expect("snapshot retry");
        if snapshot.visible_text.contains(needle) {
            return snapshot;
        }
        sleep(Duration::from_millis(100)).await;
    }
    panic!("expected visible text to include {needle}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_snapshot_metadata() {
    let server = TestServer::start().await;
    let url = server.url("/harness");
    let config = CdpConfig {
        initial_url: url.clone(),
        ..CdpConfig::default()
    };
    let browser = CdpBrowser::launch(config).await.expect("launch browser");

    let snapshot = browser.snapshot().await.expect("snapshot");
    assert_eq!(snapshot.url, url, "snapshot should report page url");
    assert_eq!(snapshot.title, "mbus e2e", "snapshot should report title");
    assert!(
        snapshot.viewport[0] > 0 && snapshot.viewport[1] > 0,
        "snapshot should report non-zero viewport"
    );

    browser.shutdown().await.expect("shutdown browser");
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_snapshot_actionable_nodes() {
    let server = TestServer::start().await;
    let url = server.url("/harness");
    let config = CdpConfig {
        initial_url: url,
        ..CdpConfig::default()
    };
    let browser = CdpBrowser::launch(config).await.expect("launch browser");

    let snapshot = browser.snapshot().await.expect("snapshot");
    assert!(
        snapshot.elements.len() >= 10,
        "expected at least 10 actionable elements, got {}",
        snapshot.elements.len()
    );

    browser.shutdown().await.expect("shutdown browser");
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_click_type_select() {
    let server = TestServer::start().await;
    let url = server.url("/harness");
    let config = CdpConfig {
        initial_url: url,
        ..CdpConfig::default()
    };
    let browser = CdpBrowser::launch(config).await.expect("launch browser");
    let validator = Validator::default();

    let snapshot = browser.snapshot().await.expect("initial snapshot");
    let click_id = find_element(&snapshot, "button", "Click Button").id.clone();
    let click = Action::Click { id: click_id };
    validator.validate(&click, &snapshot).expect("valid click");
    let step = browser.apply(&click).await.expect("apply click");
    assert!(step.ok);

    let snapshot = browser.snapshot().await.expect("snapshot after click");
    assert!(
        snapshot.visible_text.contains("clicked"),
        "visible text should reflect click"
    );

    let type_id = find_element_by_roles(&snapshot, &["textbox", "searchbox"], "Name")
        .id
        .clone();
    let focus = Action::Click { id: type_id.clone() };
    validator.validate(&focus, &snapshot).expect("valid focus");
    let step = browser.apply(&focus).await.expect("apply focus");
    assert!(step.ok);

    let typed = Action::Type {
        id: type_id,
        text: "Ada Lovelace".to_string(),
        submit: Some(false),
    };
    validator.validate(&typed, &snapshot).expect("valid type");
    let step = browser.apply(&typed).await.expect("apply type");
    assert!(step.ok);

    let snapshot = wait_for_visible_text(&browser, "typed:Ada Lovelace").await;

    let select_id = find_element_by_roles(&snapshot, &["combobox", "listbox"], "Choice")
        .id
        .clone();
    let select = Action::Select {
        id: select_id,
        value: "beta".to_string(),
    };
    validator.validate(&select, &snapshot).expect("valid select");
    let step = browser.apply(&select).await.expect("apply select");
    assert!(step.ok);

    let snapshot = wait_for_visible_text(&browser, "selected:beta").await;
    assert!(
        snapshot.visible_text.contains("selected:beta"),
        "visible text should reflect select change"
    );

    browser.shutdown().await.expect("shutdown browser");
    server.shutdown().await;
}
