// ---
// relationships:
//   implements: architecture
// ---
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use std::collections::BTreeMap;

use serde_json::Value;

use crate::{
    fault::TemplateFault,
    hook::{HookResults, ResultSpec},
    inject::{
        Anchor, JsonFormat, JsonPath, MarkerStyle, PlannedEdit, PlannedJsonEdit, PlannedRegionEdit,
        RegionKey,
    },
    interview::Completed,
    jinja::{Expr, Tmpl, context_from_answers},
    staging::CanonicalTarget,
    template::{
        AnchorRule, Capture, HookNode, HookProgram, Id, InjectRule, InjectValue, RegionBody,
        Template,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPath(PathBuf);
impl TargetPath {
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.is_empty()
            || value.starts_with('/')
            || value.starts_with('\\')
            || value.contains('\\')
            || value.as_bytes().get(1) == Some(&b':')
        {
            return Err(format!("invalid target path: {value}"));
        }
        let mut parts = Vec::new();
        for part in value.split('/') {
            match part {
                ".git" => {
                    return Err("target path enters .git".into());
                }
                "" | "." => {}
                ".." => {
                    if parts.pop().is_none() {
                        return Err(format!("target path escapes target: {value}"));
                    }
                }
                value
                    if value.as_bytes().get(1) == Some(&b':')
                        && value.as_bytes()[0].is_ascii_alphabetic() =>
                {
                    return Err(format!("invalid target path: {value}"));
                }
                value => parts.push(value.to_owned()),
            }
        }
        if parts.is_empty() {
            return Err("empty target path".into());
        }
        Ok(Self(parts.iter().collect()))
    }
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}
impl fmt::Display for TargetPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = self.0.components();
        if let Some(first) = parts.next() {
            first.as_os_str().to_string_lossy().fmt(f)?;
        }
        for part in parts {
            write!(f, "/{}", part.as_os_str().to_string_lossy())?;
        }
        Ok(())
    }
}

pub(crate) fn has_symlink_component(
    target: &Path,
    path: &TargetPath,
) -> Result<bool, std::io::Error> {
    let mut current = target.to_owned();
    for component in path.as_path().components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Ok(true),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

#[derive(Debug)]
pub struct Plan {
    pub files: Vec<PlannedFile>,
    pub edits: Vec<PlannedEdit>,
    pub conflicts: Vec<TargetPath>,
    pub hooks: Vec<Planned<PlannedHook>>,
    pub before_apply: Option<String>,
    pub after_apply: Option<Planned<String>>,
    /// Every declared producer, seeded not-run before the apply loop so a reader
    /// of a skipped producer sees a bound `none`. Empty when no hook declares a
    /// result. Not part of the reviewable or persisted surface.
    pub(crate) result_seed: Vec<ResultSpec>,
}
/// A surface rendered at plan build (`Ready`) or retained to render in the apply
/// loop after its producer hooks run (`AfterHooks`). A result-free surface — the
/// overwhelming common case — is always `Ready` and identical to before this
/// feature.
#[derive(Debug)]
pub enum Planned<T> {
    Ready(T),
    /// Boxed so a `Vec<Planned<PlannedHook>>` stays small: the deferred case is
    /// rare and carries a large retained render program.
    AfterHooks(Box<Deferred>),
}
#[derive(Debug)]
pub struct PlannedFile {
    pub path: TargetPath,
    pub content: Content,
    pub source: PathBuf,
}
#[derive(Debug)]
pub enum Content {
    Rendered(String),
    Copied(PathBuf),
}
#[derive(Clone)]
pub struct PlannedHook {
    pub program: PlannedProgram,
    pub cwd: Option<TargetPath>,
    pub template_root: PathBuf,
    /// Opt-in result identity. `None` is a hook exactly as before this feature.
    pub id: Option<Id>,
    pub capture: Capture,
    pub allow_failure: bool,
    pub parse_json: bool,
    pub status_id: Option<Id>,
}
impl std::fmt::Debug for PlannedHook {
    /// Prints the pre-feature fields always and the new fields only when they are
    /// non-default, so a hook without a result is byte-identical to before.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("PlannedHook");
        s.field("program", &self.program)
            .field("cwd", &self.cwd)
            .field("template_root", &self.template_root);
        if self.id.is_some() {
            s.field("id", &self.id);
        }
        if self.capture != Capture::default() {
            s.field("capture", &self.capture);
        }
        if self.allow_failure {
            s.field("allow_failure", &self.allow_failure);
        }
        if self.parse_json {
            s.field("parse_json", &self.parse_json);
        }
        if self.status_id.is_some() {
            s.field("status_id", &self.status_id);
        }
        s.finish()
    }
}
#[derive(Debug, Clone)]
pub enum PlannedProgram {
    Run(Vec<String>),
    Script { path: PathBuf, args: Vec<String> },
}
impl PlannedHook {
    pub fn argv(&self) -> Vec<String> {
        match &self.program {
            PlannedProgram::Run(v) => v.clone(),
            PlannedProgram::Script { path, args } => std::iter::once(path.display().to_string())
                .chain(args.iter().cloned())
                .collect(),
        }
    }
}

