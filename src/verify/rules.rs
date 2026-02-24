use crate::limits::exceeds_symmetric_limit_i64;
use crate::types::{Action, Observation};
use reqwest::Url;
use serde::Serialize;
use std::collections::HashSet;
use std::time::Duration;

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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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
        let element_ids: HashSet<&str> = observation
            .elements
            .iter()
            .map(|el| el.id.as_str())
            .collect();
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
                            length, self.config.max_text_len
                        ),
                    ));
                }
            }
            Action::Select { id, .. } => {
                validate_element_id(id, &element_ids, &mut errors);
            }
            Action::Scroll { dx, dy } => {
                let limit = self.config.max_scroll;
                if exceeds_symmetric_limit_i64(*dx, limit)
                    || exceeds_symmetric_limit_i64(*dy, limit)
                {
                    errors.push(ValidationError::new(
                        "scroll_out_of_bounds",
                        Some("scroll"),
                        format!("scroll out of bounds: dx={dx}, dy={dy}, max={limit}"),
                    ));
                }
            }
            Action::Wait { ms } => {
                if wait_exceeds_max(*ms, self.config.max_wait_ms) {
                    errors.push(ValidationError::new(
                        "wait_too_long",
                        Some("ms"),
                        format!("wait {ms}ms exceeds max {}ms", self.config.max_wait_ms),
                    ));
                }
            }
            Action::Navigate { url } => match parse_navigate_url(url) {
                Ok(parsed) => {
                    if let Some(error) = evaluate_url_policy(&parsed, self.config.allow_insecure) {
                        errors.push(error);
                    }
                }
                Err(error) => errors.push(error),
            },
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

fn wait_exceeds_max(ms: u64, max_wait_ms: u64) -> bool {
    Duration::from_millis(ms) > Duration::from_millis(max_wait_ms)
}

fn parse_navigate_url(raw: &str) -> Result<Url, ValidationError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ValidationError::new(
            "missing_url",
            Some("url"),
            "navigate url is required",
        ));
    }
    Url::parse(trimmed).map_err(|err| {
        ValidationError::new(
            "invalid_url",
            Some("url"),
            format!("invalid url {trimmed}: {err}"),
        )
    })
}

