// ---
// relationships:
//   implements:
//     - interview-protocol
//     - project-updates
// ---
//! Shared application preparation, owned interview replay and plan/application boundaries.
use crate::{
    AnswerError, Applied, ApplyOptions, Completed, Interview, Plan, RawAnswers, Template,
    context::{
        EnvironmentDecision, ExecutionFacts, FixedEnvironmentSource, HostFacts, InvocationContext,
        SelectedTemplate,
    },
    resolve::ResolvedTemplate,
    staging::{CanonicalTarget, StagedRecord, Store},
};
use std::path::{Path, PathBuf};
pub fn admit_parts(
    template: &mut Template,
    mut parts: crate::parts::Selection,
    prior: Option<&serde_json::Value>,
    supplied: &crate::RawAnswers,
) -> Result<(), String> {
    parts.admit(template, supplied)?;
    if let Some(prior) = prior {
        parts.base = prior
            .get("parts")
            .and_then(|v| serde_json::from_value(v.clone()).ok());
    }
    template.part_selection = parts;
    Ok(())
}
pub fn admitted_documents(template: &Template) -> indexmap::IndexMap<String, serde_json::Value> {
    template
        .document_question_ids()
        .into_iter()
        .filter_map(|id| {
            let crate::QuestionKind::Document { contract } = &template.question(&id)?.kind else {
                return None;
            };
            contract
                .admitted()
                .cloned()
                .map(|value| (id.to_string(), value))
        })
        .collect()
}
pub fn save_document_answers(template: &Template, saved: &mut StagedRecord) {
    let mut values = admitted_documents(template);
    for submission in &mut saved.submissions {
        for (id, value) in submission {
            if let Some(new) = values.shift_remove(id) {
                *value = new;
            }
        }
    }
    if !values.is_empty() {
        saved.submissions.push(values);
    }
}
pub fn recorded_answers(saved: &StagedRecord) -> crate::RawAnswers {
    saved
        .submissions
        .iter()
        .flat_map(|s| s.iter())
        .filter_map(|(id, value)| {
            crate::Id::parse(id)
                .ok()
                .map(|id| (id, crate::RawAnswer(value.clone())))
        })
        .collect()
}
pub fn prepare_staged_documents(
    template: &mut Template,
    target: &Path,
    input: &[PathBuf],
    incoming: &crate::RawAnswers,
    saved: &mut StagedRecord,
) -> Result<(), crate::document::PrepareError> {
    crate::document::prepare_staged(template, target, input, &recorded_answers(saved), incoming)?;
    save_document_answers(template, saved);
    Ok(())
}
pub fn effective_trust(trust: bool, resolved: &ResolvedTemplate, template: &Template) -> bool {
    if trust {
        return true;
    }
    match crate::HookSurface::of(template) {
        Ok(surface) => matches!(
            crate::evaluate_trust(resolved.approval.as_ref(), &surface.digest()),
            crate::Trust::Trusted
        ),
        Err(_) => false,
    }
}

pub fn invocation_context(
    target: &CanonicalTarget,
    resolved: &ResolvedTemplate,
    template: &Template,
    decision: EnvironmentDecision,
    host: HostFacts,
    execution: ExecutionFacts,
    source: &mut impl FixedEnvironmentSource,
) -> Result<InvocationContext, String> {
    let environment = template
        .admit_environment(decision, source)
        .map_err(|e| crate::staging::stage_admission_error(e).to_string())?;
    let selected = SelectedTemplate::new(
        resolved.formal_name.clone(),
        template.name.clone(),
        resolved.aliases.clone(),
        resolved.source.clone(),
    );
    InvocationContext::new(target.clone(), selected, host, execution, environment)
        .map_err(|e| e.to_string())
}
/// Admit parts and freeze the already admitted documents and optional seed in an owned record.
/// Drivers can perform their input/prompt admission first without duplicating replay state construction.
#[allow(clippy::too_many_arguments)]
pub fn prepare_record(
    template: &mut Template,
    resolved: &ResolvedTemplate,
    context: InvocationContext,
    now: &jiff::Zoned,
    parts: crate::parts::Selection,
    answers: &RawAnswers,
    seed: Option<&crate::application_seed::LikeSeed>,
) -> Result<StagedRecord, String> {
    admit_parts(template, parts, seed.map(|s| &s.entry), answers)?;
    let mut record = StagedRecord::new_with_context(
        context,
        resolved.commit.clone(),
        resolved.named,
        now.to_string(),
        vec![],
    );
    record.part_selection = template.part_selection.clone();
    if let Some(seed) = seed {
        record.seed = Some(seed.id.clone());
        record.seed_entry = Some(seed.entry.clone());
    }
    save_document_answers(template, &mut record);
    Ok(record)
}

