#![doc = include_str!("../README.md")]
// ---
// relationships:
//   implements: architecture
// ---
pub mod applied;
pub mod apply;
pub mod config;
pub mod context;
pub mod document;
mod fault;
pub mod git_target;
pub mod hook;
pub mod inject;
pub mod interview;
mod jinja;
pub mod parts;
pub mod plan;
pub mod protocol;
pub mod registry;
pub mod requirement;
pub mod review;
pub mod source;
pub mod staging;
pub mod template;

pub use apply::{Applied, ApplyError, ApplyOptions, HookReport};
pub use inject::{
    Anchor, EditReport, EditResolution, JsonEditError, JsonFormat, JsonPath, JsonPathSegment,
    MarkerStyle, Occurrence, PlannedEdit, PlannedJsonEdit, PlannedRegionEdit, RegionError,
    RegionKey, json_value_to_cst_input, report_json_edit, report_region_edit, resolve_json_edit,
    resolve_region_edit,
};
pub use interview::{
    Answer, AnswerError, Answers, Batch, CheckError, Completed, Disposition, EndKind, Ended,
    EvalError, Interview, Item, Pending, Prompt, PromptKind, RawAnswer, RawAnswers, Rejection,
    RejectionKind, Rejections, Seed, Step,
};
pub use plan::{
    Content, Deferred, FileMutation, Plan, PlanError, Planned, PlannedFile, PlannedHook,
    PlannedProgram, TargetPath,
};
pub use requirement::{MissingRequirements, Requirement};
pub use review::{
    HookSurface, HookSurfaceDiff, HookView, ReviewDigest, ReviewError, ScriptDigest, Trust,
    evaluate_trust,
};
pub use template::{
    FlowAction, FlowNode, Id, LoadError, Node, Problem, Question, QuestionKind, SkipScope, Template,
};

/// Application selection and answer defaults.
pub mod application_seed;
/// The embedded offline template and its content identity.
pub mod bundled;
/// Resolve, fetch, cache and resume template identities with caller-owned paths.
pub mod resolve;

/// Installed-template transactions and structured reports.
pub mod catalog;

/// Embedded agent guidance inventory, bytes and export.
pub mod assets;

/// Shared application/session operations with explicit caller inputs.
pub mod operations;

/// Prepared baseline and recorded-application update operations.
pub mod updates;