/// A hook or after-apply message retained at plan build to render in the apply
/// loop, after its producer hooks have run and their results are known. Opaque:
/// it owns its compiled result-reading fields, one plan-time context, and its
/// eagerly-rendered parts (`run[0]`, the script path). A hook's `each` is already
/// expanded at build, so one `Deferred` is one planned invocation.
#[derive(Debug)]
pub struct Deferred(DeferredKind);
#[derive(Debug)]
enum DeferredKind {
    Hook(Box<DeferredHook>),
    Message(Box<DeferredMessage>),
}
#[derive(Debug)]
struct DeferredHook {
    label: String,
    template_root: PathBuf,
    context: BTreeMap<String, Value>,
    when: Option<Expr>,
    program: DeferredProgram,
    cwd: Option<Tmpl>,
    id: Option<Id>,
    capture: Capture,
    allow_failure: bool,
    parse_json: bool,
    status_id: Option<Id>,
}
#[derive(Debug)]
enum DeferredProgram {
    /// `run[0]` is rendered eagerly (it cannot read a result); the tail carries
    /// its original one-based index for error attribution.
    Run {
        head: String,
        tail: Vec<(usize, Tmpl)>,
    },
    Script {
        path: PathBuf,
        args: Vec<(usize, Tmpl)>,
    },
}
#[derive(Debug)]
struct DeferredMessage {
    template_root: PathBuf,
    context: BTreeMap<String, Value>,
    tmpl: Tmpl,
}
impl Deferred {
    /// The source-form argv and cwd of a deferred hook, for the dry-run/trust
    /// listing: `run[0]` shows its rendered value, later arguments and cwd show
    /// their template source with `{{ id.field }}` placeholders intact.
    pub fn hook_preview(&self) -> Option<(Vec<String>, Option<String>)> {
        let DeferredKind::Hook(hook) = &self.0 else {
            return None;
        };
        let argv = match &hook.program {
            DeferredProgram::Run { head, tail } => std::iter::once(head.clone())
                .chain(tail.iter().map(|(_, t)| t.source().to_owned()))
                .collect(),
            DeferredProgram::Script { path, args } => std::iter::once(path.display().to_string())
                .chain(args.iter().map(|(_, t)| t.source().to_owned()))
                .collect(),
        };
        Some((argv, hook.cwd.as_ref().map(|t| t.source().to_owned())))
    }
    /// Renders a deferred hook against the results known so far. Returns the one
    /// planned invocation, or none when its deferred `when` is false (it does not
    /// run; its result stays not-run).
    pub(crate) fn render_hooks(
        &self,
        results: &HookResults,
    ) -> Result<Vec<PlannedHook>, PlanError> {
        let DeferredKind::Hook(hook) = &self.0 else {
            return Ok(vec![]);
        };
        let mut ctx = hook.context.clone();
        results.project(&mut ctx);
        if let Some(when) = &hook.when {
            let active = when
                .eval(&ctx)
                .map_err(|e| hook.fault("when", when.source(), &e.to_string()))?
                .is_true();
            if !active {
                return Ok(vec![]);
            }
        }
        let program = match &hook.program {
            DeferredProgram::Run { head, tail } => {
                let mut argv = vec![head.clone()];
                for (i, tmpl) in tail {
                    argv.push(tmpl.render(&ctx).map_err(|e| {
                        hook.fault(&format!("run[{i}]"), tmpl.source(), &e.to_string())
                    })?);
                }
                PlannedProgram::Run(argv)
            }
            DeferredProgram::Script { path, args } => {
                let mut rendered = Vec::new();
                for (i, tmpl) in args {
                    rendered.push(tmpl.render(&ctx).map_err(|e| {
                        hook.fault(&format!("args[{i}]"), tmpl.source(), &e.to_string())
                    })?);
                }
                PlannedProgram::Script {
                    path: path.clone(),
                    args: rendered,
                }
            }
        };
        let cwd = match &hook.cwd {
            Some(tmpl) => {
                let text = tmpl
                    .render(&ctx)
                    .map_err(|e| hook.fault("cwd", tmpl.source(), &e.to_string()))?;
                (!text.is_empty())
                    .then(|| TargetPath::parse(&text))
                    .transpose()
                    .map_err(PlanError::Path)?
            }
            None => None,
        };
        Ok(vec![PlannedHook {
            program,
            cwd,
            template_root: hook.template_root.clone(),
            id: hook.id.clone(),
            capture: hook.capture,
            allow_failure: hook.allow_failure,
            parse_json: hook.parse_json,
            status_id: hook.status_id.clone(),
        }])
    }
    /// Renders a deferred after-apply message against the final results.
    pub(crate) fn render_message(
        &self,
        results: &HookResults,
    ) -> Result<Option<String>, PlanError> {
        let DeferredKind::Message(message) = &self.0 else {
            return Ok(None);
        };
        let mut ctx = message.context.clone();
        results.project(&mut ctx);
        let text = message.tmpl.render(&ctx).map_err(|e| PlanError::Render {
            path: message.template_root.clone(),
            message: e.to_string(),
        })?;
        Ok((!text.trim().is_empty()).then_some(text))
    }
}
impl DeferredHook {
    fn fault(&self, field: &str, source: &str, message: &str) -> PlanError {
        PlanError::Render {
            path: self.template_root.clone(),
            message: format!(
                "template error in {}.{field} `{source}`: {message}",
                self.label
            ),
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("{0}")]
    Path(String),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {message}")]
    Render { path: PathBuf, message: String },
    #[error("duplicate target {target}: {first} and {second}")]
    Duplicate {
        target: TargetPath,
        first: PathBuf,
        second: PathBuf,
    },
    #[error("plan target does not match the completed interview's context target")]
    ContextTarget,
    #[error("{field}: {message}")]
    Inject { field: String, message: String },
}
fn rendered(text: &str, ctx: &impl serde::Serialize, source: &Path) -> Result<String, PlanError> {
    Tmpl::compile(text.into())
        .and_then(|t| t.render(ctx))
        .map_err(|e| PlanError::Render {
            path: source.into(),
            message: e.to_string(),
        })
}
fn read_render(path: &Path, ctx: &impl serde::Serialize) -> Result<String, PlanError> {
    let bytes = fs::read(path).map_err(|source| PlanError::Io {
        path: path.into(),
        source,
    })?;
    let text = String::from_utf8(bytes).map_err(|_| PlanError::Render {
        path: path.into(),
        message: "file is not UTF-8; add it to static".into(),
    })?;
    rendered(&text, ctx, path)
}
impl Plan {
    pub fn build(
        template: &Template,
        completed: &Completed,
        target: &CanonicalTarget,
    ) -> Result<Self, PlanError> {
        // The plan renders under the completed interview's context. A current
        // context must carry the target the plan is applied to; a legacy context
        // carries the same target restored during replay; a context-free start
        // (`Resolution::start`) projects nothing and imposes no target check.
        let mut ctx = context_from_answers(&completed.answers, &template.data, &completed.now);
        if let Some(context) = completed.context() {
            if context.target() != target {
                return Err(PlanError::ContextTarget);
            }
            context.project(&mut ctx);
        }
        let mut plan = Self {
            files: vec![],
            edits: vec![],
            conflicts: vec![],
            hooks: vec![],
            before_apply: None,
            after_apply: None,
            result_seed: template.result_specs(),
        };
        // Render the retained source-tree program; it is not reopened here.
        for entry in &template.render_program {
            let mut parts = Vec::new();
            let mut skip = false;
            for segment in &entry.segments {
                let value = segment.render(&ctx).map_err(|e| PlanError::Render {
                    path: entry.source.clone(),
                    message: TemplateFault {
                        field: "path".into(),
                        expression: Some(segment.source().into()),
                        message: e.to_string(),
                    }
                    .to_string(),
                })?;
                if value.is_empty() {
                    skip = true;
                    break;
                }
                parts.push(value);
            }
            if skip {
                continue;
            }
            let path = TargetPath::parse(&parts.join("/")).map_err(PlanError::Path)?;
            let content = match &entry.body {
                crate::template::SourceBody::Static => Content::Copied(entry.source.clone()),
                crate::template::SourceBody::Rendered(body) => {
                    Content::Rendered(body.render(&ctx).map_err(|e| PlanError::Render {
                        path: entry.source.clone(),
                        message: e.to_string(),
                    })?)
                }
                crate::template::SourceBody::NonUtf8 => {
                    return Err(PlanError::Render {
                        path: entry.source.clone(),
                        message: "file is not UTF-8; add it to static".into(),
                    });
                }
            };
            plan.add(path, content, entry.source.clone(), target.as_path())?;
        }
        for (i, rule) in template.files.iter().enumerate() {
            let origin = template.root.join(&rule.source);
            if let Some(when) = &rule.when {
                let enabled = when.eval(&ctx).map_err(|e| PlanError::Render {
                    path: origin.clone(),
                    message: TemplateFault {
                        field: format!("files[{i}].when"),
                        expression: Some(when.source().into()),
                        message: e.to_string(),
                    }
                    .to_string(),
                })?;
                if !enabled.is_true() {
                    continue;
                }
            }
            let contexts = rule.each.contexts(&ctx).map_err(|e| PlanError::Render {
                path: origin.clone(),
                message: TemplateFault {
                    field: format!("files[{i}].each"),
                    expression: Some(rule.each.expr.source().into()),
                    message: e,
                }
                .to_string(),
            })?;
            for local in contexts {
                let path_text = rule.path.render(&local).map_err(|e| PlanError::Render {
                    path: origin.clone(),
                    message: TemplateFault {
                        field: format!("files[{i}].path"),
                        expression: Some(rule.path.source().into()),
                        message: e.to_string(),
                    }
                    .to_string(),
                })?;
                let path = TargetPath::parse(&path_text).map_err(PlanError::Path)?;
                let rendered = match &rule.body {
                    Some(body) => body.render(&local).map_err(|e| PlanError::Render {
                        path: origin.clone(),
                        message: e.to_string(),
                    })?,
                    // A non-UTF-8 rule body was tolerated at load; surface it now.
                    None => read_render(&origin, &local)?,
                };
                let content = Content::Rendered(rendered);
                plan.add(path, content, origin.clone(), target.as_path())?;
            }
        }
        // Content injection: render each rule to a bounded edit. Planning is pure
        // over the template and answers and reads no target bytes.
        for (i, rule) in template.inject.iter().enumerate() {
            plan.add_edit(rule, i, &ctx)?;
        }
        // Interview hooks were rendered during the interview; they may produce a
        // result but never read one, so they are always ready.
        for hook in &completed.hooks {
            plan.hooks.push(Planned::Ready(plan_hook(hook, template)?));
        }
        // A top-level hook that reads a result is retained and rendered in the
        // apply loop; one that does not is rendered here exactly as before. `each`
        // is expanded eagerly in both paths, so the slot count is fixed at build.
        for (i, hook) in template.hooks.iter().enumerate() {
            let label = format!("hooks[{i}]");
            if template.reads_results(hook) {
                for context in each_contexts(hook, &ctx, &label, template)? {
                    plan.hooks
                        .push(Planned::AfterHooks(Box::new(build_deferred_hook(
                            hook, &label, context, template,
                        )?)));
                }
                continue;
            }
            if let Some(when) = &hook.when
                && !when
                    .eval(&ctx)
                    .map_err(|e| PlanError::Render {
                        path: template.root.clone(),
                        message: format!(
                            "template error in hooks[{i}].when `{}`: {e}",
                            when.source()
                        ),
                    })?
                    .is_true()
            {
                continue;
            }
            let rendered =
                crate::interview::render_hooks(hook, &ctx).map_err(|f| PlanError::Render {
                    path: template.root.clone(),
                    message: format!(
                        "template error in hooks[{i}].{} `{}`: {}",
                        f.field, f.source, f.message
                    ),
                })?;
            for hook in &rendered {
                plan.hooks.push(Planned::Ready(plan_hook(hook, template)?));
            }
        }
        // before-apply never reads a result (it runs before hooks); after-apply
        // may, so it is retained when it does.
        if let Some(source) = &template.messages.before_apply {
            let text = source.render(&ctx).map_err(|e| PlanError::Render {
                path: template.root.clone(),
                message: e.to_string(),
            })?;
            if !text.trim().is_empty() {
                plan.before_apply = Some(text);
            }
        }
        if let Some(source) = &template.messages.after_apply {
            if template.references_result(source.references()) {
                plan.after_apply = Some(Planned::AfterHooks(Box::new(Deferred(
                    DeferredKind::Message(Box::new(DeferredMessage {
                        template_root: template.root.clone(),
                        context: ctx.clone(),
                        tmpl: recompile_tmpl(source),
                    })),
                ))));
            } else {
                let text = source.render(&ctx).map_err(|e| PlanError::Render {
                    path: template.root.clone(),
                    message: e.to_string(),
                })?;
                if !text.trim().is_empty() {
                    plan.after_apply = Some(Planned::Ready(text));
                }
            }
        }
        Ok(plan)
    }
    fn add(
        &mut self,
        path: TargetPath,
        content: Content,
        source: PathBuf,
        target: &Path,
    ) -> Result<(), PlanError> {
        if let Some(first) = self.files.iter().find(|f| f.path == path) {
            return Err(PlanError::Duplicate {
                target: path,
                first: first.source.clone(),
                second: source,
            });
        }
        let destination = target.join(path.as_path());
        if has_symlink_component(target, &path).map_err(|source| PlanError::Io {
            path: destination.clone(),
            source,
        })? {
            return Err(PlanError::Path(format!(
                "target path contains symlink: {path}"
            )));
        }
        if destination.exists() {
            self.conflicts.push(path.clone());
        }
        self.files.push(PlannedFile {
            path,
            content,
            source,
        });
        Ok(())
    }
    /// Renders one `inject` rule to a bounded edit, resolving the marker style or
    /// JSON format and checking the per-target uniqueness the edit requires.
    fn add_edit(
        &mut self,
        rule: &InjectRule,
        index: usize,
        ctx: &std::collections::BTreeMap<String, Value>,
    ) -> Result<(), PlanError> {
        let field = format!("inject[{index}]");
        match rule {
            InjectRule::Region(region) => {
                if let Some(when) = &region.when
                    && !inject_when(when, &field, ctx)?
                {
                    return Ok(());
                }
                let path = inject_target(&region.into, &field, ctx)?;
                if json_format(&path).is_some() {
                    return Err(PlanError::Inject {
                        field,
                        message: format!(
                            "region rule targets a JSON-family file `{path}`; use struct: for a typed value or an existing files: rule for the whole file"
                        ),
                    });
                }
                let marker = match region.marker.clone().or_else(|| MarkerStyle::infer(&path)) {
                    Some(marker) => marker,
                    None => {
                        return Err(PlanError::Inject {
                            field,
                            message: format!(
                                "cannot infer a comment style for `{path}`; add a marker: override"
                            ),
                        });
                    }
                };
                let body = render_region_body(&region.body, &field, ctx)?;
                let anchor = match &region.anchor {
                    Some(anchor) => Some(render_anchor(anchor, &field, ctx)?),
                    None => None,
                };
                let source = match &region.body {
                    RegionBody::Source { path, .. } => Some(path.clone()),
                    RegionBody::Content(_) => None,
                };
                let edit = PlannedRegionEdit {
                    path,
                    region: region.region.clone(),
                    body,
                    marker,
                    anchor,
                    create: region.create,
                    source,
                };
                self.check_region_unique(&edit, &field)?;
                self.edits.push(PlannedEdit::Region(edit));
            }
            InjectRule::Struct(rule) => {
                if let Some(when) = &rule.when
                    && !inject_when(when, &field, ctx)?
                {
                    return Ok(());
                }
                let path = inject_target(&rule.into, &field, ctx)?;
                let Some(format) = json_format(&path) else {
                    return Err(PlanError::Inject {
                        field,
                        message: format!(
                            "struct rule targets `{path}`, which is not a .json, .jsonc, or .json5 file"
                        ),
                    });
                };
                let desired = render_inject_value(&rule.value, &field, ctx)?;
                let edit = PlannedJsonEdit {
                    path,
                    json_path: rule.path.clone(),
                    desired,
                    format,
                    create: rule.create,
                };
                self.check_json_unique(&edit, &field)?;
                self.edits.push(PlannedEdit::JsonValue(edit));
            }
        }
        Ok(())
    }
    /// Rejects a second region rule with the same key on one target.
    fn check_region_unique(&self, edit: &PlannedRegionEdit, field: &str) -> Result<(), PlanError> {
        for existing in &self.edits {
            if let PlannedEdit::Region(other) = existing
                && other.path == edit.path
                && other.region == edit.region
            {
                return Err(PlanError::Inject {
                    field: field.to_owned(),
                    message: format!("duplicate region `{}` on `{}`", edit.region, edit.path),
                });
            }
        }
        Ok(())
    }
    /// Rejects a duplicate or ancestor/descendant JSON path on one target.
    fn check_json_unique(&self, edit: &PlannedJsonEdit, field: &str) -> Result<(), PlanError> {
        for existing in &self.edits {
            if let PlannedEdit::JsonValue(other) = existing
                && other.path == edit.path
                && other.json_path.overlaps(&edit.json_path)
            {
                return Err(PlanError::Inject {
                    field: field.to_owned(),
                    message: format!(
                        "JSON paths `{}` and `{}` overlap on `{}`",
                        other.json_path, edit.json_path, edit.path
                    ),
                });
            }
        }
        Ok(())
    }
    /// The derived ownership view over every planned mutation: a whole file, a
    /// region, or a typed JSON value. Project update consumes this.
    pub fn mutations(&self) -> impl Iterator<Item = FileMutation<'_>> {
        self.files
            .iter()
            .map(|file| FileMutation::Whole { path: &file.path })
            .chain(self.edits.iter().map(|edit| match edit {
                PlannedEdit::Region(region) => FileMutation::Region {
                    path: &region.path,
                    region: &region.region,
                },
                PlannedEdit::JsonValue(json) => FileMutation::JsonValue {
                    path: &json.path,
                    json_path: &json.json_path,
                },
            }))
    }
}

