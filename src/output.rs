use crate::agent::memory::StepRecord;
use crate::types::Action;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExtractOutput {
    pub task_id: String,
    pub timestamp: String,
    pub extracts: Vec<ExtractRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExtractRecord {
    pub step_index: usize,
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub value: String,
}

pub fn task_id_for(task: &str) -> String {
    let mut hasher = DefaultHasher::new();
    task.hash(&mut hasher);
    format!("task_{:016x}", hasher.finish())
}

pub fn current_timestamp() -> Result<String, time::error::Format> {
    use time::format_description::well_known::Rfc3339;
    Ok(time::OffsetDateTime::now_utc().format(&Rfc3339)?)
}

pub fn build_extract_output(
    task_id: impl Into<String>,
    timestamp: impl Into<String>,
    steps: &[StepRecord],
) -> Option<ExtractOutput> {
    let extracts: Vec<ExtractRecord> = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| {
            if !step.result.ok {
                return None;
            }
            let extract = step.result.extract.as_ref()?;
            if !matches!(step.action, Action::Extract { .. }) {
                return None;
            }
            Some(ExtractRecord {
                step_index: index + 1,
                query: extract.query.clone(),
                id: extract.id.clone(),
                value: extract.value.clone(),
            })
        })
        .collect();

    if extracts.is_empty() {
        None
    } else {
        Some(ExtractOutput {
            task_id: task_id.into(),
            timestamp: timestamp.into(),
            extracts,
        })
    }
}

pub fn write_extract_output(path: &Path, output: &ExtractOutput) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let data = serde_json::to_vec_pretty(output)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    std::fs::write(path, data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Action, ExtractResult, StepResult};

    #[test]
    fn task_id_is_deterministic() {
        let first = task_id_for("find price");
        let second = task_id_for("find price");
        assert_eq!(first, second);
    }

    #[test]
    fn build_extract_output_skips_non_extract_steps() {
        let steps = vec![StepRecord {
            action: Action::Click {
                id: "el_1".to_string(),
            },
            result: StepResult {
                ok: true,
                error: None,
                new_state_hash: None,
                extract: None,
            },
        }];

        let output = build_extract_output("task_1", "now", &steps);
        assert!(output.is_none());
    }

    #[test]
    fn build_extract_output_collects_extracts() {
        let steps = vec![StepRecord {
            action: Action::Extract {
                query: "price".to_string(),
                id: Some("el_9".to_string()),
            },
            result: StepResult {
                ok: true,
                error: None,
                new_state_hash: None,
                extract: Some(ExtractResult {
                    query: "price".to_string(),
                    id: Some("el_9".to_string()),
                    value: "$10".to_string(),
                }),
            },
        }];

        let output = build_extract_output("task_1", "now", &steps).expect("output");
        assert_eq!(output.task_id, "task_1");
        assert_eq!(output.timestamp, "now");
        assert_eq!(output.extracts.len(), 1);
        assert_eq!(output.extracts[0].step_index, 1);
        assert_eq!(output.extracts[0].query, "price");
        assert_eq!(output.extracts[0].id, Some("el_9".to_string()));
        assert_eq!(output.extracts[0].value, "$10");
    }
}
