# 2026-02-11 CDP-003 Accessibility actionable nodes

- Snapshot now uses `Accessibility.getFullAXTree` with `Accessibility.enable` to pull AX nodes.
- Actionable elements are filtered by interactive AX roles (button/link/textbox/combobox/etc), then mapped to `ElementRef` with role + accessible name.
- Element refs store backend DOM node ids from AX nodes; bbox comes from `DOM.getBoxModel` using the backend id.
- E2E harness updated to assert at least 10 actionable nodes, with roles now aligned to AX (`textbox`, `combobox`).