/// The ownership identity of one planned mutation, derived from the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMutation<'a> {
    /// Toha owns and replaces the complete file.
    Whole { path: &'a TargetPath },
    /// Toha owns the marker pair and enclosed byte span.
    Region {
        path: &'a TargetPath,
        region: &'a RegionKey,
    },
    /// Toha owns the typed value at the path and converges it on replay.
    JsonValue {
        path: &'a TargetPath,
        json_path: &'a JsonPath,
    },
}

/// The JSON-family format for a target extension, or `None` for any other file.
fn json_format(path: &TargetPath) -> Option<JsonFormat> {
    match path
        .as_path()
        .extension()
        .and_then(|ext| ext.to_str())?
        .to_ascii_lowercase()
        .as_str()
    {
        "json" => Some(JsonFormat::Json),
        "jsonc" => Some(JsonFormat::Jsonc),
        "json5" => Some(JsonFormat::Json5),
        _ => None,
    }
}

fn inject_when(
    when: &Expr,
    field: &str,
    ctx: &std::collections::BTreeMap<String, Value>,
) -> Result<bool, PlanError> {
    Ok(when
        .eval(ctx)
        .map_err(|e| PlanError::Inject {
            field: field.to_owned(),
            message: format!("template error in {field}.when `{}`: {e}", when.source()),
        })?
        .is_true())
}

