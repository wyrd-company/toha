// ---
// relationships:
//   implements: project-updates
//   references: command-line-interface
// ---
//! Completion output for person and staged-agent project updates.
use crate::Outcome;
use toha::snapshot::UpdateReport;

pub(crate) fn text_outcome(outcome: Outcome, report: &UpdateReport, dry_run: bool) -> Outcome {
    let Outcome::Document(document, 0) = outcome else {
        return outcome;
    };
    let mut lines = vec![
        format!(
            "Template: {}",
            document["context"]["template"].as_str().unwrap_or("")
        ),
        format!(
            "Target: {}",
            document["context"]["target"].as_str().unwrap_or("")
        ),
        String::new(),
    ];
    let status = document["status"].as_str().unwrap_or("");
    if status == "already-current" {
        lines.push("Already current.".into());
    }
    if let Some(changes) = document["merge"]["changes"].as_array() {
        for change in changes {
            let action = match change["action"].as_str().unwrap_or("") {
                "added" => "Added",
                "updated" => "Updated",
                "merged" => "Merged",
                "deleted" => "Deleted",
                "conflicted" => "Conflicted",
                "not-previewed" => "Not previewed",
                _ => "Changed",
            };
            let mut line = format!("{action} {}", change["path"].as_str().unwrap_or(""));
            if let Some(kind) = change["conflict"].as_str() {
                line.push_str(&format!(" ({kind})"));
            }
            lines.push(line);
        }
    }
    lines.extend(report.messages.iter().cloned());
    for (index, hook) in report.hooks.iter().enumerate() {
        let name = hook
            .id
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("{}", index + 1));
        if !hook.success {
            let code = hook
                .exit_code
                .map_or_else(|| "terminated".into(), |code| format!("exit {code}"));
            lines.push(format!("Hook {name} failed ({code}); failure was allowed."));
        }
    }
    if status == "planned" {
        lines.push("Preview only; no changes written.".into());
        if document["trusted"] == false {
            lines.push("Hooks need trust; rerun with --trust after reviewing them.".into());
        }
        lines.push("No snapshot saved: preview.".into());
    } else if status == "already-current" {
        lines.push("No snapshot saved: already current.".into());
        if dry_run {
            lines.push("Preview only; no changes written.".into());
        }
    } else if let Some(id) = document["snapshot"]["id"].as_str() {
        lines.push(format!("Saved snapshot {id}"));
    }
    if document["merge"]["conflicted"]
        .as_array()
        .is_some_and(|paths| !paths.is_empty())
    {
        lines.push("Resolve conflicts in these files before committing.".into());
    }
    Outcome::Written(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn lines(document: serde_json::Value, dry_run: bool) -> Vec<String> {
        match text_outcome(
            Outcome::Document(document, 0),
            &UpdateReport::default(),
            dry_run,
        ) {
            Outcome::Written(lines) => lines,
            _ => panic!("expected prose"),
        }
    }
    #[test]
    fn update_summary_lists_actions_conflicts_and_snapshot() {
        let text = lines(
            json!({"context":{"template":"sample-template","target":"/sample"},
            "status":"applied", "snapshot":{"id":"sample-id"}, "merge":{
            "changes":[{"action":"added","path":"a.txt"},{"action":"updated","path":"b.txt"},
            {"action":"merged","path":"c.txt"},{"action":"deleted","path":"d.txt"},
            {"action":"conflicted","path":"e.txt","conflict":"content"},
            {"action":"not-previewed","path":"f.txt"}],"conflicted":["e.txt"]}}),
            false,
        );
        assert_eq!(
            text,
            vec![
                "Template: sample-template",
                "Target: /sample",
                "",
                "Added a.txt",
                "Updated b.txt",
                "Merged c.txt",
                "Deleted d.txt",
                "Conflicted e.txt (content)",
                "Not previewed f.txt",
                "Saved snapshot sample-id",
                "Resolve conflicts in these files before committing."
            ]
        );
    }
    #[test]
    fn previews_and_current_results_do_not_claim_a_saved_snapshot() {
        let planned = lines(
            json!({"context":{},"status":"planned","trusted":false}),
            true,
        );
        assert!(
            planned
                .iter()
                .any(|s| s == "Preview only; no changes written.")
        );
        assert!(
            planned
                .iter()
                .any(|s| s == "Hooks need trust; rerun with --trust after reviewing them.")
        );
        assert!(planned.iter().any(|s| s == "No snapshot saved: preview."));
        let current = lines(
            json!({"context":{},"status":"already-current","snapshot":{"id":"base-id"}}),
            true,
        );
        assert!(current.iter().any(|s| s == "Already current."));
        assert!(
            current
                .iter()
                .any(|s| s == "No snapshot saved: already current.")
        );
        assert!(!current.iter().any(|s| s.starts_with("Saved snapshot ")));
        assert!(
            current
                .iter()
                .any(|s| s == "Preview only; no changes written.")
        );
    }
}
