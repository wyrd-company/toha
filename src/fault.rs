use std::fmt;

/// A fault in one authored template field.
#[derive(Debug)]
pub(crate) struct TemplateFault {
    pub field: String,
    pub expression: Option<String>,
    pub message: String,
}

impl fmt::Display for TemplateFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "template error in {}", self.field)?;
        if let Some(expression) = &self.expression {
            write!(f, " `{expression}`")?;
        }
        write!(f, ": {}", self.message)
    }
}
