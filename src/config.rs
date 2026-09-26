//! Layered configuration for template lookup.
use crate::Id;
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
};

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
    pub defaults: IndexMap<Id, Value>,
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
    defaults: Option<IndexMap<String, Value>>,
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
    let mut defaults = IndexMap::new();
    for (layer, file) in [
        (&system, &dirs.system_config),
        (&user, &dirs.user_config),
        (&local, &local_file),
    ] {
        if let Some(values) = &layer.defaults {
            for (key, value) in values {
                defaults.insert(
                    Id::parse(key).map_err(|message| ConfigError::Parse {
                        path: file.clone(),
                        message,
                    })?,
                    value.clone(),
                );
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
        defaults,
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
        fs::write(&system, "templates-paths: [system-templates]\ndefaults: { answer: system }\nhosts: { gh: 'https://system.invalid' }\nlocal-config-name: project.yml\n").unwrap();
        fs::write(&user, "templates-paths: [~/shared]\ndefaults: { answer: user }\nhosts: { gh: 'https://user.invalid' }\n").unwrap();
        fs::write(
            base.join("project/project.yml"),
            "templates-paths: [local-templates]\ndefaults: { answer: local }\n",
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
        assert_eq!(config.defaults[&Id::parse("answer").unwrap()], "local");
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
}
