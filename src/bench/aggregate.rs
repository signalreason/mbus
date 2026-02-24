use crate::agent::memory::StepRecord;
use crate::config::LlmMode;

use super::{BenchCostSummary, BenchPricing, BenchTaskResult, BenchTokenUsage};

pub fn aggregate_usage_from_steps(steps: &[StepRecord], mode: &LlmMode) -> BenchTokenUsage {
    if steps.is_empty() {
        return BenchTokenUsage {
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            error: Some("no_llm_calls".to_string()),
        };
    }

    let mut prompt_total: u64 = 0;
    let mut completion_total: u64 = 0;
    let mut total_total: u64 = 0;
    let mut missing_calls: usize = 0;
    for step in steps {
        match step.llm_usage.as_ref() {
            Some(usage) => match (
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.total_tokens,
            ) {
                (Some(prompt), Some(completion), Some(total)) => {
                    prompt_total = prompt_total.saturating_add(prompt);
                    completion_total = completion_total.saturating_add(completion);
                    total_total = total_total.saturating_add(total);
                }
                _ => missing_calls = missing_calls.saturating_add(1),
            },
            None => missing_calls = missing_calls.saturating_add(1),
        }
    }

    if missing_calls > 0 {
        let error = match mode {
            LlmMode::Scripted => format!(
                "usage_unavailable_for_scripted_mode (missing {missing_calls}/{total} calls)",
                total = steps.len()
            ),
            LlmMode::OpenAi => format!(
                "missing_usage_for_{missing_calls}/{total} calls",
                total = steps.len()
            ),
            LlmMode::Stub => format!(
                "usage_unavailable_for_stub_mode (missing {missing_calls}/{total} calls)",
                total = steps.len()
            ),
        };
        return BenchTokenUsage {
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            error: Some(error),
        };
    }

    BenchTokenUsage {
        prompt_tokens: Some(prompt_total),
        completion_tokens: Some(completion_total),
        total_tokens: Some(total_total),
        error: None,
    }
}

pub fn aggregate_usage_from_results(results: &[BenchTaskResult]) -> BenchTokenUsage {
    if results.is_empty() {
        return BenchTokenUsage {
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            error: Some("no_tasks".to_string()),
        };
    }

    let mut prompt_total: u64 = 0;
    let mut completion_total: u64 = 0;
    let mut total_total: u64 = 0;
    let mut missing_tasks: usize = 0;
    for result in results {
        let usage = &result.usage;
        if usage.error.is_some() {
            missing_tasks = missing_tasks.saturating_add(1);
            continue;
        }
        match (
            usage.prompt_tokens,
            usage.completion_tokens,
            usage.total_tokens,
        ) {
            (Some(prompt), Some(completion), Some(total)) => {
                prompt_total = prompt_total.saturating_add(prompt);
                completion_total = completion_total.saturating_add(completion);
                total_total = total_total.saturating_add(total);
            }
            _ => missing_tasks = missing_tasks.saturating_add(1),
        }
    }

    if missing_tasks > 0 {
        return BenchTokenUsage {
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            error: Some(format!(
                "missing_usage_for_{missing_tasks}/{total} tasks",
                total = results.len()
            )),
        };
    }

    BenchTokenUsage {
        prompt_tokens: Some(prompt_total),
        completion_tokens: Some(completion_total),
        total_tokens: Some(total_total),
        error: None,
    }
}

