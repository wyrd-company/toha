// ---
// relationships:
//   implements: architecture
// ---
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::plan::{Content, Plan, TargetPath, has_symlink_component};

#[derive(Debug, Default, Clone, Copy)]
pub struct ApplyOptions {
    pub force: bool,
}
#[derive(Debug)]
pub enum Applied {
    Written(Vec<TargetPath>),
}
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("conflicting files: {0:?}")]
    Conflicts(Vec<TargetPath>),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("target path contains symlink: {0}")]
    Symlink(TargetPath),
}
impl Plan {
    pub fn apply(self, target: &Path, options: ApplyOptions) -> Result<Applied, ApplyError> {
        for file in &self.files {
            if has_symlink_component(target, &file.path).map_err(|source| ApplyError::Io {
                path: target.join(file.path.as_path()),
                source,
            })? {
                return Err(ApplyError::Symlink(file.path.clone()));
            }
        }
        if !options.force {
            let mut conflicts = self.conflicts;
            for file in &self.files {
                if target.join(file.path.as_path()).exists() && !conflicts.contains(&file.path) {
                    conflicts.push(file.path.clone());
                }
            }
            if !conflicts.is_empty() {
                return Err(ApplyError::Conflicts(conflicts));
            }
        }
        let mut written = Vec::new();
        for file in self.files {
            let path = target.join(file.path.as_path());
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| ApplyError::Io {
                    path: parent.to_owned(),
                    source,
                })?;
            }
            let Content::Rendered(content) = file.content;
            fs::write(&path, content).map_err(|source| ApplyError::Io {
                path: path.clone(),
                source,
            })?;
            written.push(file.path);
        }
        Ok(Applied::Written(written))
    }
}