fn evaluate_url_policy(url: &Url, allow_insecure: bool) -> Option<ValidationError> {
    if allow_insecure {
        return None;
    }

    match url.scheme() {
        "http" | "https" => None,
        other => Some(ValidationError::new(
            "insecure_url",
            Some("url"),
            format!("unsupported url scheme '{other}'"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ElementFlags, ElementRef, Observation};

    fn sample_observation() -> Observation {
        Observation {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            viewport: [1280, 800],
            focused: None,
            visible_text: "Hello".to_string(),
            screenshot: None,
            state_hash: "hash".to_string(),
            elements: vec![ElementRef {
                id: "el_1".to_string(),
                role: "button".to_string(),
                name: Some("Submit".to_string()),
                value: None,
                bbox: [0.0, 0.0, 10.0, 10.0],
                flags: ElementFlags::default(),
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
    fn validates_type_with_known_id() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Type {
            id: "el_1".to_string(),
            text: "hello".to_string(),
            submit: Some(false),
        };
        assert!(validator.validate(&action, &obs).is_ok());
    }

    #[test]
    fn validates_text_at_max_length() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Type {
            id: "el_1".to_string(),
            text: "a".repeat(validator.config.max_text_len),
            submit: None,
        };
        assert!(validator.validate(&action, &obs).is_ok());
    }

    #[test]
    fn rejects_text_above_max_length() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Type {
            id: "el_1".to_string(),
            text: "a".repeat(validator.config.max_text_len + 1),
            submit: None,
        };
        let errors = validator
            .validate(&action, &obs)
            .expect_err("expected errors");
        assert_eq!(errors[0].code, "text_too_long");
    }

    #[test]
    fn rejects_missing_id() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Click { id: "".to_string() };
        let errors = validator
            .validate(&action, &obs)
            .expect_err("expected errors");
        assert_eq!(errors[0].code, "missing_id");
    }

    #[test]
    fn rejects_unknown_id() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Click {
            id: "el_9".to_string(),
        };
        let errors = validator
            .validate(&action, &obs)
            .expect_err("expected errors");
        assert_eq!(errors[0].code, "unknown_id");
    }

    #[test]
    fn validates_select_with_known_id() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Select {
            id: "el_1".to_string(),
            value: "choice".to_string(),
        };
        assert!(validator.validate(&action, &obs).is_ok());
    }

    #[test]
    fn rejects_select_with_unknown_id() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Select {
            id: "el_9".to_string(),
            value: "choice".to_string(),
        };
        let errors = validator
            .validate(&action, &obs)
            .expect_err("expected errors");
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
        let errors = validator
            .validate(&action, &obs)
            .expect_err("expected errors");
        let codes: Vec<&str> = errors.iter().map(|error| error.code.as_str()).collect();
        assert_eq!(codes, vec!["missing_id", "text_too_long"]);
    }

    #[test]
    fn rejects_scroll_out_of_bounds() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Scroll { dx: 0, dy: 2501 };
        let errors = validator
            .validate(&action, &obs)
            .expect_err("expected errors");
        assert_eq!(errors[0].code, "scroll_out_of_bounds");
    }

    #[test]
    fn validates_scroll_within_bounds() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Scroll {
            dx: -2000,
            dy: 2000,
        };
        assert!(validator.validate(&action, &obs).is_ok());
    }

    #[test]
    fn validates_scroll_with_configured_bounds() {
        let validator = Validator::new(ValidatorConfig {
            max_scroll: 500,
            ..ValidatorConfig::default()
        });
        let obs = sample_observation();
        let action = Action::Scroll { dx: -500, dy: 500 };
        assert!(validator.validate(&action, &obs).is_ok());
    }

    #[test]
    fn rejects_scroll_above_configured_bounds() {
        let validator = Validator::new(ValidatorConfig {
            max_scroll: 500,
            ..ValidatorConfig::default()
        });
        let obs = sample_observation();
        let action = Action::Scroll { dx: -501, dy: 0 };
        let errors = validator
            .validate(&action, &obs)
            .expect_err("expected errors");
        assert_eq!(errors[0].code, "scroll_out_of_bounds");
    }

    #[test]
    fn rejects_wait_too_long() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Wait { ms: 40_000 };
        let errors = validator
            .validate(&action, &obs)
            .expect_err("expected errors");
        assert_eq!(errors[0].code, "wait_too_long");
    }

    #[test]
    fn validates_wait_within_bounds() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Wait { ms: 500 };
        assert!(validator.validate(&action, &obs).is_ok());
    }

    #[test]
    fn validates_wait_at_configured_max() {
        let validator = Validator::new(ValidatorConfig {
            max_wait_ms: 750,
            ..ValidatorConfig::default()
        });
        let obs = sample_observation();
        let action = Action::Wait { ms: 750 };
        assert!(validator.validate(&action, &obs).is_ok());
    }

    #[test]
    fn rejects_wait_above_configured_max() {
        let validator = Validator::new(ValidatorConfig {
            max_wait_ms: 750,
            ..ValidatorConfig::default()
        });
        let obs = sample_observation();
        let action = Action::Wait { ms: 751 };
        let errors = validator
            .validate(&action, &obs)
            .expect_err("expected errors");
        assert_eq!(errors[0].code, "wait_too_long");
    }

    #[test]
    fn rejects_missing_url() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Navigate {
            url: "  ".to_string(),
        };
        let errors = validator
            .validate(&action, &obs)
            .expect_err("expected errors");
        assert_eq!(errors[0].code, "missing_url");
    }

    #[test]
    fn rejects_insecure_url_by_default() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Navigate {
            url: "file:///tmp/test.html".to_string(),
        };
        let errors = validator
            .validate(&action, &obs)
            .expect_err("expected errors");
        assert_eq!(errors[0].code, "insecure_url");
    }

    #[test]
    fn rejects_invalid_url() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Navigate {
            url: "http://exa mple.com".to_string(),
        };
        let errors = validator
            .validate(&action, &obs)
            .expect_err("expected errors");
        assert_eq!(errors[0].code, "invalid_url");
    }

    #[test]
    fn validates_https_url() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Navigate {
            url: "https://example.com".to_string(),
        };
        assert!(validator.validate(&action, &obs).is_ok());
    }

    #[test]
    fn validates_http_url() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Navigate {
            url: "http://example.com".to_string(),
        };
        assert!(validator.validate(&action, &obs).is_ok());
    }

    #[test]
    fn validates_uppercase_scheme_url() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Navigate {
            url: "HTTPS://example.com".to_string(),
        };
        assert!(validator.validate(&action, &obs).is_ok());
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

    #[test]
    fn validates_extract_without_id() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Extract {
            query: "price".to_string(),
            id: None,
        };
        assert!(validator.validate(&action, &obs).is_ok());
    }

    #[test]
    fn rejects_extract_unknown_id() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Extract {
            query: "price".to_string(),
            id: Some("el_9".to_string()),
        };
        let errors = validator
            .validate(&action, &obs)
            .expect_err("expected errors");
        assert_eq!(errors[0].code, "unknown_id");
    }

    #[test]
    fn validates_back_action() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Back;
        assert!(validator.validate(&action, &obs).is_ok());
    }

    #[test]
    fn validates_done_action() {
        let validator = Validator::default();
        let obs = sample_observation();
        let action = Action::Done {
            summary: "Finished".to_string(),
        };
        assert!(validator.validate(&action, &obs).is_ok());
    }
}
