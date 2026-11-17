// ---
// relationships:
//   implements: architecture
// ---
//! Messages for a template or configuration document that does not match its
//! schema, written by Toha.
//!
//! A message names the place by its path and says which rule the value
//! breaks, in words that come from Toha and the schema. It never writes the
//! value it rejects, nor the schema library's own message, which does: a
//! template or configuration value can hold the credentials of a URL in any
//! form and at any depth.
use jsonschema::{
    ValidationError, error::TypeKind, error::ValidationErrorKind, paths::LocationSegment,
};

/// The path of the value `error` rejects, as a JSON pointer whose keys lose
/// their user information: a key can be a template address.
pub(crate) fn path(error: &ValidationError) -> String {
    error
        .instance_path()
        .iter()
        .map(|segment| match segment {
            LocationSegment::Property(key) => format!(
                "/{}",
                crate::source::without_userinfo(&key)
                    .replace('~', "~0")
                    .replace('/', "~1")
            ),
            LocationSegment::Index(index) => format!("/{index}"),
        })
        .collect()
}

/// Which rule the value `error` rejects breaks, without the value.
pub(crate) fn message(error: &ValidationError) -> String {
    match error.kind() {
        ValidationErrorKind::Required { property } => {
            format!("is missing the required property {property}")
        }
        ValidationErrorKind::AdditionalProperties { unexpected }
        | ValidationErrorKind::UnevaluatedProperties { unexpected } => {
            let names: Vec<String> = unexpected
                .iter()
                .map(|name| crate::source::without_userinfo(name))
                .collect();
            format!(
                "has properties the schema does not allow: {}",
                names.join(", ")
            )
        }
        ValidationErrorKind::Type { kind } => match kind {
            TypeKind::Single(kind) => format!("must be of type {kind}"),
            TypeKind::Multiple(kinds) => {
                let kinds: Vec<String> = kinds.iter().map(|kind| kind.to_string()).collect();
                format!("must be of type {}", kinds.join(" or "))
            }
        },
        ValidationErrorKind::Enum { options } => format!("must be one of {options}"),
        ValidationErrorKind::Constant { expected_value } => {
            format!("must be {expected_value}")
        }
        ValidationErrorKind::Pattern { pattern } => {
            format!("does not match the pattern {pattern}")
        }
        ValidationErrorKind::PropertyNames { .. } => {
            "has a property name the schema does not allow".into()
        }
        ValidationErrorKind::OneOfNotValid { .. } | ValidationErrorKind::AnyOf { .. } => {
            "matches none of the forms the schema allows".into()
        }
        ValidationErrorKind::OneOfMultipleValid { .. } => {
            "matches more than one of the forms the schema allows".into()
        }
        ValidationErrorKind::FalseSchema | ValidationErrorKind::Not { .. } => {
            "is not allowed here".into()
        }
        ValidationErrorKind::Format { format } => format!("is not a valid {format}"),
        ValidationErrorKind::MinItems { limit } => format!("must have at least {limit} items"),
        ValidationErrorKind::MaxItems { limit } => format!("must have at most {limit} items"),
        ValidationErrorKind::MinLength { limit } => {
            format!("must be at least {limit} characters long")
        }
        ValidationErrorKind::MaxLength { limit } => {
            format!("must be at most {limit} characters long")
        }
        ValidationErrorKind::MinProperties { limit } => {
            format!("must have at least {limit} properties")
        }
        ValidationErrorKind::MaxProperties { limit } => {
            format!("must have at most {limit} properties")
        }
        ValidationErrorKind::Minimum { limit } => format!("must be at least {limit}"),
        ValidationErrorKind::Maximum { limit } => format!("must be at most {limit}"),
        ValidationErrorKind::ExclusiveMinimum { limit } => format!("must be more than {limit}"),
        ValidationErrorKind::ExclusiveMaximum { limit } => format!("must be less than {limit}"),
        ValidationErrorKind::UniqueItems => "must not repeat an item".into(),
        _ => "does not match the schema".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const URL: &str = "https://user:p%40ss-schema-secret@127.0.0.1:9/r.git";

    /// Every error of `instance` against `schema`, as Toha writes it.
    fn written(schema: serde_json::Value, instance: serde_json::Value) -> Vec<String> {
        let validator = jsonschema::validator_for(&schema).unwrap();
        validator
            .iter_errors(&instance)
            .map(|error| format!("{}: {}", path(&error), message(&error)))
            .collect()
    }

    #[test]
    fn a_rejected_value_is_named_by_path_and_rule_without_itself() {
        let schema = json!({
            "type": "object",
            "propertyNames": { "pattern": "^[a-z]+$" },
            "additionalProperties": {
                "oneOf": [
                    { "type": "string", "maxLength": 3 },
                    { "type": "object", "required": ["preset"], "additionalProperties": false,
                      "properties": { "preset": { "type": "string", "pattern": "^[a-z]+$" } } }
                ]
            }
        });
        let table = [
            json!({ "remote": URL }),
            json!({ "remote": [URL] }),
            json!({ "remote": { "preset": [URL] } }),
            json!({ "remote": { "preset": URL } }),
            json!({ "remote": { URL: 1 } }),
            json!({ URL: "x" }),
            json!([URL]),
            json!(URL),
        ];
        for instance in table {
            let written = written(schema.clone(), instance.clone());
            assert!(!written.is_empty(), "{instance}");
            for line in &written {
                assert!(!line.contains("secret"), "{instance}: {line}");
            }
        }
    }

    #[test]
    fn a_path_keeps_its_keys_without_user_information() {
        let schema = json!({ "additionalProperties": { "type": "object" } });
        let written = written(schema, json!({ URL: 1 }));
        assert_eq!(
            written,
            ["/https:~1~1127.0.0.1:9~1r.git: must be of type object"]
        );
    }
}
