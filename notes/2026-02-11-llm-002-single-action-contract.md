# 2026-02-11 LLM-002 single-action JSON contract

- OpenAI adapter now enforces a strict single-action JSON object at the boundary.
- Non-JSON outputs return `invalid_json`; JSON arrays or action wrappers with arrays return `multi_action`.
- Repair is only attempted after a valid JSON object parses; non-JSON and multi-action outputs skip repair.
- System prompt explicitly forbids arrays, code fences, and extra text.
