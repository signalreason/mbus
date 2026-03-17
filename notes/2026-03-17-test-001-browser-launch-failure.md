# 2026-03-17 TEST-001 browser launch failure in Codex environment

- `cargo test` runs through unit tests and non-browser integration tests, but `tests/challenge_integration.rs` fails in this environment before any task execution.
- Failure mode is `cdp_launch_failed`: the Chrome process exits immediately with status `ExitStatus(unix_wait_status(6))` while `chromiumoxide` is resolving the websocket URL.
- The failure affects existing bench/challenge browser-backed integration tests, not only the newly added adversarial case.
- A Chrome binary exists at `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`, so the current issue is launch/runtime behavior, not binary discovery.
- Practical implication: doc and harness changes can be verified locally, but browser-backed challenge proof still needs an environment where Chromium launches cleanly.
