# 2026-02-05 BROW-001 browser adapter (CDP)

- Implemented `Browser` trait with async `snapshot`, `apply`, and `shutdown`.
- Added `CdpBrowser` using chromiumoxide 0.8.0 with `CdpConfig` (headful flag, initial URL, snapshot/action timeouts, max elements/text).
- Observation snapshot uses the actionable selector from the spike; element ids are `el_<backend_node_id>`; role falls back to tag name.
- Element flags include `disabled`, `bbox_missing`, and `js_info_missing` for partial data.
- ActionApplier supports click/type/scroll/wait/navigate/back; unsupported actions return `unsupported_action`.
- Click uses `DOM.getBoxModel` quad center; type focuses by backend node id and uses `Input.insertText` with optional Enter submit.
- Visible text is `document.body.innerText` truncated to max length; viewport comes from layout metrics.
