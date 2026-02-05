# SPIKE-001 CDP snapshot feasibility

## What worked
- `chromiumoxide` v0.8.0 provides `Page::find_elements` and `Element::description` for DOM data.
- `Element.backend_node_id` gives a stable CDP backend node id.
- Clicking by backend node id can be done by:
  - `DOM.getBoxModel` via `GetBoxModelParams` and computing the quad center
  - `Page::click(Point)` with that center

## Harness
- Spike harness lives in `spikes/cdp_snapshot`.
- Default URL is a local demo page at `spikes/cdp_snapshot/demo/demo.html` with >10 actionable elements.
- Run from repo root:
  - `cargo run --manifest-path spikes/cdp_snapshot/Cargo.toml`
  - Optional flags: `--headful`, `--url <url>`, `--click-index <n>`, `--backend-node-id <id>`, `--limit <n>`.

## Timing notes
- Snapshot timing measured around element collection.
- Click timing measured around the backend-node-id click path.

