// ---
// relationships:
//   implements: template-format
// ---
//! Explicit coordination events for recorded template applications.
use crate::{PlannedHook, applied::Application, hook::HookRunner, template::Template};
use std::path::{Path, PathBuf};

/// A prepared application handler invocation. Preparing never executes scripts.
#[derive(Debug)]
pub struct EventHandler {
    pub target: PathBuf,
    pub hook: PlannedHook,
}

/// Render a named event's handlers using static data and the application's
/// recorded raw answers and context. Does not run the interview or apply hooks.
/// The caller must establish trust before execution, including script bytes.
pub fn prepare(
    template: &Template,
    application: &Application,
    event: &str,
    repository: &Path,
) -> Result<Vec<EventHandler>, String> {
    let Some(handlers) = template.events.get(event) else {
        return Ok(Vec::new());
    };
    let repository = repository.canonicalize().map_err(|e| e.to_string())?;
    let target = repository
        .join(application.target().as_str())
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !target.starts_with(&repository) {
        return Err("event application target escapes the repository".into());
    }
    let mut context = std::collections::BTreeMap::new();
    for (id, value) in &template.data {
        context.insert(id.as_str().to_owned(), value.clone());
    }
    context.extend(application.answers.clone());
    context.extend(application.context.clone());
    let mut planned = Vec::new();
    for hook in handlers {
        if let Some(when) = &hook.when
            && !when.eval(&context).map_err(|e| e.to_string())?.is_true()
        {
            continue;
        }
        for mut rendered in crate::interview::render_hooks(hook, &context)
            .map_err(|e| format!("event {event}: {}: {}", e.field, e.message))?
        {
            if rendered.cwd.as_deref() == Some(".") {
                rendered.cwd = None;
            }
            let hook = crate::plan::plan_hook(&rendered, template).map_err(|e| e.to_string())?;
            let cwd = target
                .join(
                    hook.cwd
                        .as_ref()
                        .map(|p| p.as_path())
                        .unwrap_or(Path::new("")),
                )
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if !cwd.starts_with(&target) {
                return Err(
                    "event handler working directory escapes its application target".into(),
                );
            }
            planned.push(EventHandler {
                target: target.clone(),
                hook,
            });
        }
    }
    Ok(planned)
}

/// Execute a fully prepared, caller-authorized dispatch in order. Stops at the
/// first failure; prior script effects remain, as with ordinary hooks.
pub fn dispatch(handlers: &[EventHandler], runner: &dyn HookRunner) -> Result<(), String> {
    for handler in handlers {
        let outcome = runner
            .run(&handler.hook, &handler.target)
            .map_err(|e| e.to_string())?;
        if !outcome.success {
            return Err(format!(
                "event handler failed with exit code {:?}",
                outcome.code
            ));
        }
    }
    Ok(())
}

/// Explicit execution choices, independent of a command driver.
#[derive(Debug, Default, Clone, Copy)]
pub struct EventOptions {
    pub trust: bool,
    pub dry_run: bool,
}

#[derive(Debug)]
pub struct EventReport {
    pub handler_count: usize,
    pub warnings: Vec<String>,
}

/// Discover the applied record, resolve recorded template revisions, prepare
/// every participating handler and trust decision, then execute the dispatch.
/// A dry run performs discovery and preparation without executing scripts.
pub fn fire(
    name: &str,
    path: &Path,
    options: EventOptions,
    dirs: &crate::resolve::Paths,
    cwd: &Path,
    runner: &dyn HookRunner,
) -> Result<EventReport, String> {
    let target = crate::staging::canonical_target(path).map_err(|e| e.to_string())?;
    let project = crate::applied::Project::open(&target)
        .map_err(|e| e.to_string())?
        .ok_or("event requires a git repository with an applied record")?;
    let root = project.record_path().map_err(|e| e.to_string())?;
    // The record override may be nested, so derive the root from the invocation
    // target's repository-relative identity, not the record's parent directory.
    let mut repository = target.as_path().to_path_buf();
    if !project.target().is_root() {
        for _ in project.target().as_str().split('/') {
            repository.pop();
        }
    }
    if !root.exists() {
        return Err("event requires an applied record; use toha apply first".into());
    }
    let record = project.read_record().map_err(|e| e.to_string())?;
    let (config, registry, warnings) =
        crate::resolve::load_context(dirs, cwd).map_err(|e| format!("{e:?}"))?;
    let mut handlers = Vec::new();
    for app in record.applications() {
        let resolved = crate::resolve::resume_application_template(
            app.template(),
            app.revision(),
            &config,
            &registry,
            dirs,
            cwd,
        )
        .map_err(|e| format!("{e:?}"))?;
        let template = crate::Template::load(&resolved.folder).map_err(|e| e.to_string())?;
        if !template.events.contains_key(name) {
            continue;
        }
        let prepared = prepare(&template, app, name, &repository)?;
        if prepared.is_empty() {
            continue;
        }
        let surface = crate::HookSurface::of(&template).map_err(|e| e.to_string())?;
        if !options.dry_run
            && !options.trust
            && crate::evaluate_trust(resolved.approval.as_ref(), &surface.digest())
                != crate::Trust::Trusted
        {
            return Err(format!(
                "event handlers for {} require trust; use toha event {name} --trust",
                app.template()
            ));
        }
        handlers.extend(prepared);
    }
    if !options.dry_run {
        dispatch(&handlers, runner)?;
    }
    Ok(EventReport {
        handler_count: handlers.len(),
        warnings,
    })
}