pub fn estimate_cost(usage: &BenchTokenUsage, pricing: Option<BenchPricing>) -> BenchCostSummary {
    let pricing = match pricing {
        Some(pricing) => pricing,
        None => {
            return BenchCostSummary {
                pricing: None,
                input_cost_usd: None,
                output_cost_usd: None,
                total_cost_usd: None,
                error: Some("missing_pricing".to_string()),
            };
        }
    };

    if pricing.input_cost_per_million < 0.0 || pricing.output_cost_per_million < 0.0 {
        return BenchCostSummary {
            pricing: Some(pricing),
            input_cost_usd: None,
            output_cost_usd: None,
            total_cost_usd: None,
            error: Some("invalid_pricing".to_string()),
        };
    }

    if let Some(err) = usage.error.as_ref() {
        return BenchCostSummary {
            pricing: Some(pricing),
            input_cost_usd: None,
            output_cost_usd: None,
            total_cost_usd: None,
            error: Some(format!("usage_error: {err}")),
        };
    }

    let Some(prompt_tokens) = usage.prompt_tokens else {
        return BenchCostSummary {
            pricing: Some(pricing),
            input_cost_usd: None,
            output_cost_usd: None,
            total_cost_usd: None,
            error: Some("missing_prompt_tokens".to_string()),
        };
    };
    let Some(completion_tokens) = usage.completion_tokens else {
        return BenchCostSummary {
            pricing: Some(pricing),
            input_cost_usd: None,
            output_cost_usd: None,
            total_cost_usd: None,
            error: Some("missing_completion_tokens".to_string()),
        };
    };

    let input_cost = (prompt_tokens as f64 / 1_000_000.0) * pricing.input_cost_per_million;
    let output_cost = (completion_tokens as f64 / 1_000_000.0) * pricing.output_cost_per_million;
    let total_cost = input_cost + output_cost;

    BenchCostSummary {
        pricing: Some(pricing),
        input_cost_usd: Some(input_cost),
        output_cost_usd: Some(output_cost),
        total_cost_usd: Some(total_cost),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::memory::{StepOutcomeLog, StepTimings, ValidationOutcome};
    use crate::types::{Action, StepResult, TokenUsage};

    fn step_with_usage(usage: Option<TokenUsage>) -> StepRecord {
        StepRecord {
            action: Action::Done {
                summary: "ok".to_string(),
            },
            validation: ValidationOutcome::success(),
            result: StepResult {
                ok: true,
                error: None,
                diagnostics: Vec::new(),
                new_state_hash: None,
                scroll: None,
                extract: None,
            },
            outcome: StepOutcomeLog::Done,
            timings: StepTimings {
                step_duration_ms: 0,
                llm_duration_ms: 0,
                apply_duration_ms: 0,
                snapshot_duration_ms: 0,
            },
            llm_usage: usage,
        }
    }

    #[test]
    fn estimate_cost_uses_per_million_pricing() {
        let usage = BenchTokenUsage {
            prompt_tokens: Some(1_000_000),
            completion_tokens: Some(2_000_000),
            total_tokens: Some(3_000_000),
            error: None,
        };
        let pricing = BenchPricing {
            input_cost_per_million: 1.0,
            output_cost_per_million: 2.0,
        };

        let cost = estimate_cost(&usage, Some(pricing));

        assert!(cost.error.is_none());
        let input = cost.input_cost_usd.expect("input cost");
        let output = cost.output_cost_usd.expect("output cost");
        let total = cost.total_cost_usd.expect("total cost");
        assert!((input - 1.0).abs() < 1e-9);
        assert!((output - 4.0).abs() < 1e-9);
        assert!((total - 5.0).abs() < 1e-9);
    }

    #[test]
    fn estimate_cost_requires_pricing() {
        let usage = BenchTokenUsage {
            prompt_tokens: Some(10),
            completion_tokens: Some(20),
            total_tokens: Some(30),
            error: None,
        };

        let cost = estimate_cost(&usage, None);

        assert_eq!(cost.error.as_deref(), Some("missing_pricing"));
        assert!(cost.total_cost_usd.is_none());
    }

    #[test]
    fn estimate_cost_rejects_negative_pricing() {
        let usage = BenchTokenUsage {
            prompt_tokens: Some(1),
            completion_tokens: Some(1),
            total_tokens: Some(2),
            error: None,
        };
        let pricing = BenchPricing {
            input_cost_per_million: -0.1,
            output_cost_per_million: 1.0,
        };

        let cost = estimate_cost(&usage, Some(pricing));

        assert_eq!(cost.error.as_deref(), Some("invalid_pricing"));
        assert!(cost.total_cost_usd.is_none());
    }

    #[test]
    fn estimate_cost_reports_usage_error() {
        let usage = BenchTokenUsage {
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            error: Some("missing_usage_for_1/2 calls".to_string()),
        };
        let pricing = BenchPricing {
            input_cost_per_million: 1.0,
            output_cost_per_million: 1.0,
        };

        let cost = estimate_cost(&usage, Some(pricing));

        assert_eq!(
            cost.error.as_deref(),
            Some("usage_error: missing_usage_for_1/2 calls")
        );
        assert!(cost.total_cost_usd.is_none());
    }

    #[test]
    fn estimate_cost_requires_prompt_tokens() {
        let usage = BenchTokenUsage {
            prompt_tokens: None,
            completion_tokens: Some(10),
            total_tokens: Some(10),
            error: None,
        };
        let pricing = BenchPricing {
            input_cost_per_million: 1.0,
            output_cost_per_million: 1.0,
        };

        let cost = estimate_cost(&usage, Some(pricing));

        assert_eq!(cost.error.as_deref(), Some("missing_prompt_tokens"));
    }

    #[test]
    fn estimate_cost_requires_completion_tokens() {
        let usage = BenchTokenUsage {
            prompt_tokens: Some(10),
            completion_tokens: None,
            total_tokens: Some(10),
            error: None,
        };
        let pricing = BenchPricing {
            input_cost_per_million: 1.0,
            output_cost_per_million: 1.0,
        };

        let cost = estimate_cost(&usage, Some(pricing));

        assert_eq!(cost.error.as_deref(), Some("missing_completion_tokens"));
    }

    #[test]
    fn estimate_cost_handles_zero_tokens() {
        let usage = BenchTokenUsage {
            prompt_tokens: Some(0),
            completion_tokens: Some(0),
            total_tokens: Some(0),
            error: None,
        };
        let pricing = BenchPricing {
            input_cost_per_million: 2.5,
            output_cost_per_million: 7.5,
        };

        let cost = estimate_cost(&usage, Some(pricing));

        assert!(cost.error.is_none());
        assert_eq!(cost.input_cost_usd, Some(0.0));
        assert_eq!(cost.output_cost_usd, Some(0.0));
        assert_eq!(cost.total_cost_usd, Some(0.0));
    }

    #[test]
    fn aggregate_usage_from_steps_flags_missing_usage_for_openai() {
        let steps = vec![step_with_usage(None)];

        let usage = aggregate_usage_from_steps(&steps, &LlmMode::OpenAi);

        assert_eq!(usage.error.as_deref(), Some("missing_usage_for_1/1 calls"));
        assert!(usage.total_tokens.is_none());
    }

    #[test]
    fn aggregate_usage_from_steps_flags_missing_usage_for_scripted() {
        let steps = vec![step_with_usage(None)];

        let usage = aggregate_usage_from_steps(&steps, &LlmMode::Scripted);

        assert_eq!(
            usage.error.as_deref(),
            Some("usage_unavailable_for_scripted_mode (missing 1/1 calls)")
        );
    }

    #[test]
    fn aggregate_usage_from_steps_sums_tokens() {
        let steps = vec![
            step_with_usage(Some(TokenUsage {
                prompt_tokens: Some(10),
                completion_tokens: Some(5),
                total_tokens: Some(15),
            })),
            step_with_usage(Some(TokenUsage {
                prompt_tokens: Some(7),
                completion_tokens: Some(3),
                total_tokens: Some(10),
            })),
        ];

        let usage = aggregate_usage_from_steps(&steps, &LlmMode::OpenAi);

        assert!(usage.error.is_none());
        assert_eq!(usage.prompt_tokens, Some(17));
        assert_eq!(usage.completion_tokens, Some(8));
        assert_eq!(usage.total_tokens, Some(25));
    }
}
