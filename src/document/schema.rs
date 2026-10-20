// ---
// relationships:
//   implements: template-composition
// ---
use jsonschema::{Retrieve, Uri};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug, Default)]
pub(crate) struct SchemaIndex(Arc<BTreeMap<String, Value>>);
impl SchemaIndex {
    pub(crate) fn load(
        root: &Path,
        source: &Path,
        directory: Option<&str>,
    ) -> Result<Self, String> {
        let Some(directory) = directory else {
            return Ok(Self::default());
        };
        let path = root.join(directory);
        let canonical = path
            .canonicalize()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if Path::new(directory).is_absolute()
            || !canonical.starts_with(root)
            || canonical.starts_with(source)
            || source.starts_with(&canonical)
        {
            return Err(
                "schemas directory must stay inside template root and outside source".into(),
            );
        }
        let mut paths = vec![];
        schema_files(&canonical, root, &mut paths)?;
        paths.sort();
        let mut schemas = BTreeMap::new();
        let mut origins = BTreeMap::new();
        for path in paths {
            let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let schema: Value = if path.extension().is_some_and(|e| e == "json") {
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?
            } else {
                serde_norway::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?
            };
            let id = schema
                .get("$id")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{}: schema needs a string $id", path.display()))?
                .to_owned();
            Uri::parse(id.as_str()).map_err(|e| format!("{}: invalid $id: {e}", path.display()))?;
            if let Some(first) = origins.insert(id.clone(), path.clone()) {
                return Err(format!(
                    "duplicate schema $id {id}: {} and {}",
                    first.display(),
                    path.display()
                ));
            }
            schemas.insert(id, schema);
        }
        Ok(Self(Arc::new(schemas)))
    }
    pub(super) fn resolve(&self, uri: &str) -> Result<Value, String> {
        if let Some(schema) = self.0.get(uri) {
            return Ok(schema.clone());
        }
        fetch(uri).map_err(|e| format!("schema {uri}: {e}"))
    }
}
impl Retrieve for SchemaIndex {
    fn retrieve(
        &self,
        uri: &Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        self.resolve(uri.as_str()).map_err(Into::into)
    }
}
fn schema_files(directory: &Path, root: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|e| format!("{}: {e}", directory.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if fs::symlink_metadata(&path)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err(format!(
                "{}: schema directory cannot contain symlinks",
                path.display()
            ));
        }
        if path.is_dir() {
            schema_files(&path, root, paths)?;
        } else if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|e| matches!(e, "json" | "yml" | "yaml"))
        {
            if !path.starts_with(root) {
                return Err(format!("{}: schema leaves template root", path.display()));
            }
            paths.push(path);
        }
    }
    Ok(())
}
fn fetch(uri: &str) -> Result<Value, String> {
    let parsed = Uri::parse(uri).map_err(|e| e.to_string())?;
    if parsed.scheme().as_str() != "https" {
        return Err("schema fetch requires HTTPS".into());
    }
    let mut easy = curl::easy::Easy::new();
    easy.url(uri).map_err(|e| e.to_string())?;
    easy.fail_on_error(true).map_err(|e| e.to_string())?;
    // Use the same platform roots as HTTPS template sources, including bundled rustls.
    let mut roots = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    if let Some(path) = std::env::var_os("GIT_SSL_CAINFO") {
        easy.cainfo(Path::new(&path)).map_err(|e| e.to_string())?;
    } else if curl::Version::get()
        .ssl_version()
        .is_some_and(|v| v.starts_with("rustls"))
    {
        use base64::Engine;
        for cert in rustls_native_certs::load_native_certs().certs {
            writeln!(roots, "-----BEGIN CERTIFICATE-----").map_err(|e| e.to_string())?;
            let encoded = base64::engine::general_purpose::STANDARD.encode(cert.as_ref());
            for line in encoded.as_bytes().chunks(64) {
                roots.write_all(line).map_err(|e| e.to_string())?;
                roots.write_all(b"\n").map_err(|e| e.to_string())?;
            }
            writeln!(roots, "-----END CERTIFICATE-----").map_err(|e| e.to_string())?;
        }
        roots.flush().map_err(|e| e.to_string())?;
        if roots.as_file().metadata().map_err(|e| e.to_string())?.len() > 0 {
            easy.cainfo(roots.path()).map_err(|e| e.to_string())?;
        }
    }
    let mut bytes = Vec::new();
    let mut oversized = false;
    let result = {
        let mut transfer = easy.transfer();
        transfer
            .write_function(|chunk| {
                if bytes.len() + chunk.len() > 1024 * 1024 {
                    oversized = true;
                    return Ok(0);
                }
                bytes.extend_from_slice(chunk);
                Ok(chunk.len())
            })
            .map_err(|e| e.to_string())?;
        transfer.perform()
    };
    if let Err(e) = result {
        return Err(if oversized {
            "schema exceeds 1 MiB".into()
        } else {
            e.to_string()
        });
    }
    let schema: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if schema.get("$id").and_then(Value::as_str) != Some(uri) {
        return Err("fetched schema $id does not equal requested URI".into());
    }
    Ok(schema)
}