fn inject_target(
    into: &Tmpl,
    field: &str,
    ctx: &std::collections::BTreeMap<String, Value>,
) -> Result<TargetPath, PlanError> {
    let text = into.render(ctx).map_err(|e| PlanError::Inject {
        field: field.to_owned(),
        message: format!("template error in {field}.into `{}`: {e}", into.source()),
    })?;
    TargetPath::parse(&text).map_err(PlanError::Path)
}

fn render_region_body(
    body: &RegionBody,
    field: &str,
    ctx: &std::collections::BTreeMap<String, Value>,
) -> Result<String, PlanError> {
    match body {
        RegionBody::Content(tmpl) => tmpl.render(ctx).map_err(|e| PlanError::Inject {
            field: field.to_owned(),
            message: format!("template error in {field}.content: {e}"),
        }),
        RegionBody::Source { body, .. } => body.render(ctx).map_err(|e| PlanError::Inject {
            field: field.to_owned(),
            message: format!("template error in {field}.source: {e}"),
        }),
    }
}

fn render_anchor(
    anchor: &AnchorRule,
    field: &str,
    ctx: &std::collections::BTreeMap<String, Value>,
) -> Result<Anchor, PlanError> {
    let after = anchor.after.render(ctx).map_err(|e| PlanError::Inject {
        field: field.to_owned(),
        message: format!("template error in {field}.anchor.after: {e}"),
    })?;
    Ok(Anchor {
        after,
        occurrence: anchor.occurrence,
    })
}

