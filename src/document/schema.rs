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
    time::Duration,
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
/// What a schema fetch the operation's [stop signal](crate::stop) ended
/// reports.
const STOPPED: &str = "the schema fetch was stopped";
/// The longest a stoppable fetch waits for curl before it checks the signal.
const STOP_CHECK: Duration = Duration::from_millis(100);
/// A fetched schema body, up to 1 MiB.
#[derive(Default)]
struct Body {
    bytes: Vec<u8>,
    oversized: bool,
}
impl curl::easy::Handler for Body {
    fn write(&mut self, chunk: &[u8]) -> Result<usize, curl::easy::WriteError> {
        if self.bytes.len() + chunk.len() > 1024 * 1024 {
            self.oversized = true;
            return Ok(0);
        }
        self.bytes.extend_from_slice(chunk);
        Ok(chunk.len())
    }
}
/// Fetch the schema at `uri`, on the calling thread.
///
/// Inside a stop signal's scope the transfer runs on a curl multi handle that
/// this thread drives, checking the signal at least every [`STOP_CHECK`],
/// also while the endpoint sends nothing. curl's own progress callback is not
/// called while some builds wait for a TLS handshake. Once the signal fires,
/// the transfer is removed and its handle dropped, which closes its
/// connection, before this returns. Without a signal the transfer runs as
/// before.
fn fetch(uri: &str) -> Result<Value, String> {
    let parsed = Uri::parse(uri).map_err(|e| e.to_string())?;
    if parsed.scheme().as_str() != "https" {
        return Err("schema fetch requires HTTPS".into());
    }
    let mut easy = curl::easy::Easy2::new(Body::default());
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
    let (result, body) = if crate::stop::scoped() {
        perform_stoppable(easy)?
    } else {
        let result = easy.perform();
        (result, std::mem::take(easy.get_mut()))
    };
    if let Err(e) = result {
        return Err(if body.oversized {
            "schema exceeds 1 MiB".into()
        } else {
            e.to_string()
        });
    }
    let schema: Value = serde_json::from_slice(&body.bytes).map_err(|e| e.to_string())?;
    if schema.get("$id").and_then(Value::as_str) != Some(uri) {
        return Err("fetched schema $id does not equal requested URI".into());
    }
    Ok(schema)
}
/// `CURLOPT_QUICK_EXIT`, which curl-sys 0.4.91 does not define: a transfer's
/// cleanup detaches its resolver threads instead of joining them.
const CURLOPT_QUICK_EXIT: curl_sys::CURLoption = curl_sys::CURLOPTTYPE_LONG + 322;
/// `CURLMOPT_QUICK_EXIT`, which curl-sys 0.4.91 does not define: the multi
/// handle's cleanup detaches its resolver pool instead of joining it.
const CURLMOPT_QUICK_EXIT: curl_sys::CURLMoption = curl_sys::CURLOPTTYPE_LONG + 21;
/// End a transfer the stop signal stopped, and close its connection.
///
/// A name lookup can't be interrupted: `getaddrinfo` runs on curl's resolver
/// threads until the system resolver returns. Joining them would hold the
/// caller until then. So when the lookup is still pending, the cleanup
/// detaches them instead (wyrd-company/toha#81). They hold no connection to
/// the schema host and end by themselves. A libcurl that resolves on a thread
/// per transfer, such as 8.18, takes the transfer's option; one with a pool
/// owned by the multi handle, such as 8.22, takes the multi handle's option.
/// After the lookup the pool is joined, as without a stop, so no idle thread
/// is left. A libcurl that knows neither option joins, and the stop waits for
/// the lookup.
fn release_stopped(multi: curl::multi::Multi, handle: curl::multi::Easy2Handle<Body>) {
    // SAFETY: `handle` owns a live easy handle until `remove2` below, and
    // CURLOPT_QUICK_EXIT takes a long. A libcurl without the option returns
    // CURLE_UNKNOWN_OPTION and changes nothing.
    unsafe {
        curl_sys::curl_easy_setopt(handle.raw(), CURLOPT_QUICK_EXIT, 1 as std::ffi::c_long);
    }
    let Ok(easy) = multi.remove2(handle) else {
        return;
    };
    let lookup_pending = easy.namelookup_time().is_ok_and(|time| time.is_zero());
    if lookup_pending {
        // SAFETY: `multi` is the live multi handle that ran the transfer, and
        // CURLMOPT_QUICK_EXIT takes a long. A libcurl without the option
        // returns CURLM_UNKNOWN_OPTION and changes nothing.
        unsafe {
            curl_sys::curl_multi_setopt(multi.raw(), CURLMOPT_QUICK_EXIT, 1 as std::ffi::c_long);
        }
    }
    drop(easy);
    drop(multi);
}
/// Run `easy` to completion on a multi handle this thread drives, or return
/// [`STOPPED`] once the operation's stop signal fires. A stopped transfer is
/// ended with [`release_stopped`], which closes its connection.
fn perform_stoppable(
    easy: curl::easy::Easy2<Body>,
) -> Result<(Result<(), curl::Error>, Body), String> {
    let multi = curl::multi::Multi::new();
    let handle = multi.add2(easy).map_err(|e| e.to_string())?;
    loop {
        if crate::stop::requested() {
            release_stopped(multi, handle);
            return Err(STOPPED.into());
        }
        if multi.perform().map_err(|e| e.to_string())? == 0 {
            break;
        }
        multi.wait(&mut [], STOP_CHECK).map_err(|e| e.to_string())?;
    }
    let mut result = Ok(());
    multi.messages(|message| {
        if let Some(outcome) = message.result_for2(&handle) {
            result = outcome;
        }
    });
    let mut easy = multi.remove2(handle).map_err(|e| e.to_string())?;
    Ok((result, std::mem::take(easy.get_mut())))
}
