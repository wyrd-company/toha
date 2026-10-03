//! Layered configuration for template lookup.
use crate::Id;
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;
use std::{
    fmt, fs,
    path::{Path, PathBuf},
    sync::LazyLock,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PresetName(String);
impl PresetName {
    pub fn parse(value: &str) -> Result<Self, String> {
        Id::parse(value).map(|_| Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for PresetName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum DefaultSource {
    Ref(PresetName),
    Literal(Value),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigLayer {
    System,
    User,
    Local,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigOrigin {
    pub layer: ConfigLayer,
    pub path: PathBuf,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigEntry<T> {
    pub value: T,
    pub origin: ConfigOrigin,
}

#[derive(Debug, Clone)]
pub struct ConfigPaths {
    pub system_config: PathBuf,
    pub user_config: PathBuf,
    pub local_config_override: Option<PathBuf>,
    pub system_data: PathBuf,
    pub user_data: PathBuf,
    pub cache: PathBuf,
    pub home: PathBuf,
}
#[derive(Debug, Clone)]
pub struct Config {
    pub templates_paths: Vec<PathBuf>,
    pub local_templates_paths: Vec<PathBuf>,
    pub presets: IndexMap<PresetName, ConfigEntry<Value>>,
    pub template_defaults: IndexMap<String, IndexMap<Id, ConfigEntry<DefaultSource>>>,
    pub hosts: IndexMap<String, String>,
    pub local_config_name: String,
}
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
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
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Layer {
    templates_paths: Option<Vec<String>>,
    presets: Option<IndexMap<String, Value>>,
    template_defaults: Option<IndexMap<String, IndexMap<String, Value>>>,
    hosts: Option<IndexMap<String, String>>,
    local_config_name: Option<String>,
}
static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_norway::from_str(include_str!("../docs/specifications/config.schema.yml"))
        .expect("embedded config schema")
});
fn read(path: &Path, definition: &str) -> Result<Layer, ConfigError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Layer::default()),
        Err(source) => {
            return Err(ConfigError::Io {
                path: path.into(),
                source,
            });
        }
    };
    let value: Value = serde_norway::from_str(&text).map_err(|e| ConfigError::Parse {
        path: path.into(),
        message: e.to_string(),
    })?;
    if value.get("defaults").is_some() {
        return Err(ConfigError::Parse {
            path: path.into(),
            message: "`defaults:` is no longer supported; preserve each value under a named `presets:` entry and add an explicit `template-defaults:` mapping keyed by the template formal name (for example, `title: { preset: default_title }`)".into(),
        });
    }
    let mut schema = SCHEMA.clone();
    schema["$ref"] = Value::String(format!("#/$defs/{definition}"));
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(&schema)
        .expect("embedded config definition");
    if let Some(error) = validator.iter_errors(&value).next() {
        return Err(ConfigError::Schema {
            path: path.into(),
            instance_path: error.instance_path().to_string(),
            message: error.to_string(),
        });
    }
    serde_json::from_value(value).map_err(|e| ConfigError::Parse {
        path: path.into(),
        message: e.to_string(),
    })
}
fn path(value: &str, file: &Path, home: &Path) -> PathBuf {
    let expanded = if value == "~" {
        home.to_path_buf()
    } else if let Some(rest) = value.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(value)
    };
    if expanded.is_absolute() {
        expanded
    } else {
        file.parent().unwrap_or(Path::new(".")).join(expanded)
    }
}
fn paths(layer: &Layer, file: &Path, default: PathBuf, home: &Path) -> Vec<PathBuf> {
    layer.templates_paths.as_ref().map_or_else(
        || vec![default],
        |values| values.iter().map(|v| path(v, file, home)).collect(),
    )
}
pub fn load(dirs: &ConfigPaths, cwd: &Path) -> Result<Config, ConfigError> {
    let system = read(&dirs.system_config, "shared-config")?;
    let user = read(&dirs.user_config, "shared-config")?;
    let local_config_name = user
        .local_config_name
        .clone()
        .or(system.local_config_name.clone())
        .unwrap_or_else(|| ".toha.yml".into());
    let local_file = dirs
        .local_config_override
        .as_ref()
        .map(|value| {
            if value.is_absolute() {
                value.clone()
            } else {
                cwd.join(value)
            }
        })
        .unwrap_or_else(|| cwd.join(&local_config_name));
    let local = read(&local_file, "local-config")?;
    let local_default = cwd.join(".templates");
    let local_paths = paths(&local, &local_file, local_default, &dirs.home);

    let templates_paths = local_paths
        .clone()
        .into_iter()
        .chain(paths(
            &user,
            &dirs.user_config,
            dirs.user_data.clone(),
            &dirs.home,
        ))
        .chain(paths(
            &system,
            &dirs.system_config,
            dirs.system_data.clone(),
            &dirs.home,
        ))
        .collect();
    let mut presets = IndexMap::new();
    let mut template_defaults: IndexMap<String, IndexMap<Id, ConfigEntry<DefaultSource>>> =
        IndexMap::new();
    for (layer, file, layer_name) in [
        (&system, &dirs.system_config, ConfigLayer::System),
        (&user, &dirs.user_config, ConfigLayer::User),
        (&local, &local_file, ConfigLayer::Local),
    ] {
        let origin = ConfigOrigin {
            layer: layer_name,
            path: file.clone(),
        };
        if let Some(values) = &layer.presets {
            for (key, value) in values {
                presets.insert(
                    PresetName::parse(key).map_err(|message| ConfigError::Parse {
                        path: file.clone(),
                        message,
                    })?,
                    ConfigEntry {
                        value: value.clone(),
                        origin: origin.clone(),
                    },
                );
            }
        }
        if let Some(identities) = &layer.template_defaults {
            for (formal_name, values) in identities {
                let merged = template_defaults.entry(formal_name.clone()).or_default();
                for (key, value) in values {
                    let id = Id::parse(key).map_err(|message| ConfigError::Parse {
                        path: file.clone(),
                        message,
                    })?;
                    let source = match value {
                        Value::Object(object) => {
                            let name = object
                                .get("preset")
                                .and_then(Value::as_str)
                                .expect("schema validates preset references");
                            DefaultSource::Ref(PresetName::parse(name).map_err(|message| {
                                ConfigError::Parse {
                                    path: file.clone(),
                                    message,
                                }
                            })?)
                        }
                        literal => DefaultSource::Literal(literal.clone()),
                    };
                    merged.insert(
                        id,
                        ConfigEntry {
                            value: source,
                            origin: origin.clone(),
                        },
                    );
                }
            }
        }
    }
    let mut hosts = IndexMap::from([
        ("gh".into(), "https://github.com".into()),
        ("gl".into(), "https://gitlab.com".into()),
        ("bb".into(), "https://bitbucket.org".into()),
        ("cb".into(), "https://codeberg.org".into()),
        ("ge".into(), "https://gitee.com".into()),
    ]);
    for layer in [&system, &user] {
        if let Some(values) = &layer.hosts {
            hosts.extend(values.clone());
        }
    }
    Ok(Config {
        templates_paths,
        local_templates_paths: local_paths.clone(),
        presets,
        template_defaults,
        hosts,
        local_config_name,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merges_layers_and_resolves_paths() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path();
        for dir in ["system", "user", "project", "home"] {
            fs::create_dir_all(base.join(dir)).unwrap();
        }
        let system = base.join("system/config.yml");
        let user = base.join("user/config.yml");
        fs::write(&system, "templates-paths: [system-templates]\npresets: { shared_value: system, system_value: retained }\ntemplate-defaults: { example-template: { answer: { preset: shared_value }, system_answer: system } }\nhosts: { gh: 'https://system.invalid' }\nlocal-config-name: project.yml\n").unwrap();
        fs::write(&user, "templates-paths: [~/shared]\npresets: { shared_value: user }\ntemplate-defaults: { example-template: { answer: user } }\nhosts: { gh: 'https://user.invalid' }\n").unwrap();
        fs::write(
            base.join("project/project.yml"),
            "templates-paths: [local-templates]\npresets: { shared_value: local }\ntemplate-defaults: { example-template: { answer: local } }\n",
        )
        .unwrap();
        let dirs = ConfigPaths {
            system_config: system,
            user_config: user,
            local_config_override: None,
            system_data: base.join("system-data"),
            user_data: base.join("user-data"),
            cache: base.join("cache"),
            home: base.join("home"),
        };
        let config = load(&dirs, &base.join("project")).unwrap();
        assert_eq!(config.local_config_name, "project.yml");
        assert_eq!(
            config.templates_paths,
            vec![
                base.join("project/local-templates"),
                base.join("home/shared"),
                base.join("system/system-templates")
            ]
        );
        let shared = &config.presets[&PresetName::parse("shared_value").unwrap()];
        assert_eq!(shared.value, "local");
        assert_eq!(shared.origin.layer, ConfigLayer::Local);
        assert_eq!(shared.origin.path, base.join("project/project.yml"));
        assert_eq!(
            config.presets[&PresetName::parse("system_value").unwrap()]
                .origin
                .layer,
            ConfigLayer::System
        );
        let mapping = &config.template_defaults["example-template"];
        let answer = &mapping[&Id::parse("answer").unwrap()];
        assert_eq!(
            answer.value,
            DefaultSource::Literal(Value::String("local".into()))
        );
        assert_eq!(answer.origin.layer, ConfigLayer::Local);
        assert_eq!(
            mapping[&Id::parse("system_answer").unwrap()].origin.layer,
            ConfigLayer::System
        );
        assert_eq!(config.hosts["gh"], "https://user.invalid");
    }
    #[test]
    fn local_forbidden_fields_report_file_and_path() {
        let root = tempfile::tempdir().unwrap();
        let local = root.path().join("local.yml");
        fs::write(&local, "hosts: { gh: 'https://wrong.invalid' }\n").unwrap();
        let dirs = ConfigPaths {
            system_config: root.path().join("missing-system"),
            user_config: root.path().join("missing-user"),
            local_config_override: Some(local.clone()),
            system_data: root.path().join("system"),
            user_data: root.path().join("user"),
            cache: root.path().join("cache"),
            home: root.path().join("home"),
        };
        let error = load(&dirs, root.path()).unwrap_err();
        assert!(error.to_string().contains(local.to_str().unwrap()));
        assert!(matches!(error, ConfigError::Schema { .. }));
    }

    #[test]
    fn legacy_defaults_are_refused_with_conversion_guidance() {
        let root = tempfile::tempdir().unwrap();
        let user = root.path().join("user.yml");
        fs::write(&user, "defaults: { answer: kept }\n").unwrap();
        let dirs = ConfigPaths {
            system_config: root.path().join("missing-system"),
            user_config: user.clone(),
            local_config_override: Some(root.path().join("missing-local")),
            system_data: root.path().join("system"),
            user_data: root.path().join("user"),
            cache: root.path().join("cache"),
            home: root.path().join("home"),
        };
        let error = load(&dirs, root.path()).unwrap_err().to_string();
        assert!(error.starts_with(user.to_str().unwrap()), "{error}");
        assert!(error.contains("`presets:`"), "{error}");
        assert!(error.contains("`template-defaults:`"), "{error}");
    }

    #[test]
    fn preset_store_rejects_reference_objects() {
        let root = tempfile::tempdir().unwrap();
        let user = root.path().join("user.yml");
        fs::write(
            &user,
            "presets: { first_value: { preset: second_value } }\n",
        )
        .unwrap();
        let dirs = ConfigPaths {
            system_config: root.path().join("missing-system"),
            user_config: user,
            local_config_override: Some(root.path().join("missing-local")),
            system_data: root.path().join("system"),
            user_data: root.path().join("user"),
            cache: root.path().join("cache"),
            home: root.path().join("home"),
        };
        assert!(matches!(
            load(&dirs, root.path()),
            Err(ConfigError::Schema { .. })
        ));
    }
}
