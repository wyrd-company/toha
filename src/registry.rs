//! Validated registry layers, discovery, and name resolution.
use crate::review::ReviewDigest;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    System,
    User,
    Local,
    Discovered,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub source: String,
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    pub path: PathBuf,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// The approved executable-surface digest. Approval authorizes the
    /// template's hooks only while its live digest still equals this value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<ReviewDigest>,
    /// A denial recorded by a `trusted: false` field. A denial clears any
    /// approval inherited from a lower registry layer, so the entry reads as
    /// needing review; it is written back as `trusted: false` so the denial
    /// survives a round-trip. It is set only from the legacy `false` value and
    /// carries no serde field of its own. (A future untrust command — design
    /// 1054 — records a denial this way, persisting `trusted: false` while
    /// leaving `approval` absent.)
    #[serde(skip)]
    pub denied: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegistryFile {
    #[serde(default)]
    pub templates: IndexMap<String, Entry>,
    #[serde(skip)]
    presence: IndexMap<String, FieldPresence>,
}
#[derive(Debug, Clone, Copy, Default)]
struct FieldPresence {
    aliases: bool,
    approval: bool,
}
#[derive(Debug, Clone)]
pub struct Listed {
    pub formal_name: String,
    pub entry: Entry,
    pub layer: Layer,
    /// The effective approval digest for this entry, after layer merge.
    pub approval: Option<ReviewDigest>,
}
#[derive(Debug, Clone, Default)]
pub struct Registry {
    pub entries: IndexMap<String, Listed>,
}
#[derive(Debug, Clone)]
pub struct Resolved {
    pub formal_name: String,
    pub entry: Entry,
    pub layer: Layer,
    /// The effective approval digest for this entry, after layer merge.
    pub approval: Option<ReviewDigest>,
}
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("{path}{instance_path}: {message}")]
    Schema {
        path: PathBuf,
        instance_path: String,
        message: String,
    },
    #[error("alias {alias} conflicts with {formal}")]
    AliasConflict { alias: String, formal: String },
    #[error("template not found: {0}")]
    NotFound(String),
}
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("template not found: {0}")]
    NotFound(String),
    #[error("ambiguous template name: {name}")]
    Ambiguous { name: String, matches: Vec<String> },
}
static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_norway::from_str(include_str!(
        "../docs/specifications/template-registry.schema.yml"
    ))
    .expect("embedded registry schema")
});
impl RegistryFile {
    pub fn load(path: &Path, layer: Layer) -> Result<Self, RegistryError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(source) => {
                return Err(RegistryError::Io {
                    path: path.into(),
                    source,
                });
            }
        };
        let value: Value = serde_norway::from_str(&text).map_err(|e| RegistryError::Parse {
            path: path.into(),
            message: e.to_string(),
        })?;
        let mut schema = SCHEMA.clone();
        schema["$ref"] = Value::String(format!(
            "#/$defs/{}",
            if layer == Layer::Local {
                "local-registry"
            } else {
                "registry"
            }
        ));
        let validator = jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .build(&schema)
            .expect("embedded registry definition");
        if let Some(e) = validator.iter_errors(&value).next() {
            return Err(RegistryError::Schema {
                path: path.into(),
                instance_path: e.instance_path().to_string(),
                message: e.to_string(),
            });
        }
        let presence: IndexMap<String, FieldPresence> = value["templates"]
            .as_object()
            .into_iter()
            .flat_map(|templates| templates.iter())
            .map(|(formal, entry)| {
                (
                    formal.clone(),
                    FieldPresence {
                        aliases: entry.get("aliases").is_some(),
                        approval: entry.get("approval").is_some(),
                    },
                )
            })
            .collect();
        // A legacy `trusted: false` records a denial; `trusted: true` is ignored.
        let denied: std::collections::HashSet<String> = value["templates"]
            .as_object()
            .into_iter()
            .flat_map(|templates| templates.iter())
            .filter(|(_, entry)| entry.get("trusted").and_then(Value::as_bool) == Some(false))
            .map(|(formal, _)| formal.clone())
            .collect();
        if layer == Layer::Local {
            let aliases: LocalRegistry =
                serde_json::from_value(value).map_err(|e| RegistryError::Parse {
                    path: path.into(),
                    message: e.to_string(),
                })?;
            Ok(Self {
                templates: aliases
                    .templates
                    .into_iter()
                    .map(|(k, v)| {
                        (
                            k,
                            Entry {
                                aliases: v.aliases,
                                ..Entry::default()
                            },
                        )
                    })
                    .collect(),
                presence,
            })
        } else {
            let mut file: Self =
                serde_json::from_value(value).map_err(|e| RegistryError::Parse {
                    path: path.into(),
                    message: e.to_string(),
                })?;
            file.presence = presence;
            for (formal, entry) in file.templates.iter_mut() {
                entry.denied = denied.contains(formal);
            }
            Ok(file)
        }
    }
    pub fn add(&self, entries: impl IntoIterator<Item = (String, Entry)>) -> Self {
        let mut next = self.clone();
        for (formal, mut entry) in entries {
            if let Some(old) = next.templates.get(&formal) {
                for alias in &old.aliases {
                    if !entry.aliases.contains(alias) {
                        entry.aliases.push(alias.clone());
                    }
                }
                if entry.approval.is_some() {
                    // An explicit new grant clears a prior denial.
                    entry.denied = false;
                } else {
                    entry.approval = old.approval.clone();
                    entry.denied = entry.denied || old.denied;
                }
            }
            next.presence.shift_remove(&formal);
            next.templates.insert(formal, entry);
        }
        next
    }
    pub fn set_commit(&self, formal: &str, commit: String) -> Result<Self, RegistryError> {
        let mut next = self.clone();
        next.templates
            .get_mut(formal)
            .ok_or_else(|| RegistryError::NotFound(formal.into()))?
            .commit = Some(commit);
        Ok(next)
    }
    pub fn remove(&self, formal: &str) -> Result<Self, RegistryError> {
        let mut next = self.clone();
        next.templates
            .shift_remove(formal)
            .ok_or_else(|| RegistryError::NotFound(formal.into()))?;
        next.presence.shift_remove(formal);
        Ok(next)
    }
    pub fn add_alias(&self, formal: &str, alias: &str) -> Result<Self, RegistryError> {
        let mut next = self.clone();
        let entry = next
            .templates
            .get_mut(formal)
            .ok_or_else(|| RegistryError::NotFound(formal.into()))?;
        if !entry.aliases.iter().any(|v| v == alias) {
            entry.aliases.push(alias.into());
        }
        next.presence
            .entry(formal.into())
            .or_insert(FieldPresence {
                aliases: true,
                approval: true,
            })
            .aliases = true;
        Ok(next)
    }
    pub fn remove_alias(&self, alias: &str) -> Result<Self, RegistryError> {
        let mut next = self.clone();
        let (formal, entry) = next
            .templates
            .iter_mut()
            .find(|(_, e)| e.aliases.iter().any(|a| a == alias))
            .ok_or_else(|| RegistryError::NotFound(alias.into()))?;
        entry.aliases.retain(|a| a != alias);
        next.presence
            .entry(formal.clone())
            .or_insert(FieldPresence {
                aliases: true,
                approval: true,
            })
            .aliases = true;
        Ok(next)
    }
    pub fn write_atomic(&self, path: &Path) -> Result<(), RegistryError> {
        let mut value = serde_json::to_value(self).map_err(|e| RegistryError::Parse {
            path: path.into(),
            message: e.to_string(),
        })?;
        for (formal, fields) in &self.presence {
            if let Some(entry) = value["templates"][formal].as_object_mut() {
                if !fields.aliases {
                    entry.remove("aliases");
                }
                if !fields.approval {
                    entry.remove("approval");
                }
            }
        }
        // A denial is written back as `trusted: false` so it survives a
        // round-trip through this and every other writer; nothing writes
        // `trusted: true`.
        for (formal, entry) in &self.templates {
            if entry.denied {
                if let Some(object) = value["templates"][formal].as_object_mut() {
                    object.insert("trusted".into(), Value::Bool(false));
                }
            }
        }
        let mut schema = SCHEMA.clone();
        schema["$ref"] = Value::String("#/$defs/registry".into());
        let validator = jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .build(&schema)
            .expect("embedded registry definition");
        if let Some(error) = validator.iter_errors(&value).next() {
            return Err(RegistryError::Schema {
                path: path.into(),
                instance_path: error.instance_path().to_string(),
                message: error.to_string(),
            });
        }
        let parent = path.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(parent).map_err(|source| RegistryError::Io {
            path: parent.into(),
            source,
        })?;
        let mut temp =
            tempfile::NamedTempFile::new_in(parent).map_err(|source| RegistryError::Io {
                path: parent.into(),
                source,
            })?;
        serde_norway::to_writer(&mut temp, &value).map_err(|e| RegistryError::Parse {
            path: path.into(),
            message: e.to_string(),
        })?;
        temp.persist(path).map_err(|e| RegistryError::Io {
            path: path.into(),
            source: e.error,
        })?;
        Ok(())
    }
}
#[derive(Deserialize)]
struct LocalRegistry {
    #[serde(default)]
    templates: IndexMap<String, LocalEntry>,
}
#[derive(Deserialize)]
struct LocalEntry {
    #[serde(default)]
    aliases: Vec<String>,
}
impl Registry {
    pub fn merge(
        system: &RegistryFile,
        user: &RegistryFile,
        local: &RegistryFile,
    ) -> Result<Self, RegistryError> {
        let mut entries: IndexMap<String, Listed> = IndexMap::new();
        for (layer, file) in [(Layer::System, system), (Layer::User, user)] {
            for (formal, entry) in &file.templates {
                let own = file.presence.get(formal).copied().unwrap_or(FieldPresence {
                    aliases: true,
                    approval: true,
                });
                let mut merged = entry.clone();
                if let Some(old) = entries.get(formal) {
                    if merged.reference.is_none() {
                        merged.reference = old.entry.reference.clone();
                    }
                    if merged.commit.is_none() {
                        merged.commit = old.entry.commit.clone();
                    }
                    if !own.aliases {
                        merged.aliases = old.entry.aliases.clone();
                    }
                    if !own.approval {
                        merged.approval = old.entry.approval.clone();
                    }
                }
                // A denial at this layer clears its own approval and any
                // approval inherited from a lower layer, so the entry reads as
                // needing review. A lower denial needs no propagation: a denied
                // lower entry already carries no approval to inherit, and a
                // higher layer that grants an approval is not itself denied.
                if merged.denied {
                    merged.approval = None;
                }
                entries.insert(
                    formal.clone(),
                    Listed {
                        formal_name: formal.clone(),
                        approval: merged.approval.clone(),
                        entry: merged,
                        layer,
                    },
                );
            }
        }
        for (formal, local_entry) in &local.templates {
            if let Some(merged) = entries.get_mut(formal) {
                if !local.presence.get(formal).is_none_or(|p| p.aliases) {
                    continue;
                }
                merged.entry.aliases = local_entry.aliases.clone();
                merged.layer = Layer::Local;
            }
        }
        let result = Self { entries };
        result.check_aliases()?;
        Ok(result)
    }
    pub fn apply_local_aliases(
        &mut self,
        local: &RegistryFile,
    ) -> Result<Vec<(String, String)>, RegistryError> {
        let mut missing = Vec::new();
        for (formal, entry) in &local.templates {
            let Some(listed) = self.entries.get_mut(formal) else {
                missing.extend(
                    entry
                        .aliases
                        .iter()
                        .map(|alias| (alias.clone(), formal.clone())),
                );
                continue;
            };
            if !local.presence.get(formal).is_none_or(|p| p.aliases) {
                continue;
            }
            listed.entry.aliases = entry.aliases.clone();
            if listed.layer != Layer::Discovered {
                listed.layer = Layer::Local;
            }
        }
        self.check_aliases()?;
        Ok(missing)
    }
    pub fn check_aliases(&self) -> Result<(), RegistryError> {
        let mut used = IndexMap::new();
        for (formal, entry) in &self.entries {
            for alias in &entry.entry.aliases {
                if self.entries.contains_key(alias)
                    || used.insert(alias.clone(), formal.clone()).is_some()
                {
                    return Err(RegistryError::AliasConflict {
                        alias: alias.clone(),
                        formal: formal.clone(),
                    });
                }
            }
        }
        Ok(())
    }
    pub fn discover(&mut self, paths: &[PathBuf], user_data: &Path) -> Result<(), RegistryError> {
        for root in paths {
            let dirs = match fs::read_dir(root) {
                Ok(dirs) => dirs,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(source) => {
                    return Err(RegistryError::Io {
                        path: root.clone(),
                        source,
                    });
                }
            };
            for dir in dirs {
                let dir = dir.map_err(|source| RegistryError::Io {
                    path: root.clone(),
                    source,
                })?;
                let path = dir.path();
                if path == user_data.join("repos")
                    || !path.is_dir()
                    || !path.join("template.yml").is_file()
                {
                    continue;
                }
                let path = path
                    .canonicalize()
                    .map_err(|source| RegistryError::Io { path, source })?;
                let formal = path.to_string_lossy().into_owned();
                if self.entries.contains_key(&formal)
                    || self.entries.values().any(|e| e.entry.path == path)
                {
                    continue;
                }
                let text = fs::read_to_string(path.join("template.yml")).map_err(|source| {
                    RegistryError::Io {
                        path: path.join("template.yml"),
                        source,
                    }
                })?;
                let yaml: Value =
                    serde_norway::from_str(&text).map_err(|e| RegistryError::Parse {
                        path: path.join("template.yml"),
                        message: e.to_string(),
                    })?;
                let name = yaml
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RegistryError::Parse {
                        path: path.join("template.yml"),
                        message: "missing name".into(),
                    })?
                    .to_string();
                self.entries.insert(
                    formal.clone(),
                    Listed {
                        formal_name: formal,
                        entry: Entry {
                            name,
                            source: path.to_string_lossy().into_owned(),
                            path,
                            ..Entry::default()
                        },
                        layer: Layer::Discovered,
                        approval: None,
                    },
                );
            }
        }
        self.check_aliases()
    }
    pub fn resolve(&self, name: &str) -> Result<Resolved, ResolveError> {
        let aliases: Vec<_> = self
            .entries
            .values()
            .filter(|v| v.entry.aliases.iter().any(|a| a == name))
            .collect();
        if let Some(v) = aliases.first() {
            return Ok(v.resolved());
        }
        let shorts: Vec<_> = self
            .entries
            .values()
            .filter(|v| v.entry.name == name)
            .collect();
        if shorts.len() > 1 {
            return Err(ResolveError::Ambiguous {
                name: name.into(),
                matches: shorts.iter().map(|v| v.formal_name.clone()).collect(),
            });
        }
        if let Some(v) = shorts.first() {
            return Ok(v.resolved());
        }
        self.entries
            .get(name)
            .map(Listed::resolved)
            .ok_or_else(|| ResolveError::NotFound(name.into()))
    }
}
impl Listed {
    fn resolved(&self) -> Resolved {
        Resolved {
            formal_name: self.formal_name.clone(),
            entry: self.entry.clone(),
            layer: self.layer,
            approval: self.approval.clone(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const D1: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    const D2: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
    fn digest(value: &str) -> ReviewDigest {
        ReviewDigest::parse(value).unwrap()
    }
    fn entry(name: &str, aliases: &[&str]) -> Entry {
        Entry {
            name: name.into(),
            source: "file:///tmp/repo".into(),
            path: PathBuf::from("/tmp/repo"),
            aliases: aliases.iter().map(|v| (*v).into()).collect(),
            ..Entry::default()
        }
    }
    #[test]
    fn merge_resolve_and_alias_conflict() {
        let mut system = RegistryFile::default();
        system
            .templates
            .insert("one".into(), entry("common", &["first"]));
        let mut user = RegistryFile::default();
        user.templates
            .insert("two".into(), entry("common", &["second"]));
        let registry = Registry::merge(&system, &user, &RegistryFile::default()).unwrap();
        assert_eq!(registry.resolve("first").unwrap().formal_name, "one");
        assert_eq!(registry.resolve("two").unwrap().formal_name, "two");
        assert!(
            matches!(registry.resolve("common"), Err(ResolveError::Ambiguous { matches, .. }) if matches.len() == 2)
        );
        assert!(matches!(
            registry.resolve("absent"),
            Err(ResolveError::NotFound(_))
        ));
        user.templates
            .get_mut("two")
            .unwrap()
            .aliases
            .push("first".into());
        assert!(matches!(
            Registry::merge(&system, &user, &RegistryFile::default()),
            Err(RegistryError::AliasConflict { .. })
        ));
    }
    #[test]
    fn higher_fields_override_only_when_present() {
        let root = tempfile::tempdir().unwrap();
        let lower_path = root.path().join("lower.yml");
        let higher_path = root.path().join("higher.yml");
        fs::write(
            &lower_path,
            format!("templates:\n  formal:\n    name: sample\n    source: file:///sample\n    path: /sample\n    ref: main\n    commit: 0000000000000000000000000000000000000000\n    aliases: [lower]\n    approval: {D1}\n"),
        )
        .unwrap();
        fs::write(
            &higher_path,
            "templates:\n  formal:\n    name: sample\n    source: file:///sample\n    path: /sample\n    aliases: []\n",
        )
        .unwrap();
        let lower = RegistryFile::load(&lower_path, Layer::System).unwrap();
        let higher = RegistryFile::load(&higher_path, Layer::User).unwrap();
        let merged = Registry::merge(&lower, &higher, &RegistryFile::default()).unwrap();
        let entry = &merged.entries["formal"].entry;
        assert!(entry.aliases.is_empty());
        assert_eq!(entry.reference.as_deref(), Some("main"));
        assert_eq!(
            entry.commit.as_deref(),
            Some("0000000000000000000000000000000000000000")
        );
        // The higher layer omits approval, so it inherits the lower digest.
        assert_eq!(entry.approval, Some(digest(D1)));
        let rewritten_path = root.path().join("rewritten.yml");
        higher.write_atomic(&rewritten_path).unwrap();
        let rewritten = RegistryFile::load(&rewritten_path, Layer::User).unwrap();
        let merged = Registry::merge(&lower, &rewritten, &RegistryFile::default()).unwrap();
        assert_eq!(merged.entries["formal"].entry.approval, Some(digest(D1)));
        assert_eq!(
            merged.entries["formal"].entry.reference.as_deref(),
            Some("main")
        );
        // A present approval overrides the lower layer's; aliases still inherit.
        fs::write(&higher_path, format!("templates:\n  formal:\n    name: sample\n    source: file:///sample\n    path: /sample\n    approval: {D2}\n")).unwrap();
        let higher = RegistryFile::load(&higher_path, Layer::User).unwrap();
        let merged = Registry::merge(&lower, &higher, &RegistryFile::default()).unwrap();
        assert_eq!(merged.entries["formal"].entry.aliases, ["lower"]);
        assert_eq!(merged.entries["formal"].entry.approval, Some(digest(D2)));
    }
    #[test]
    fn sparse_entry_writes_track_fields_set_by_each_operation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("templates.yml");
        fs::write(
            &path,
            "templates:\n  formal:\n    name: sample\n    source: file:///sample\n    path: /sample\n",
        )
        .unwrap();
        let sparse = RegistryFile::load(&path, Layer::User).unwrap();
        let mut replacement = entry("sample", &["kept"]);
        replacement.approval = Some(digest(D1));
        sparse
            .add([("formal".into(), replacement)])
            .write_atomic(&path)
            .unwrap();
        let written: Value = serde_norway::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            written["templates"]["formal"]["aliases"],
            serde_json::json!(["kept"])
        );
        assert_eq!(written["templates"]["formal"]["approval"], D1);

        let commit = "0000000000000000000000000000000000000000";
        sparse
            .set_commit("formal", commit.into())
            .unwrap()
            .write_atomic(&path)
            .unwrap();
        let written: Value = serde_norway::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["templates"]["formal"]["commit"], commit);
        assert!(written["templates"]["formal"].get("aliases").is_none());
        assert!(written["templates"]["formal"].get("approval").is_none());
        assert!(matches!(
            sparse.set_commit("absent", commit.into()),
            Err(RegistryError::NotFound(_))
        ));

        let with_alias = sparse.add_alias("formal", "kept").unwrap();
        with_alias.write_atomic(&path).unwrap();
        let written: Value = serde_norway::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            written["templates"]["formal"]["aliases"],
            serde_json::json!(["kept"])
        );
        assert!(written["templates"]["formal"].get("approval").is_none());
        with_alias
            .remove_alias("kept")
            .unwrap()
            .write_atomic(&path)
            .unwrap();
        let written: Value = serde_norway::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            written["templates"]["formal"]["aliases"],
            serde_json::json!([])
        );
        assert!(written["templates"]["formal"].get("approval").is_none());

        sparse
            .remove("formal")
            .unwrap()
            .write_atomic(&path)
            .unwrap();
        let written: Value = serde_norway::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(written["templates"].get("formal").is_none());
    }
    #[test]
    fn local_layer_carries_no_approval() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("templates.yml");
        fs::write(&path, "templates:\n  formal:\n    aliases: [local]\n").unwrap();
        let local = RegistryFile::load(&path, Layer::Local).unwrap();
        let mut user = RegistryFile::default();
        user.templates.insert("formal".into(), entry("short", &[]));
        let merged = Registry::merge(&RegistryFile::default(), &user, &local).unwrap();
        assert!(merged.resolve("local").unwrap().approval.is_none());
        fs::write(
            &path,
            format!("templates:\n  formal:\n    approval: {D1}\n"),
        )
        .unwrap();
        assert!(matches!(
            RegistryFile::load(&path, Layer::Local),
            Err(RegistryError::Schema { .. })
        ));
    }
    #[test]
    fn readding_keeps_aliases_and_approval() {
        let mut existing = RegistryFile::default();
        let mut original = entry("short", &["old"]);
        original.approval = Some(digest(D1));
        existing.templates.insert("formal".into(), original);
        let next = existing.add([("formal".into(), entry("short", &["new"]))]);
        assert_eq!(next.templates["formal"].aliases, ["new", "old"]);
        assert_eq!(next.templates["formal"].approval, Some(digest(D1)));
    }
    #[test]
    fn invalid_write_preserves_existing_registry() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("templates.yml");
        fs::write(&path, "templates: {}\n").unwrap();
        let mut file = RegistryFile::default();
        file.templates
            .insert("formal".into(), entry("short", &["INVALID"]));
        assert!(matches!(
            file.write_atomic(&path),
            Err(RegistryError::Schema { .. })
        ));
        assert_eq!(fs::read_to_string(path).unwrap(), "templates: {}\n");
    }
    #[test]
    fn user_approval_field_has_precedence() {
        let mut system = RegistryFile::default();
        let mut approved = entry("short", &[]);
        approved.approval = Some(digest(D1));
        system.templates.insert("formal".into(), approved);
        let mut user = RegistryFile::default();
        user.templates.insert("formal".into(), entry("short", &[]));
        let merged = Registry::merge(&system, &user, &RegistryFile::default()).unwrap();
        assert!(merged.resolve("formal").unwrap().approval.is_none());
    }
    #[test]
    fn legacy_trusted_field_loads_and_grants_no_trust() {
        // A registry written before approval digests carries `trusted`. It must
        // load (Decision 4: no migration) and grant no trust, reading as
        // needs-review, while the local layer still forbids the field.
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("templates.yml");
        fs::write(
            &path,
            "templates:\n  formal:\n    name: sample\n    source: file:///sample\n    path: /sample\n    trusted: true\n",
        )
        .unwrap();
        let user = RegistryFile::load(&path, Layer::User).unwrap();
        assert!(user.templates["formal"].approval.is_none());
        let merged =
            Registry::merge(&RegistryFile::default(), &user, &RegistryFile::default()).unwrap();
        assert!(merged.resolve("sample").unwrap().approval.is_none());
        // Rewriting the registry drops the legacy field.
        user.write_atomic(&path).unwrap();
        let written: Value = serde_norway::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(written["templates"]["formal"].get("trusted").is_none());
        // The local layer never accepted a trust-granting field.
        let local_path = root.path().join("local.yml");
        fs::write(&local_path, "templates:\n  formal:\n    trusted: true\n").unwrap();
        assert!(matches!(
            RegistryFile::load(&local_path, Layer::Local),
            Err(RegistryError::Schema { .. })
        ));
    }
    #[test]
    fn higher_layer_denial_clears_lower_approval() {
        // A user layer loaded with `trusted: false` clears a system approval
        // for the same formal name, so the template reads as needing review.
        let root = tempfile::tempdir().unwrap();
        let system_path = root.path().join("system.yml");
        let user_path = root.path().join("user.yml");
        fs::write(
            &system_path,
            format!("templates:\n  formal:\n    name: sample\n    source: file:///sample\n    path: /sample\n    approval: {D1}\n"),
        )
        .unwrap();
        fs::write(
            &user_path,
            "templates:\n  formal:\n    name: sample\n    source: file:///sample\n    path: /sample\n    trusted: false\n",
        )
        .unwrap();
        let system = RegistryFile::load(&system_path, Layer::System).unwrap();
        let user = RegistryFile::load(&user_path, Layer::User).unwrap();
        assert!(user.templates["formal"].denied);
        let merged = Registry::merge(&system, &user, &RegistryFile::default()).unwrap();
        assert!(
            merged.resolve("sample").unwrap().approval.is_none(),
            "a higher-layer denial clears the lower approval"
        );
        // A user layer that omits both fields inherits the system approval.
        fs::write(
            &user_path,
            "templates:\n  formal:\n    name: sample\n    source: file:///sample\n    path: /sample\n",
        )
        .unwrap();
        let user = RegistryFile::load(&user_path, Layer::User).unwrap();
        let merged = Registry::merge(&system, &user, &RegistryFile::default()).unwrap();
        assert_eq!(merged.resolve("sample").unwrap().approval, Some(digest(D1)));
    }
    #[test]
    fn explicit_higher_grant_clears_lower_denial() {
        // A system `trusted: false` denial is overridden by an explicit user
        // approval for the same formal name.
        let root = tempfile::tempdir().unwrap();
        let system_path = root.path().join("system.yml");
        let user_path = root.path().join("user.yml");
        fs::write(
            &system_path,
            "templates:\n  formal:\n    name: sample\n    source: file:///sample\n    path: /sample\n    trusted: false\n",
        )
        .unwrap();
        fs::write(
            &user_path,
            format!("templates:\n  formal:\n    name: sample\n    source: file:///sample\n    path: /sample\n    approval: {D2}\n"),
        )
        .unwrap();
        let system = RegistryFile::load(&system_path, Layer::System).unwrap();
        let user = RegistryFile::load(&user_path, Layer::User).unwrap();
        let merged = Registry::merge(&system, &user, &RegistryFile::default()).unwrap();
        assert_eq!(merged.resolve("sample").unwrap().approval, Some(digest(D2)));
        // With the user omitting both, the system denial is inherited.
        fs::write(
            &user_path,
            "templates:\n  formal:\n    name: sample\n    source: file:///sample\n    path: /sample\n",
        )
        .unwrap();
        let user = RegistryFile::load(&user_path, Layer::User).unwrap();
        let merged = Registry::merge(&system, &user, &RegistryFile::default()).unwrap();
        assert!(merged.resolve("sample").unwrap().approval.is_none());
    }
    #[test]
    fn denial_survives_writers_and_a_grant_clears_it() {
        // A denial round-trips through write_atomic and an unrelated writer, and
        // an explicit grant clears it.
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("templates.yml");
        fs::write(
            &path,
            "templates:\n  formal:\n    name: sample\n    source: file:///sample\n    path: /sample\n    trusted: false\n",
        )
        .unwrap();
        let file = RegistryFile::load(&path, Layer::User).unwrap();
        file.write_atomic(&path).unwrap();
        let written: Value = serde_norway::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["templates"]["formal"]["trusted"], false);
        let reloaded = RegistryFile::load(&path, Layer::User).unwrap();
        assert!(reloaded.templates["formal"].denied);
        // An unrelated writer keeps the denial.
        reloaded
            .add_alias("formal", "kept")
            .unwrap()
            .write_atomic(&path)
            .unwrap();
        let written: Value = serde_norway::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["templates"]["formal"]["trusted"], false);
        // An explicit grant (add carrying an approval) clears the denial.
        let mut grant = entry("sample", &[]);
        grant.approval = Some(digest(D1));
        let granted = reloaded.add([("formal".into(), grant)]);
        assert!(!granted.templates["formal"].denied);
        granted.write_atomic(&path).unwrap();
        let written: Value = serde_norway::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(written["templates"]["formal"].get("trusted").is_none());
        assert_eq!(written["templates"]["formal"]["approval"], D1);
    }
    #[test]
    fn discovery_reads_only_name() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("sample");
        fs::create_dir(&folder).unwrap();
        fs::write(
            folder.join("template.yml"),
            "name: sample\nnot-valid: true\n",
        )
        .unwrap();
        let mut registry = Registry::default();
        registry
            .discover(&[root.path().to_path_buf()], root.path())
            .unwrap();
        let resolved = registry.resolve("sample").unwrap();
        assert_eq!(resolved.layer, Layer::Discovered);
        assert!(resolved.approval.is_none());
    }
}
