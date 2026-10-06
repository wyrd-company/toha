//! Throwaway spike probe: render foreign-template files with an environment
//! configured like Toha's (`src/jinja.rs`) and classify each failure.
//! Input: JSON on stdin `{ "context": {...}, "files": [{"name": "...", "source": "..."}] }`.
//! Output: one JSON line per file `{ "name", "stage", "kind", "detail" }`.
use minijinja::{Environment, ErrorKind, Value};
use std::io::Read;

fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_keep_trailing_newline(true);
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
    // Toha's case filters, `toyaml`, `now`, and `dateformat`: present by name so
    // a foreign template using the same name is not misreported as unknown.
    for name in ["kebab", "snake", "camel", "pascal", "constant", "title", "toyaml", "dateformat"] {
        env.add_filter(name, |v: Value| v);
    }
    env.add_function("now", || Value::from("2026-01-01T00:00:00Z"));
    #[cfg(feature = "pycompat")]
    env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
    env
}

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&input).unwrap();
    let ctx = Value::from_serialize(&doc["context"]);
    let mut env = environment();
    let files = doc["files"].as_array().unwrap();
    for f in files {
        let name = f["name"].as_str().unwrap().to_owned();
        let source = f["source"].as_str().unwrap().to_owned();
        let surface = f["surface"].as_bool().unwrap_or(true);
        if let Err(e) = env.add_template_owned(name.clone(), source) {
            if surface {
                report(&name, "compile", e.kind(), &e.to_string());
            }
        }
    }
    for f in files.iter().filter(|f| f["surface"].as_bool().unwrap_or(true)) {
        let name = f["name"].as_str().unwrap();
        let Ok(t) = env.get_template(name) else { continue };
        match t.render(&ctx) {
            Ok(body) => {
                // Render the root-relative path as one template so the caller
                // can find the native tool's output for a byte comparison.
                let path = env.render_str(name, &ctx).ok();
                println!("{}", serde_json::json!({ "name": name, "stage": "render", "kind": "ok", "detail": "ok", "output": body, "path": path }));
            }
            Err(e) => report(name, "render", e.kind(), &e.to_string()),
        }
    }
}

fn report(name: &str, stage: &str, kind: ErrorKind, detail: &str) {
    let kind = if detail == "ok" { "ok".to_owned() } else { format!("{kind:?}") };
    let detail: String = detail.lines().next().unwrap_or("").chars().take(200).collect();
    println!("{}", serde_json::json!({ "name": name, "stage": stage, "kind": kind, "detail": detail }));
}
