#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
REPORT_DIR="${ROOT_DIR}/target/challenge/proof-${TIMESTAMP}"
REPORT_PATH="${REPORT_DIR}/report.json"
PACKAGE_DIR="${ROOT_DIR}/target/challenge/package/proof-${TIMESTAMP}"
ZIP_PATH="${ROOT_DIR}/target/challenge/package/proof-${TIMESTAMP}.zip"

if [[ -z "${MBUS_LLM_API_KEY:-}" ]]; then
  echo "MBUS_LLM_API_KEY is required." >&2
  exit 1
fi

if [[ -z "${MBUS_LLM_INPUT_COST_PER_MILLION:-}" ]]; then
  echo "MBUS_LLM_INPUT_COST_PER_MILLION is required for proof runs." >&2
  exit 1
fi

if [[ -z "${MBUS_LLM_OUTPUT_COST_PER_MILLION:-}" ]]; then
  echo "MBUS_LLM_OUTPUT_COST_PER_MILLION is required for proof runs." >&2
  exit 1
fi

mkdir -p "${REPORT_DIR}" "${ROOT_DIR}/target/challenge/package"

challenge_args=(
  cargo run --bin mbus -- challenge
  --headless true
  --report-path "${REPORT_PATH}"
  --llm-api-key "${MBUS_LLM_API_KEY}"
  --llm-input-cost-per-million "${MBUS_LLM_INPUT_COST_PER_MILLION}"
  --llm-output-cost-per-million "${MBUS_LLM_OUTPUT_COST_PER_MILLION}"
)

if [[ -n "${MBUS_LLM_BASE_URL:-}" ]]; then
  challenge_args+=(--llm-base-url "${MBUS_LLM_BASE_URL}")
fi

if [[ -n "${MBUS_LLM_MODEL_FAST:-}" ]]; then
  challenge_args+=(--llm-model-fast "${MBUS_LLM_MODEL_FAST}")
fi

if [[ -n "${MBUS_LLM_MODEL_MID:-}" ]]; then
  challenge_args+=(--llm-model-mid "${MBUS_LLM_MODEL_MID}")
fi

if [[ -n "${MBUS_LLM_MODEL_STRONG:-}" ]]; then
  challenge_args+=(--llm-model-strong "${MBUS_LLM_MODEL_STRONG}")
fi

if (($# > 0)); then
  challenge_args+=("$@")
fi

(
  cd "${ROOT_DIR}"
  "${challenge_args[@]}"
)

(
  cd "${ROOT_DIR}"
  cargo run --bin mbus -- package \
    --report-path "${REPORT_PATH}" \
    --output-dir "${PACKAGE_DIR}" \
    --zip-path "${ZIP_PATH}"
)

echo "challenge_report=${REPORT_PATH}"
echo "challenge_package_dir=${PACKAGE_DIR}"
echo "challenge_package_zip=${ZIP_PATH}"
