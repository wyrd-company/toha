// ---
// relationships:
//   implements: architecture
// ---
//! The snapshot commands: `snapshots list`, `snapshots clean`, and `init`.
//!
//! Each opens the target's repository through `toha::snapshot::Project` (which
//! strips drivers in memory) and reports through the shared `Outcome`. The
//! non-interactive routes — `list`, `clean --remove`, and `init` — are here;
//! they never run a driver program and never push.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use toha::snapshot::{LikelyBaseBy, Listed, Project, Revision, SnapshotId};
use toha::staging::canonical_target;

/// The outcome kinds this module produces, mapped by the caller to the binary's
/// `Outcome`.
pub enum SnapshotOutput {
    Lines(Vec<String>),
    Document(Value),
    Error(String),
}

/// `toha init [PATH] [--remote NAME]`.
pub fn init(path: Option<PathBuf>, remote: &str) -> SnapshotOutput {
    let project = match open(path.as_deref()) {
        Ok(Some(project)) => project,
        Ok(None) => return SnapshotOutput::Error(not_in_git()),
        Err(message) => return SnapshotOutput::Error(message),
    };
    match project.add_fetch(remote) {
        Ok(setting) if setting.added => SnapshotOutput::Lines(vec![format!(
            "added fetch {} to remote {}",
            setting.refspec, setting.remote
        )]),
        Ok(setting) => SnapshotOutput::Lines(vec![format!(
            "remote {} already fetches snapshots",
            setting.remote
        )]),
        Err(err) => SnapshotOutput::Error(err.to_string()),
    }
}

/// `toha snapshots list [PATH] [--json]`.
pub fn list(path: Option<PathBuf>, as_json: bool) -> SnapshotOutput {
    let project = match open(path.as_deref()) {
        Ok(Some(project)) => project,
        Ok(None) => return SnapshotOutput::Error(not_in_git()),
        Err(message) => return SnapshotOutput::Error(message),
    };
    let listed = match project.snapshots() {
        Ok(listed) => listed,
        Err(err) => return SnapshotOutput::Error(err.to_string()),
    };
    let valid: Vec<_> = listed
        .iter()
        .filter_map(|entry| match entry {
            Listed::Valid(snapshot) => Some(snapshot.clone()),
            Listed::Invalid { .. } => None,
        })
        .collect();
    let marks = project.likely_bases(&valid).unwrap_or_default();
    let mark_for = |id: &str| {
        marks
            .iter()
            .find(|m| m.id.to_string() == id)
            .map(|m| match m.by {
                LikelyBaseBy::Ancestry => "likely base",
                LikelyBaseBy::Content => "likely base (by content)",
            })
    };

    if as_json {
        let entries: Vec<Value> = listed
            .iter()
            .map(|entry| match entry {
                Listed::Valid(s) => {
                    let id = s.id().to_string();
                    json!({
                        "id": id,
                        "template": s.template(),
                        "source": s.source(),
                        "commit": revision_commit(s.revision()),
                        "target": s.target().as_str(),
                        "created": s.created().to_string(),
                        "project": { "commit": s.project().commit().as_str(), "branch": s.project().branch() },
                        "built_from": s.built_from().map(|b| b.to_string()),
                        "mark": mark_for(&id),
                    })
                }
                Listed::Invalid { r#ref, reason } => json!({ "ref": r#ref, "invalid": reason }),
            })
            .collect();
        SnapshotOutput::Document(json!({ "snapshots": entries }))
    } else {
        let mut lines = Vec::new();
        for entry in &listed {
            match entry {
                Listed::Valid(s) => {
                    let id = s.id().to_string();
                    let mark = mark_for(&id)
                        .map(|m| format!("  [{m}]"))
                        .unwrap_or_default();
                    lines.push(format!("{id}  {}  {}{mark}", s.template(), s.created()));
                }
                Listed::Invalid { r#ref, reason } => {
                    lines.push(format!("{ref}  invalid: {reason}"));
                }
            }
        }
        if lines.is_empty() {
            lines.push("no snapshots".to_owned());
        }
        SnapshotOutput::Lines(lines)
    }
}

/// `toha snapshots clean [PATH] --remove FILE [--force]` (the script/agent route).
pub fn clean_remove(path: Option<PathBuf>, remove: &str, force: bool) -> SnapshotOutput {
    let project = match open(path.as_deref()) {
        Ok(Some(project)) => project,
        Ok(None) => return SnapshotOutput::Error(not_in_git()),
        Err(message) => return SnapshotOutput::Error(message),
    };
    let text = match read_input(remove) {
        Ok(text) => text,
        Err(message) => return SnapshotOutput::Error(message),
    };
    let request: RemoveRequest = match serde_json::from_str(&text) {
        Ok(request) => request,
        Err(err) => return SnapshotOutput::Error(format!("invalid remove document: {err}")),
    };
    let ids = match request
        .remove
        .iter()
        .map(|raw| raw.parse::<SnapshotId>())
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(ids) => ids,
        Err(err) => return SnapshotOutput::Error(err.to_string()),
    };
    match project.remove(&ids, force) {
        Ok(removed) => {
            let deletes: Vec<String> = removed
                .removed
                .iter()
                .map(|id| format!("git push origin --delete refs/toha/snapshots/{id}"))
                .collect();
            SnapshotOutput::Document(json!({
                "removed": removed.removed.iter().map(|i| i.to_string()).collect::<Vec<_>>(),
                "not_found": removed.not_found.iter().map(|i| i.to_string()).collect::<Vec<_>>(),
                "push_commands": deletes,
            }))
        }
        Err(err) => SnapshotOutput::Error(err.to_string()),
    }
}

#[derive(serde::Deserialize)]
struct RemoveRequest {
    remove: Vec<String>,
}

fn open(path: Option<&Path>) -> Result<Option<Project>, String> {
    let cwd;
    let path = match path {
        Some(path) => path,
        None => {
            cwd = std::env::current_dir().map_err(|e| e.to_string())?;
            cwd.as_path()
        }
    };
    let target = canonical_target(path).map_err(|e| e.to_string())?;
    Project::open(&target).map_err(|e| e.to_string())
}

fn read_input(source: &str) -> Result<String, String> {
    if source == "-" {
        use std::io::Read;
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        Ok(text)
    } else {
        std::fs::read_to_string(source).map_err(|e| format!("{source}: {e}"))
    }
}

fn not_in_git() -> String {
    "the target is not inside a git repository; snapshots need git".to_owned()
}

/// The 40-hex commit of a revision, or `None` for a folder template.
fn revision_commit(revision: &Revision) -> Option<&str> {
    match revision {
        Revision::Commit(commit) => Some(commit.as_str()),
        Revision::Unversioned => None,
    }
}
