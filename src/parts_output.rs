// ---
// relationships:
//   implements: template-composition
// ---
use serde_json::{Value, json};
use toha::applied::Revision;
pub(crate) fn revision(value: Option<&Revision>) -> Value {
    match value {
        Some(Revision::Commit(c)) => json!({"commit": c.as_str()}),
        Some(Revision::Content(d)) => json!({"content_digest": d.as_str()}),
        _ => Value::Null,
    }
}
pub(crate) fn metadata(
    prior: Option<&Revision>,
    resolved: &Revision,
    old: &[String],
    new: &[String],
) -> Value {
    json!({"prior_revision":revision(prior), "resolved_revision":revision(Some(resolved)),
        "parts_added":new.iter().filter(|p| !old.contains(p)).collect::<Vec<_>>(),
        "parts_removed":old.iter().filter(|p| !new.contains(p)).collect::<Vec<_>>()})
}
pub(crate) fn lines(value: &Value) -> Vec<String> {
    let display = |revision: &Value| {
        revision["commit"]
            .as_str()
            .or_else(|| revision["content_digest"].as_str())
            .unwrap_or("unversioned")
            .to_owned()
    };
    let mut lines = vec![
        format!("Prior revision: {}", display(&value["prior_revision"])),
        format!(
            "Resolved revision: {}",
            display(&value["resolved_revision"])
        ),
    ];
    for (key, label) in [
        ("parts_added", "Parts added"),
        ("parts_removed", "Parts removed"),
    ] {
        let parts = value[key]
            .as_array()
            .map(|p| {
                p.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        lines.push(format!(
            "{label}: {}",
            if parts.is_empty() { "none" } else { &parts }
        ));
    }
    lines
}
