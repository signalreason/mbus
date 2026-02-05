use crate::types::Action;
use jsonschema::{Draft, JSONSchema};
use schemars::schema_for;
use serde_json::Value;

pub struct ActionSchema {
    schema: Value,
    validator: JSONSchema,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaViolation {
    pub instance_path: String,
    pub schema_path: String,
    pub message: String,
}

impl SchemaViolation {
    fn from_error(error: jsonschema::ValidationError) -> Self {
        Self {
            instance_path: error.instance_path.to_string(),
            schema_path: error.schema_path.to_string(),
            message: error.to_string(),
        }
    }
}

impl ActionSchema {
    pub fn new() -> Self {
        let root = schema_for!(Action);
        let schema = serde_json::to_value(&root).expect("action schema to be serializable");
        let validator = JSONSchema::options()
            .with_draft(Draft::Draft7)
            .compile(&schema)
            .expect("action schema to be valid");
        Self { schema, validator }
    }

    pub fn json(&self) -> &Value {
        &self.schema
    }

    pub fn validate_json(&self, value: &Value) -> Result<(), Vec<SchemaViolation>> {
        match self.validator.validate(value) {
            Ok(()) => Ok(()),
            Err(errors) => Err(errors.map(SchemaViolation::from_error).collect()),
        }
    }
}

impl Default for ActionSchema {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn schema_accepts_all_action_variants() {
        let schema = ActionSchema::new();
        let samples = vec![
            json!({"type": "click", "id": "el_1"}),
            json!({"type": "type", "id": "el_2", "text": "hello"}),
            json!({"type": "type", "id": "el_2", "text": "hello", "submit": true}),
            json!({"type": "select", "id": "el_3", "value": "option"}),
            json!({"type": "scroll", "dx": 10, "dy": -20}),
            json!({"type": "wait", "ms": 500}),
            json!({"type": "navigate", "url": "https://example.com"}),
            json!({"type": "back"}),
            json!({"type": "extract", "query": "price"}),
            json!({"type": "extract", "query": "price", "id": "el_4"}),
            json!({"type": "done", "summary": "Finished"}),
        ];
        for sample in samples {
            assert!(
                schema.validate_json(&sample).is_ok(),
                "expected valid action, got error: {sample:?}"
            );
        }
    }

    #[test]
    fn schema_rejects_invalid_actions() {
        let schema = ActionSchema::new();
        let samples = vec![
            json!({"type": "click"}),
            json!({"type": "click", "id": "el_1", "extra": 1}),
            json!({"type": "type", "id": "el_2"}),
            json!({"type": "select", "id": "el_3"}),
            json!({"type": "scroll", "dx": 0}),
            json!({"type": "wait"}),
            json!({"type": "wait", "ms": "fast"}),
            json!({"type": "navigate"}),
            json!({"type": "extract"}),
            json!({"type": "done"}),
            json!({"type": "back", "extra": true}),
            json!({"type": "unknown"}),
        ];
        for sample in samples {
            assert!(
                schema.validate_json(&sample).is_err(),
                "expected invalid action, got ok: {sample:?}"
            );
        }
    }
}
