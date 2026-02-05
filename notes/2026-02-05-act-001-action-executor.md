# 2026-02-05 ACT-001 action executor

- `ActionApplier` now supports select, extract, and done actions alongside click/type.
- Select uses `DOM.resolveNode` + `Runtime.callFunctionOn` to set values and
  dispatch input/change events on the target element.
- Extract runs a small JS helper that supports CSS selectors with a text-search
  fallback (first 2000 nodes) and returns an error when no match is found.
- Done is a no-op at the browser layer to allow the agent loop to terminate.
- JS result parsing returns structured action errors for select/extract failures.
