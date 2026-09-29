#![doc = include_str!("../README.md")]
// ---
// relationships:
//   implements: architecture
// ---
pub mod apply;
pub mod config;
pub mod context;
mod fault;
pub mod hook;
pub mod inject;
pub mod interview;
mod jinja;
pub mod plan;
pub mod protocol;
pub mod registry;
pub mod review;
pub mod source;
pub mod staging;
pub mod template;

pub use apply::{Applied, ApplyError, ApplyOptions};
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
pub use review::{
    HookSurface, HookSurfaceDiff, HookView, ReviewDigest, ReviewError, ScriptDigest, Trust,
    evaluate_trust,
};
pub use template::{
    FlowAction, FlowNode, Id, LoadError, Node, Problem, Question, QuestionKind, SkipScope, Template,
};
