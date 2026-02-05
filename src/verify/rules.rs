use crate::types::{Action, Observation};
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub struct Validator {
    config: ValidatorConfig,
}

#[derive(Clone, Debug)]
pub struct ValidatorConfig {
    pub allow_insecure: bool,
    pub max_text_len: usize,
    pub max_wait_ms: u64,
    pub max_scroll: i64,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            allow_insecure: false,
            max_text_len: 2000,
            max_wait_ms: 30_000,
            max_scroll: 2000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    pub code: String,
    pub field: Option<String>,
    pub message: String,
}

impl ValidationError {
    fn new(code: &'static str, field: Option<&'static str>, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            field: field.map(|value| value.to_string()),
            message: message.into(),
        }
    }
}

impl Default for Validator {
    fn default() -> Self {
        Self::new(ValidatorConfig::default())
    }
}

impl Validator {
    pub fn new(config: ValidatorConfig) -> Self {
        Self { config }
    }

    pub fn validate(
        &self,
        action: &Action,
        observation: &Observation,
    ) -> Result<(), Vec<ValidationError>> {
        let element_ids: HashSet<&str> =
            observation.elements.iter().map(|el| el.id.as_str()).collect();
        let mut errors = Vec::new();

        match action {
            Action::Click { id } => {
                validate_element_id(id, &element_ids, &mut errors);
            }
            Action::Type { id, text, .. } => {
                validate_element_id(id, &element_ids, &mut errors);
                let length = text.chars().count();
                if length > self.config.max_text_len {
                    errors.push(ValidationError::new(
                        "text_too_long",
                        Some("text"),
                        format!(
                            "text length {} exceeds max {}",
                            length,
                            self.config.max_text_len
                        ),
                    ));
                }
            }
            Action::Select { id, .. } => {
                validate_element_id(id, &element_ids, &mut errors);
            }
            Action::Scroll { dx, dy } => {
                let limit = self.config.max_scroll;
                if dx.saturating_abs() > limit || dy.saturating_abs() > limit {
                    errors.push(ValidationError::new(
                        "scroll_out_of_bounds",
                        Some("scroll"),
                        format!(
                            "scroll out of bounds: dx={dx}, dy={dy}, max={limit}"
                        ),
                    ));
                }
            }
            Action::Wait { ms } => {
                if *ms > self.config.max_wait_ms {
                    errors.push(ValidationError::new(
                        "wait_too_long",
                        Some("ms"),
                        format!(
                            "wait {ms}ms exceeds max {}ms",
                            self.config.max_wait_ms
                        ),
                    ));
                }
            }
            Action::Navigate { url } => {
                if url.trim().is_empty() {
                    errors.push(ValidationError::new(
                        "missing_url",
                        Some("url"),
                        "navigate url is required",
                    ));
                } else if !self.config.allow_insecure
                    && !(url.starts_with("http://") || url.starts_with("https://"))
                {
                    errors.push(ValidationError::new(
                        "insecure_url",
                        Some("url"),
                        format!("unsupported url scheme for {url}"),
                    ));
                }
            }
            Action::Back => {}
            Action::Extract { id, .. } => {
                if let Some(id) = id.as_ref() {
                    validate_element_id(id, &element_ids, &mut errors);
                }
            }
            Action::Done { .. } => {}
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn validate_element_id(id: &str, element_ids: &HashSet<&str>, errors: &mut Vec<ValidationError>) {
    if id.trim().is_empty() {
        errors.push(ValidationError::new(
            "missing_id",
            Some("id"),
            "action id is required",
        ));
        return;
    }

    if !element_ids.contains(id) {
        errors.push(ValidationError::new(
            "unknown_id",
            Some("id"),
            format!("id {id} not found in observation"),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ElementRef, Observation};

    fn sample_observation() -> Observation {
        Observation {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            viewport: [1280, 800],
            focused: None,
            visible_text: "Hello".to_string(),
            state_hash: Some("hash".to_string()),
            elements: vec![ElementRef {
                id: "el_1".to_string(),
                role: "button".to_string(),
                name: Some("Submit".to_string()),
                value: None,
                bbox: [0.0, 0.0, 10.0, 10.0],
                flags: vec![],
            }],
        }
    }

    #[test]
    fn validates_click_with_known_id() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Click {
            id: "el_1".to_string(),
        };
        assert!(validator.validate(&action, &obs).is_ok());
    }

    #[test]
    fn rejects_missing_id() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Click { id: "".to_string() };
        let errors = validator.validate(&action, &obs).expect_err("expected errors");
        assert_eq!(errors[0].code, "missing_id");
    }

    #[test]
    fn rejects_unknown_id() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Click {
            id: "el_9".to_string(),
        };
        let errors = validator.validate(&action, &obs).expect_err("expected errors");
        assert_eq!(errors[0].code, "unknown_id");
    }

    #[test]
    fn rejects_long_text_and_missing_id_in_order() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Type {
            id: "".to_string(),
            text: "a".repeat(2001),
            submit: None,
        };
        let errors = validator.validate(&action, &obs).expect_err("expected errors");
        let codes: Vec<&str> = errors.iter().map(|error| error.code.as_str()).collect();
        assert_eq!(codes, vec!["missing_id", "text_too_long"]);
    }

    #[test]
    fn rejects_scroll_out_of_bounds() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Scroll { dx: 0, dy: 2501 };
        let errors = validator.validate(&action, &obs).expect_err("expected errors");
        assert_eq!(errors[0].code, "scroll_out_of_bounds");
    }

    #[test]
    fn rejects_wait_too_long() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Wait { ms: 40_000 };
        let errors = validator.validate(&action, &obs).expect_err("expected errors");
        assert_eq!(errors[0].code, "wait_too_long");
    }

    #[test]
    fn rejects_insecure_url_by_default() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Navigate {
            url: "file:///tmp/test.html".to_string(),
        };
        let errors = validator.validate(&action, &obs).expect_err("expected errors");
        assert_eq!(errors[0].code, "insecure_url");
    }

    #[test]
    fn allows_insecure_url_when_configured() {
        let validator = Validator::new(ValidatorConfig {
            allow_insecure: true,
            ..ValidatorConfig::default()
        });
        let obs = sample_observation();
        let action = Action::Navigate {
            url: "file:///tmp/test.html".to_string(),
        };
        assert!(validator.validate(&action, &obs).is_ok());
    }
}