/// Renders a structured value's string leaves to a `serde_json::Value`, keeping
/// every other JSON type.
fn render_inject_value(
    value: &InjectValue,
    field: &str,
    ctx: &std::collections::BTreeMap<String, Value>,
) -> Result<Value, PlanError> {
    match value {
        InjectValue::Template(tmpl) => {
            let text = tmpl.render(ctx).map_err(|e| PlanError::Inject {
                field: field.to_owned(),
                message: format!("template error in {field}.struct.value: {e}"),
            })?;
            Ok(Value::String(text))
        }
        InjectValue::Literal(literal) => Ok(literal.clone()),
        InjectValue::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(render_inject_value(item, field, ctx)?);
            }
            Ok(Value::Array(out))
        }
        InjectValue::Object(pairs) => {
            let mut map = serde_json::Map::new();
            for (key, item) in pairs {
                map.insert(key.clone(), render_inject_value(item, field, ctx)?);
            }
            Ok(Value::Object(map))
        }
    }
}
fn plan_hook(
    hook: &crate::interview::RenderedHook,
    template: &Template,
) -> Result<PlannedHook, PlanError> {
    use crate::interview::RenderedProgram;
    let program = match &hook.program {
        RenderedProgram::Run(v) => PlannedProgram::Run(v.clone()),
        RenderedProgram::Script { path, args } => PlannedProgram::Script {
            path: template.root.join(path),
            args: args.clone(),
        },
    };
    let cwd = hook
        .cwd
        .as_deref()
        .filter(|v| !v.is_empty())
        .map(TargetPath::parse)
        .transpose()
        .map_err(PlanError::Path)?;
    Ok(PlannedHook {
        program,
        cwd,
        template_root: template.root.clone(),
        id: hook.id.clone(),
        capture: hook.capture,
        allow_failure: hook.allow_failure,
        parse_json: hook.parse_json,
        status_id: hook.status_id.clone(),
    })
}

