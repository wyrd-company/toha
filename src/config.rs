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
/// A configured default: a preset's name or a literal value. Its `Debug`
/// hides a literal, which can be a URL with credentials in any form.
#[derive(Clone, PartialEq)]
pub enum DefaultSource {
    Ref(PresetName),
    Literal(Value),
}
impl fmt::Debug for DefaultSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ref(name) => f.debug_tuple("Ref").field(name).finish(),
            Self::Literal(_) => f.debug_tuple("Literal").field(&HIDDEN).finish(),
        }
    }
}
/// What `Debug` shows in place of a configured value.
const HIDDEN: Hidden = Hidden;
struct Hidden;
impl fmt::Debug for Hidden {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<hidden>")
    }
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
/// A configured value and where it came from. Its `Debug` shows the origin
/// and hides the value, which can be a URL with credentials in any form.
#[derive(Clone, PartialEq)]
pub struct ConfigEntry<T> {
    pub value: T,
    pub origin: ConfigOrigin,
}
impl<T> fmt::Debug for ConfigEntry<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfigEntry")
            .field("value", &HIDDEN)
            .field("origin", &self.origin)
            .finish()
    }
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
/// The loaded configuration. Its `Debug` shows host bases and template
/// addresses without user information and hides configured values, so no
/// credential a URL in it carries is shown; the values keep them, as Toha
/// authenticates with them.
#[derive(Clone)]
pub struct Config {
    pub templates_paths: Vec<PathBuf>,
    pub local_templates_paths: Vec<PathBuf>,
    pub presets: IndexMap<PresetName, ConfigEntry<Value>>,
    pub template_defaults: IndexMap<String, IndexMap<Id, ConfigEntry<DefaultSource>>>,
    pub hosts: IndexMap<String, String>,
    pub local_config_name: String,
}
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shown = crate::source::without_userinfo;
        f.debug_struct("Config")
            .field("templates_paths", &self.templates_paths)
            .field("local_templates_paths", &self.local_templates_paths)
            .field("presets", &self.presets)
            .field(
                "template_defaults",
                &self
                    .template_defaults
                    .iter()
                    .map(|(template, defaults)| (shown(template), defaults))
                    .collect::<IndexMap<_, _>>(),
            )
            .field(
                "hosts",
                &self
                    .hosts
                    .iter()
                    .map(|(name, base)| (name, shown(base)))
                    .collect::<IndexMap<_, _>>(),
            )
            .field("local_config_name", &self.local_config_name)
            .finish()
    }
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
        message: crate::schema_message::yaml(&e),
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
    // The schema library's message writes the instance it rejects, which can
    // hold the credentials of a URL in a preset, a default or a host base, so
    // Toha writes its own, without the instance.
    if let Some(error) = validator.iter_errors(&value).next() {
        return Err(ConfigError::Schema {
            path: path.into(),
            instance_path: crate::schema_message::path(&error),
            message: crate::schema_message::message(&error),
        });
    }
    serde_json::from_value(value).map_err(|e| ConfigError::Parse {
        path: path.into(),
        message: crate::schema_message::json(&e),
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
    fn a_loaded_configuration_shows_no_credentials() {
        let root = tempfile::tempdir().unwrap();
        let user = root.path().join("user.yml");
        let host = "https://user:p%40ss-config-host-secret@host.invalid";
        let preset = "https://user:p%40ss-config-preset-secret@host.invalid/r.git";
        let default = "https://user:p%40ss-config-default-secret@host.invalid/r.git";
        let keyed = "https://user:p%40ss-config-key-secret@host.invalid/t.git";
        fs::write(
            &user,
            format!(
                "hosts: {{ review: {host:?} }}\npresets: {{ review_remote: {preset:?} }}\n\
                 template-defaults: {{ sample: {{ remote: {default:?} }}, {keyed:?}: {{ other: x }} }}\n"
            ),
        )
        .unwrap();
        let dirs = ConfigPaths {
            system_config: root.path().join("absent-system.yml"),
            user_config: user,
            local_config_override: None,
            system_data: root.path().join("system-data"),
            user_data: root.path().join("user-data"),
            cache: root.path().join("cache"),
            home: root.path().join("home"),
        };
        let config = load(&dirs, root.path()).unwrap();
        let shown = format!("{config:?}");
        assert!(!shown.contains("secret"), "{shown}");
        assert!(
            shown.contains("\"review\": \"https://host.invalid\""),
            "{shown}"
        );
        // The values keep their credentials, as Toha authenticates with them.
        assert_eq!(config.hosts["review"], host);
        assert_eq!(
            config.presets[&PresetName::parse("review_remote").unwrap()].value,
            preset
        );
        let entry = &config.template_defaults["sample"][&Id::parse("remote").unwrap()];
        assert_eq!(
            entry.value,
            DefaultSource::Literal(Value::String(default.into()))
        );
        assert!(!format!("{entry:?} {:?}", entry.value).contains("secret"));
    }
    #[test]
    fn schema_errors_show_no_credentials_of_presets_defaults_or_hosts() {
        let url = "https://user:p%40ss-config-secret@127.0.0.1:9/r.git";
        let root = tempfile::tempdir().unwrap();
        let local = root.path().join("local.yml");
        for text in [
            format!("presets: {{ remote: {{ {url:?}: 1 }} }}\n"),
            format!("presets: {{ remote: [{url:?}, 1] }}\n"),
            format!("template-defaults: {{ {url:?}: {{ remote: {{ {url:?}: 1 }} }} }}\n"),
            format!("template-defaults: {{ sample: {{ remote: [{url:?}, 1] }} }}\n"),
            format!("template-defaults: {{ {url:?}: 1 }}\n"),
            format!("hosts: {{ gh: {url:?} }}\n"),
            format!("template-defaults: {{ sample: {{ remote: {{ preset: [{url:?}] }} }} }}\n"),
            format!("template-defaults: {{ sample: {{ remote: {{ preset: {url:?} }} }} }}\n"),
            format!(
                "template-defaults: {{ sample: {{ remote: {{ preset: {{ {url:?}: 1 }} }} }} }}\n"
            ),
            format!("template-defaults: {{ sample: [{url:?}] }}\n"),
            format!("presets: [{url:?}]\n"),
            format!("presets: {{ {url:?}: x }}\n"),
            format!("{url:?}: 1\n"),
            format!("presets: {{ remote: !!int {url} }}\n"),
            format!("presets: {{ remote: !!float {url} }}\n"),
            format!("template-defaults: {{ {url:?}: {{ remote: !!int {url} }} }}\n"),
        ] {
            fs::write(&local, &text).unwrap();
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
            assert!(
                matches!(
                    error,
                    ConfigError::Schema { .. } | ConfigError::Parse { .. }
                ),
                "{text}: {error}"
            );
            let shown = format!("{error} {error:?}");
            assert!(!shown.contains("secret"), "{text}: {shown}");
        }
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