#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    #[error("staged --like has no copied application; abort and stage again")]
    MissingCopy,
    #[error("copied --like application has another source; abort and stage again")]
    WrongSource,
    #[error("copied --like application has another target; abort and stage again")]
    WrongTarget,
    #[error(transparent)]
    Record(#[from] crate::applied::RecordError),
}

pub fn pinned_seed(
    saved: &StagedRecord,
    source: &str,
    _target: &CanonicalTarget,
) -> Result<Option<crate::interview::ApplicationSeed>, SeedError> {
    let Some(pin) = &saved.seed_entry else {
        return if saved.seed.is_some() {
            Err(SeedError::MissingCopy)
        } else {
            Ok(None)
        };
    };
    let record = serde_json::json!({"$schema":crate::applied::SCHEMA_ID,"applications":[pin]});
    let record = crate::applied::AppliedRecord::parse(&serde_json::to_vec(&record).unwrap())?;
    let application = &record.applications()[0];
    if application.source() != source {
        return Err(SeedError::WrongSource);
    }
    if saved.seed.as_deref() != Some(application.target().as_str()) {
        return Err(SeedError::WrongTarget);
    }
    Ok(Some(crate::application_seed::seed(application).engine))
}

#[derive(Debug, thiserror::Error)]
pub enum OperationError {
    #[error("{0}")]
    Input(String),
    #[error("{0}")]
    Seed(#[from] SeedError),
    #[error("{0}")]
    Resolve(#[from] crate::resolve::ResolveError),
    #[error("{0}")]
    Load(#[from] crate::LoadError),
    #[error("{0}")]
    Staging(#[from] crate::staging::StagingError),
    #[error("{0}")]
    Document(#[from] crate::document::PrepareError),
    #[error("{0}")]
    Evaluation(#[from] crate::EvalError),
    #[error("{0}")]
    Plan(#[from] crate::PlanError),
    #[error("{0}")]
    Record(#[from] crate::applied::RecordError),
    #[error("{0}")]
    Requirements(#[from] crate::MissingRequirements),
    #[error("{0}")]
    Apply(Box<crate::ApplyError>),
}

impl From<crate::ApplyError> for OperationError {
    fn from(error: crate::ApplyError) -> Self {
        Self::Apply(Box::new(error))
    }
}

/// An owned response: question errors retain their question ids. No Pending escapes a request.
#[allow(clippy::large_enum_variant)]
pub enum SessionState {
    Asking {
        document: serde_json::Value,
        rejections: crate::Rejections,
    },
    Completed(Completed),
    Ended(crate::Ended),
}

/// A loaded request. Persist/return `record`, then drop this value between requests.
/// Reconstruct with `resume` so the template revision, context and admitted documents stay pinned.
pub struct Session {
    pub template: Template,
    pub resolved: ResolvedTemplate,
    pub record: StagedRecord,
    pub target: CanonicalTarget,
    config: crate::config::Config,
}

impl Session {
    /// Start a new in-memory application. Host inputs and storage admission belong to the caller.
    /// The same record can be persisted through Store or carried by a stateless request protocol.
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn start(
        mut template: Template,
        resolved: ResolvedTemplate,
        target: CanonicalTarget,
        context: InvocationContext,
        now: jiff::Zoned,
        config: crate::config::Config,
        parts: crate::parts::Selection,
        inputs: &[PathBuf],
        answers: &RawAnswers,
        seed: Option<crate::application_seed::LikeSeed>,
    ) -> Result<Self, OperationError> {
        if context.target() != &target
            || context.formal_name() != Some(resolved.formal_name.as_str())
        {
            return Err(OperationError::Input(
                "invocation context does not match the selected template and target".into(),
            ));
        }
        crate::requirement::check(&template)?;
        crate::document::prepare(&mut template, target.as_path(), inputs, answers)?;
        let record = prepare_record(
            &mut template,
            &resolved,
            context,
            &now,
            parts,
            answers,
            seed.as_ref(),
        )
        .map_err(OperationError::Input)?;
        Ok(Self {
            template,
            resolved,
            record,
            target,
            config,
        })
    }

    /// Reload a request using its exact recorded revision and admitted document values.
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn resume(
        mut record: StagedRecord,
        target: CanonicalTarget,
        paths: &crate::resolve::Paths,
        config: crate::config::Config,
        registry: &crate::registry::Registry,
        cwd: &Path,
        inputs: &[PathBuf],
        incoming: &RawAnswers,
    ) -> Result<Self, OperationError> {
        record.invocation_context(&target)?;
        let resolved = crate::resolve::resume_template(
            &record.template,
            &record.commit,
            record.named,
            &config,
            registry,
            paths,
            cwd,
        )?;
        let mut template = Template::load(&resolved.folder)?;
        crate::requirement::check(&template)?;
        crate::document::restore_missing(&mut template, &recorded_answers(&record))
            .map_err(OperationError::Input)?;
        prepare_staged_documents(
            &mut template,
            target.as_path(),
            inputs,
            incoming,
            &mut record,
        )?;
        template.part_selection = record.part_selection.clone();
        Ok(Self {
            template,
            resolved,
            record,
            target,
            config,
        })
    }

    /// Rebuild the pure engine for this request. Existing legacy record replay remains supported.
    pub fn replay(&self) -> Result<Interview<'_>, OperationError> {
        let resolution = crate::interview::configured_defaults(
            &self.resolved.formal_name,
            &self.template,
            &self.config.presets,
            &self.config.template_defaults,
        )?;
        let seed = pinned_seed(
            &self.record,
            &crate::applied::source_identity(&self.resolved.formal_name),
            &self.target,
        )?;
        Ok(self
            .record
            .replay_with_seed(&self.template, resolution, seed, &self.target)?)
    }

    fn response(&self, interview: Interview<'_>, rejections: crate::Rejections) -> SessionState {
        match interview {
            Interview::Complete(done) => SessionState::Completed(done),
            Interview::Ended(ended) => SessionState::Ended(ended),
            Interview::Asking(pending) => SessionState::Asking {
                document: crate::protocol::batch_document(
                    pending.batch(),
                    &crate::protocol::Context::new(&self.target, &self.record),
                    Some(&rejections),
                ),
                rejections,
            },
        }
    }

    pub fn state(&self) -> Result<SessionState, OperationError> {
        Ok(self.response(self.replay()?, vec![]))
    }

    /// Accept one batch. A rejected batch returns typed per-question errors and changes no record.
    #[allow(clippy::result_large_err)]
    pub fn answer(&mut self, answers: RawAnswers) -> Result<SessionState, OperationError> {
        let (state, accepted) = match self.replay()? {
            Interview::Asking(pending) => match pending.answer(answers.clone()) {
                Ok(next) => (self.response(next, vec![]), true),
                Err(AnswerError::Rejected {
                    pending,
                    rejections,
                }) => (self.response(Interview::Asking(pending), rejections), false),
                Err(AnswerError::Eval(e)) => return Err(e.into()),
            },
            _ => {
                return Err(OperationError::Input(
                    "submission after completed interview".into(),
                ));
            }
        };
        if accepted {
            self.record.submissions.push(
                answers
                    .into_iter()
                    .map(|(id, raw)| (id.to_string(), raw.0))
                    .collect(),
            );
        }
        Ok(state)
    }

    pub fn plan(&self, completed: &Completed) -> Result<Plan, OperationError> {
        build_plan(
            &self.template,
            completed,
            &self.target,
            &self.resolved.formal_name,
            &self.resolved.commit,
        )
    }

    /// Plan/preview/apply through the same engine and caller runner. Store cleanup occurs only after a successful write.
    #[allow(clippy::result_large_err)]
    pub fn apply(
        &self,
        completed: &Completed,
        options: ApplyOptions,
        dry_run: bool,
        runner: &dyn crate::hook::HookRunner,
        store: Option<&Store>,
    ) -> Result<ApplicationResult, OperationError> {
        let plan = self.plan(completed)?;
        if crate::updates::effective_dry_run(dry_run, completed) {
            return Ok(ApplicationResult::Planned(plan));
        }
        let result = plan.apply(&self.target, options, runner)?;
        if matches!(result, Applied::Written { .. })
            && let Some(store) = store
        {
            store.remove(&self.target)?;
        }
        Ok(ApplicationResult::Applied(result))
    }
}

#[allow(clippy::large_enum_variant)]
pub enum ApplicationResult {
    Planned(Plan),
    Applied(Applied),
}

pub fn build_plan(
    template: &Template,
    completed: &Completed,
    target: &CanonicalTarget,
    formal: &str,
    commit: &str,
) -> Result<Plan, OperationError> {
    if let Some(context) = completed.context()
        && (context.target() != target || context.formal_name().is_some_and(|name| name != formal))
    {
        return Err(OperationError::Input(
            "completed interview does not match the selected template and target".into(),
        ));
    }
    let mut plan = Plan::build(template, completed, target)?;
    plan.record_identity(
        formal.to_owned(),
        crate::resolve::applied_revision(formal, commit)?,
    );
    Ok(plan)
}

/// Classifies each planned write against the target's current bytes before any
/// write: `create`/`overwrite`/`conflict` for a whole-file rule, `inject`/
/// `update` for an edit (by whether the region or value is present), carrying the
/// region key or JSON path an edit governs. A whole-file rule that a path also
/// edits keeps its create/overwrite action; the edit folds into the generated
/// image and is not classified against the not-yet-written target. An unchanged
/// edit is omitted, as it writes nothing.
#[allow(clippy::result_large_err)]
pub fn classify_paths(
    plan: &Plan,
    target: &CanonicalTarget,
    force: bool,
) -> Result<Vec<crate::protocol::PlannedPath>, crate::ApplyError> {
    use crate::{EditReport, PlannedEdit};
    let mut paths = Vec::new();
    for file in &plan.files {
        let action = if plan.conflicts.contains(&file.path) {
            if force { "overwrite" } else { "conflict" }
        } else {
            "create"
        };
        paths.push(crate::protocol::PlannedPath {
            path: file.path.to_string(),
            action: action.to_string(),
            owner: None,
        });
    }
    for edit in &plan.edits {
        if plan.files.iter().any(|file| file.path == *edit.path()) {
            continue;
        }
        let full = target.as_path().join(edit.path().as_path());
        let current = std::fs::read(&full).ok();
        let (report, owner) = match edit {
            PlannedEdit::Region(region) => (
                crate::report_region_edit(current.as_deref(), region)?,
                region.region.to_string(),
            ),
            PlannedEdit::JsonValue(json) => (
                crate::report_json_edit(current.as_deref(), json)?,
                json.json_path.to_string(),
            ),
        };
        let action = match report {
            EditReport::Inject => "inject",
            EditReport::Update | EditReport::Drift => "update",
            EditReport::Unchanged => continue,
        };
        paths.push(crate::protocol::PlannedPath {
            path: edit.path().to_string(),
            action: action.to_string(),
            owner: Some(owner),
        });
    }
    Ok(paths)
}