/// The plan-time contexts a top-level hook expands to: one per `each` item, or a
/// single copy of `ctx` without `each`. `each` never reads a result, so it is
/// evaluated eagerly here even for a deferred hook.
fn each_contexts(
    hook: &HookNode,
    ctx: &BTreeMap<String, Value>,
    label: &str,
    template: &Template,
) -> Result<Vec<BTreeMap<String, Value>>, PlanError> {
    match &hook.each {
        None => Ok(vec![ctx.clone()]),
        Some(each) => each.contexts(ctx).map_err(|message| PlanError::Render {
            path: template.root.clone(),
            message: format!(
                "template error in {label}.each `{}`: {message}",
                each.expr.source()
            ),
        }),
    }
}

/// Builds one deferred invocation of a result-reading top-level hook: `run[0]`
/// (or the script path) is rendered eagerly against `context`; every result-
/// reading field is retained compiled to render in the apply loop.
fn build_deferred_hook(
    hook: &HookNode,
    label: &str,
    context: BTreeMap<String, Value>,
    template: &Template,
) -> Result<Deferred, PlanError> {
    let render_head = |tmpl: &Tmpl, field: &str| {
        tmpl.render(&context).map_err(|e| PlanError::Render {
            path: template.root.clone(),
            message: format!("template error in {label}.{field} `{}`: {e}", tmpl.source()),
        })
    };
    let program = match &hook.command.program {
        HookProgram::Run(args) => DeferredProgram::Run {
            head: render_head(&args[0], "run[0]")?,
            tail: args[1..]
                .iter()
                .enumerate()
                .map(|(k, t)| (k + 1, recompile_tmpl(t)))
                .collect(),
        },
        HookProgram::Script { path, args } => DeferredProgram::Script {
            path: template.root.join(path),
            args: args
                .iter()
                .enumerate()
                .map(|(k, t)| (k, recompile_tmpl(t)))
                .collect(),
        },
    };
    Ok(Deferred(DeferredKind::Hook(Box::new(DeferredHook {
        label: label.to_owned(),
        template_root: template.root.clone(),
        context,
        when: hook.when.as_ref().map(recompile_expr),
        program,
        cwd: hook.command.cwd.as_ref().map(recompile_tmpl),
        id: hook.id.clone(),
        capture: hook.capture,
        allow_failure: hook.allow_failure,
        parse_json: hook.parse_json,
        status_id: hook.status_id.clone(),
    }))))
}

/// Rebuilds an owned `Tmpl` from an already-validated one; used to move the
/// result-reading fields into a `Deferred` (the compiled `Tmpl` is not `Clone`).
/// The source compiled once at load, so recompilation cannot fail.
fn recompile_tmpl(tmpl: &Tmpl) -> Tmpl {
    Tmpl::compile(tmpl.source().to_owned()).expect("previously compiled template")
}
fn recompile_expr(expr: &Expr) -> Expr {
    Expr::compile(expr.source().to_owned()).expect("previously compiled expression")
}
#[cfg(test)]
mod tests {
    use super::TargetPath;
    #[test]
    fn target_path() {
        assert_eq!(TargetPath::parse("a/./b/../c").unwrap().to_string(), "a/c");
        for bad in [
            "",
            ".",
            "../a",
            "a/../../b",
            "/a",
            "a/.git/b",
            "a\\b",
            "C:/a",
            "a/C:/b",
            "//server/share",
            "\\\\server\\share",
        ] {
            assert!(TargetPath::parse(bad).is_err(), "{bad}");
        }
    }
}
