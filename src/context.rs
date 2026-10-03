// ---
// relationships:
//   implements:
//     - template-format
//     - interview-protocol
//   references:
//     - error-attribution
// ---
//! Toha's immutable, typed invocation context.
//!
//! The command adapter resolves the selected identity, canonical target, host
//! facts, execution facts, and an environment snapshot once, before the first
//! interview evaluation. The pure interview engine carries the resulting
//! [`InvocationContext`] unchanged, staging stores and restores it, and planning
//! consumes it. The engine never reads the process, registry, terminal, host
//! files, privilege state, or trust state.
//!
//! This module owns the typed facts, their projection into the seventeen
//! reserved Jinja names, redaction of captured environment values, the
//! environment-admission matrix, and the versioned staged wire form. Template
//! reference analysis (which produces an [`EnvironmentNeed`]) lives in
//! `template`/`jinja`; the command adapter owns live host and process capture.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::staging::CanonicalTarget;

/// The seventeen reserved Toha Jinja names, in table order.
///
/// Every current-context template exposes each name as a variable. Only these
/// exact names are reserved against template data keys, question ids, computed
/// ids, and `each` bindings; neighbouring `toha_` names remain legal.
pub const RESERVED_NAMES: [&str; 17] = [
    "toha_target_name",
    "toha_template_name",
    "toha_template_formal_name",
    "toha_template_aliases",
    "toha_template_source",
    "toha_host_os",
    "toha_host_arch",
    "toha_host_os_name",
    "toha_host_os_id",
    "toha_host_os_id_like",
    "toha_is_admin",
    "toha_is_interactive",
    "toha_env_user",
    "toha_env_hostname",
    "toha_env_editor",
    "toha_env_shell",
    "toha_env_visual",
];

/// The five fixed environment names, the closed vocabulary the trust gate
/// governs. Every value is captured together or not at all.
pub const ENVIRONMENT_NAMES: [&str; 5] = [
    "toha_env_user",
    "toha_env_hostname",
    "toha_env_editor",
    "toha_env_shell",
    "toha_env_visual",
];

/// The immutable per-invocation context carried through the interview engine.
///
/// A current context projects the seventeen reserved names; a legacy context
/// (restored from a pre-context staged record) projects none of them and keeps
/// the pre-context Jinja contract.
#[derive(Clone, Debug)]
pub struct InvocationContext {
    target: CanonicalTarget,
    contract: ContextContract,
}

#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
enum ContextContract {
    Legacy,
    Current(CurrentContext),
}

#[derive(Clone, Debug)]
struct CurrentContext {
    selected: SelectedTemplate,
    host: HostFacts,
    execution: ExecutionFacts,
    environment: EnvironmentSnapshot,
}

/// A failure to assemble an [`InvocationContext`] from supplied facts.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ContextError {
    #[error("selected template has an empty formal name")]
    EmptyFormalName,
    #[error("selected template has an empty short name")]
    EmptyShortName,
}

impl InvocationContext {
    /// Assembles a current context from resolved facts.
    pub fn new(
        target: CanonicalTarget,
        selected: SelectedTemplate,
        host: HostFacts,
        execution: ExecutionFacts,
        environment: EnvironmentSnapshot,
    ) -> Result<Self, ContextError> {
        if selected.formal_name.is_empty() {
            return Err(ContextError::EmptyFormalName);
        }
        if selected.short_name.is_empty() {
            return Err(ContextError::EmptyShortName);
        }
        Ok(Self {
            target,
            contract: ContextContract::Current(CurrentContext {
                selected,
                host,
                execution,
                environment,
            }),
        })
    }

    /// A legacy context carries only the target; it projects none of the
    /// seventeen names. Restored by staging for a pre-context record and used
    /// by direct callers that opt out of the current contract.
    pub(crate) fn legacy(target: CanonicalTarget) -> Self {
        Self {
            target,
            contract: ContextContract::Legacy,
        }
    }

    pub fn target(&self) -> &CanonicalTarget {
        &self.target
    }

    /// A direct-caller convenience: a current context bound to `target` with a
    /// generic identity, captured host facts, no administrative or interactive
    /// status, and no environment snapshot. For callers and tests that carry a
    /// context through the engine without exercising the seventeen values.
    pub fn for_target(target: CanonicalTarget) -> Self {
        Self {
            target,
            contract: ContextContract::Current(CurrentContext {
                selected: SelectedTemplate::new(
                    "template".to_owned(),
                    "template".to_owned(),
                    Vec::new(),
                    None,
                ),
                host: HostFacts::capture(),
                execution: ExecutionFacts::new(false, false),
                environment: EnvironmentSnapshot::Unavailable,
            }),
        }
    }

    /// The selected template's stable formal name, or `None` for a legacy
    /// context. Used by staging to serialize the record's formal name.
    pub(crate) fn formal_name(&self) -> Option<&str> {
        match &self.contract {
            ContextContract::Legacy => None,
            ContextContract::Current(current) => Some(&current.selected.formal_name),
        }
    }

    /// The target, formal name, and versioned wire for staging. A legacy
    /// context has no formal name of its own; staging keeps the record's
    /// existing formal name in that case.
    pub(crate) fn staged_parts(&self) -> (CanonicalTarget, Option<String>, InvocationContextWire) {
        (
            self.target.clone(),
            self.formal_name().map(str::to_owned),
            self.wire(),
        )
    }

    fn wire(&self) -> InvocationContextWire {
        match &self.contract {
            ContextContract::Legacy => InvocationContextWire::Legacy,
            ContextContract::Current(current) => InvocationContextWire::Current {
                short_name: current.selected.short_name.clone(),
                aliases: current.selected.aliases.clone(),
                source: current.selected.source.clone(),
                host: current.host.wire(),
                execution: current.execution.wire(),
                environment: current.environment.wire(),
            },
        }
    }

    /// Rebuilds a context from a producer-created target carrier, the record's
    /// formal name, and the restored wire. Used only by staging replay.
    pub(crate) fn from_wire(
        target: CanonicalTarget,
        formal_name: &str,
        wire: InvocationContextWire,
    ) -> Self {
        match wire {
            InvocationContextWire::Legacy => Self::legacy(target),
            InvocationContextWire::Current {
                short_name,
                aliases,
                source,
                host,
                execution,
                environment,
            } => Self {
                target,
                contract: ContextContract::Current(CurrentContext {
                    selected: SelectedTemplate {
                        formal_name: formal_name.to_owned(),
                        short_name,
                        aliases,
                        source,
                    },
                    host: HostFacts::from_wire(host),
                    execution: ExecutionFacts::from_wire(execution),
                    environment: EnvironmentSnapshot::from_wire(environment),
                }),
            },
        }
    }

    /// Inserts the seventeen reserved names into a Jinja evaluation context. A
    /// legacy context inserts none of them.
    pub(crate) fn project(&self, out: &mut BTreeMap<String, Value>) {
        let ContextContract::Current(current) = &self.contract else {
            return;
        };
        let mut set = |name: &str, value: Value| {
            out.insert(name.to_owned(), value);
        };
        set(
            "toha_target_name",
            optional(target_name(self.target.as_path())),
        );
        set(
            "toha_template_name",
            Value::String(current.selected.short_name.clone()),
        );
        set(
            "toha_template_formal_name",
            Value::String(current.selected.formal_name.clone()),
        );
        set(
            "toha_template_aliases",
            Value::Array(
                current
                    .selected
                    .aliases
                    .iter()
                    .map(|a| Value::String(a.clone()))
                    .collect(),
            ),
        );
        set(
            "toha_template_source",
            optional(current.selected.source.clone()),
        );
        set("toha_host_os", Value::String(current.host.os.clone()));
        set("toha_host_arch", Value::String(current.host.arch.clone()));
        set("toha_host_os_name", optional(current.host.os_name.clone()));
        set("toha_host_os_id", optional(current.host.os_id.clone()));
        set(
            "toha_host_os_id_like",
            Value::Array(
                current
                    .host
                    .os_id_like
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect(),
            ),
        );
        set("toha_is_admin", Value::Bool(current.execution.is_admin));
        set(
            "toha_is_interactive",
            Value::Bool(current.execution.is_interactive),
        );
        let env = &current.environment;
        set("toha_env_user", optional(env.field(EnvField::User)));
        set("toha_env_hostname", optional(env.field(EnvField::Hostname)));
        set("toha_env_editor", optional(env.field(EnvField::Editor)));
        set("toha_env_shell", optional(env.field(EnvField::Shell)));
        set("toha_env_visual", optional(env.field(EnvField::Visual)));
    }
}

fn optional(value: Option<String>) -> Value {
    value.map(Value::String).unwrap_or(Value::Null)
}

/// The final Unicode component of a canonical target, or `None` for a
/// filesystem root, prefix, or non-Unicode final component.
fn target_name(path: &Path) -> Option<String> {
    match path.components().next_back() {
        Some(Component::Normal(name)) => name.to_str().map(str::to_owned),
        _ => None,
    }
}

/// The selected template's identity, assembled at the resolution boundary. A
/// name/alias/short-name selection carries the registry entry's effective alias
/// list and source; a folder, direct Git, or bundled selection carries `[]` and
/// `None`.
#[derive(Clone, Debug)]
pub struct SelectedTemplate {
    formal_name: String,
    short_name: String,
    aliases: Vec<String>,
    source: Option<String>,
}

impl SelectedTemplate {
    pub fn new(
        formal_name: String,
        short_name: String,
        aliases: Vec<String>,
        source: Option<String>,
    ) -> Self {
        Self {
            formal_name,
            short_name,
            aliases,
            source,
        }
    }
}

/// Host facts captured by the command adapter. Never fails template load,
/// interview, or planning.
#[derive(Clone, Debug)]
pub struct HostFacts {
    os: String,
    arch: String,
    os_name: Option<String>,
    os_id: Option<String>,
    os_id_like: Vec<String>,
}

impl HostFacts {
    pub fn new(
        os: String,
        arch: String,
        os_name: Option<String>,
        os_id: Option<String>,
        os_id_like: Vec<String>,
    ) -> Self {
        Self {
            os,
            arch,
            os_name,
            os_id,
            os_id_like,
        }
    }

    /// Captures the Rust target OS/arch vocabulary and, on Linux, the
    /// `/etc/os-release` fallbacks. No file is executed and no subprocess runs.
    pub fn capture() -> Self {
        let (os_name, os_id, os_id_like) = if std::env::consts::OS == "linux" {
            match std::fs::read("/etc/os-release") {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => parse_os_release(&text),
                    Err(_) => (None, None, Vec::new()),
                },
                Err(_) => (None, None, Vec::new()),
            }
        } else {
            (None, None, Vec::new())
        };
        Self {
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            os_name,
            os_id,
            os_id_like,
        }
    }

    fn wire(&self) -> HostFactsWire {
        HostFactsWire {
            os: self.os.clone(),
            arch: self.arch.clone(),
            os_name: self.os_name.clone(),
            os_id: self.os_id.clone(),
            os_id_like: self.os_id_like.clone(),
        }
    }

    fn from_wire(wire: HostFactsWire) -> Self {
        Self {
            os: wire.os,
            arch: wire.arch,
            os_name: wire.os_name,
            os_id: wire.os_id,
            os_id_like: wire.os_id_like,
        }
    }
}

/// Parses `/etc/os-release` content, returning `NAME`, `ID`, and `ID_LIKE`.
///
/// Accepts standard quoted and unquoted assignments, unescapes double-quoted
/// values without executing the file, and uses the last valid assignment for a
/// duplicate key. Empty `NAME`/`ID` become `None`; empty `ID_LIKE` becomes
/// `[]`. `ID_LIKE` is split on ASCII whitespace in source order.
pub(crate) fn parse_os_release(text: &str) -> (Option<String>, Option<String>, Vec<String>) {
    let mut name = None;
    let mut id = None;
    let mut id_like: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, raw)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = unquote(raw.trim());
        match key {
            "NAME" => name = Some(value),
            "ID" => id = Some(value),
            "ID_LIKE" => {
                id_like = value.split_ascii_whitespace().map(str::to_owned).collect();
            }
            _ => {}
        }
    }
    (
        name.filter(|v| !v.is_empty()),
        id.filter(|v| !v.is_empty()),
        id_like,
    )
}

/// Unquotes an os-release value: double quotes honour `\\`, `\"`, `\$`, and
/// `` \` `` escapes; single quotes are literal; unquoted text is taken as is.
fn unquote(raw: &str) -> String {
    let bytes = raw.as_bytes();
    if raw.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        let inner = &raw[1..raw.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some(next @ ('\\' | '"' | '$' | '`')) => out.push(next),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(ch);
            }
        }
        out
    } else if raw.len() >= 2 && bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'' {
        raw[1..raw.len() - 1].to_owned()
    } else {
        raw.to_owned()
    }
}

/// Execution facts captured by the command adapter, frozen on resume.
#[derive(Clone, Copy, Debug)]
pub struct ExecutionFacts {
    is_admin: bool,
    is_interactive: bool,
}

impl ExecutionFacts {
    pub fn new(is_admin: bool, is_interactive: bool) -> Self {
        Self {
            is_admin,
            is_interactive,
        }
    }

    fn wire(&self) -> ExecutionFactsWire {
        ExecutionFactsWire {
            is_admin: self.is_admin,
            is_interactive: self.is_interactive,
        }
    }

    fn from_wire(wire: ExecutionFactsWire) -> Self {
        Self {
            is_admin: wire.is_admin,
            is_interactive: wire.is_interactive,
        }
    }
}

/// The five fixed environment names, used to read a captured snapshot field
/// without exposing the underlying struct.
#[derive(Clone, Copy)]
enum EnvField {
    User,
    Hostname,
    Editor,
    Shell,
    Visual,
}

/// The environment snapshot: either no captured values, or the closed set of
/// five optional plaintext values. Its `Debug` is redacted.
#[derive(Clone)]
pub enum EnvironmentSnapshot {
    Unavailable,
    Captured(FixedEnvironment),
}

impl EnvironmentSnapshot {
    fn field(&self, field: EnvField) -> Option<String> {
        match self {
            Self::Unavailable => None,
            Self::Captured(env) => env.field(field),
        }
    }

    fn wire(&self) -> EnvironmentWire {
        match self {
            Self::Unavailable => EnvironmentWire::Unavailable,
            Self::Captured(env) => EnvironmentWire::Captured {
                user: env.user.clone(),
                hostname: env.hostname.clone(),
                editor: env.editor.clone(),
                shell: env.shell.clone(),
                visual: env.visual.clone(),
            },
        }
    }

    fn from_wire(wire: EnvironmentWire) -> Self {
        match wire {
            EnvironmentWire::Unavailable => Self::Unavailable,
            EnvironmentWire::Captured {
                user,
                hostname,
                editor,
                shell,
                visual,
            } => Self::Captured(FixedEnvironment {
                user,
                hostname,
                editor,
                shell,
                visual,
            }),
        }
    }
}

impl std::fmt::Debug for EnvironmentSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => f.write_str("Unavailable"),
            Self::Captured(_) => f.write_str("Captured(<redacted>)"),
        }
    }
}

/// The closed set of five optional fixed environment values. Absent, empty, or
/// non-Unicode input is `None`. Its `Debug` is redacted so captured values
/// never enter diagnostics, protocol output, logs, or evidence.
#[derive(Clone)]
pub struct FixedEnvironment {
    user: Option<String>,
    hostname: Option<String>,
    editor: Option<String>,
    shell: Option<String>,
    visual: Option<String>,
}

impl FixedEnvironment {
    pub fn new(
        user: Option<String>,
        hostname: Option<String>,
        editor: Option<String>,
        shell: Option<String>,
        visual: Option<String>,
    ) -> Self {
        Self {
            user,
            hostname,
            editor,
            shell,
            visual,
        }
    }

    fn field(&self, field: EnvField) -> Option<String> {
        match field {
            EnvField::User => self.user.clone(),
            EnvField::Hostname => self.hostname.clone(),
            EnvField::Editor => self.editor.clone(),
            EnvField::Shell => self.shell.clone(),
            EnvField::Visual => self.visual.clone(),
        }
    }
}

impl std::fmt::Debug for FixedEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FixedEnvironment(<redacted>)")
    }
}

/// A source of the five fixed environment values. The command adapter's
/// implementation reads the process environment and native hostname; tests and
/// direct callers supply their own.
pub trait FixedEnvironmentSource {
    fn capture(&mut self) -> FixedEnvironment;
}

/// One caller decision about environment access, combined with the immutable
/// analysis need by [`admit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvironmentDecision {
    /// No access; project five nulls.
    Deny,
    /// Capture only when the loaded program references a fixed value.
    GrantIfNeeded,
    /// Stage without the flag: refuse if a need exists, else record unavailable.
    RequireStageGrant,
    /// Stage with the flag: capture all five once even without an initial need.
    CarryStageGrant,
}

impl EnvironmentDecision {
    /// The new-apply decision: capture when granted and the program needs a
    /// value; deny outright when the effective-trust gate does not hold.
    pub fn grant_if_needed(granted: bool) -> Self {
        if granted {
            Self::GrantIfNeeded
        } else {
            Self::Deny
        }
    }
}

/// The first deterministic location that references a fixed environment value,
/// carried by a refusal without any captured value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderOrigin {
    location: String,
}

impl RenderOrigin {
    pub fn new(location: impl Into<String>) -> Self {
        Self {
            location: location.into(),
        }
    }

    pub fn location(&self) -> &str {
        &self.location
    }
}

impl std::fmt::Display for RenderOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.location)
    }
}

/// The immutable analysis result: whether the loaded render program can observe
/// any fixed environment value, and the first location that does. This is the
/// private analysis mask; it is not exposed to callers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvironmentNeed {
    None,
    Needed(RenderOrigin),
}

/// A failure to admit environment access. Only [`Self::TrustRequired`] maps to a
/// stage trust refusal; any other fault retains its typed detail.
#[derive(Debug, thiserror::Error)]
pub enum EnvironmentAdmissionError {
    #[error("environment access at {reference} requires stage trust")]
    TrustRequired { reference: RenderOrigin },
}

/// Combines the immutable analysis need with one caller decision, capturing the
/// closed five-value snapshot exactly once when granted and never otherwise.
pub(crate) fn admit(
    need: &EnvironmentNeed,
    decision: EnvironmentDecision,
    source: &mut impl FixedEnvironmentSource,
) -> Result<EnvironmentSnapshot, EnvironmentAdmissionError> {
    let has_need = matches!(need, EnvironmentNeed::Needed(_));
    match decision {
        EnvironmentDecision::Deny => Ok(EnvironmentSnapshot::Unavailable),
        EnvironmentDecision::CarryStageGrant => Ok(EnvironmentSnapshot::Captured(source.capture())),
        EnvironmentDecision::GrantIfNeeded if has_need => {
            Ok(EnvironmentSnapshot::Captured(source.capture()))
        }
        EnvironmentDecision::GrantIfNeeded => Ok(EnvironmentSnapshot::Unavailable),
        EnvironmentDecision::RequireStageGrant => match need {
            EnvironmentNeed::Needed(origin) => Err(EnvironmentAdmissionError::TrustRequired {
                reference: origin.clone(),
            }),
            EnvironmentNeed::None => Ok(EnvironmentSnapshot::Unavailable),
        },
    }
}

/// The versioned staged wire for the invocation context. Separate from the live
/// domain carrier; it duplicates neither the target nor the formal name.
#[derive(Serialize, Deserialize, Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum InvocationContextWire {
    Legacy,
    Current {
        short_name: String,
        aliases: Vec<String>,
        source: Option<String>,
        host: HostFactsWire,
        execution: ExecutionFactsWire,
        environment: EnvironmentWire,
    },
}

impl std::fmt::Debug for InvocationContextWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Legacy => f.write_str("Legacy"),
            Self::Current { .. } => f.write_str("Current { <redacted> }"),
        }
    }
}

impl Default for InvocationContextWire {
    /// A staged record with no context field deserializes as legacy.
    fn default() -> Self {
        Self::Legacy
    }
}

impl InvocationContextWire {
    /// Whether this is the legacy projection. A legacy context is not
    /// serialized, so a record written by [`super::staging::StagedRecord::new`]
    /// stays byte-compatible with a pre-context record.
    pub(crate) fn is_legacy(&self) -> bool {
        matches!(self, Self::Legacy)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct HostFactsWire {
    os: String,
    arch: String,
    os_name: Option<String>,
    os_id: Option<String>,
    os_id_like: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub(crate) struct ExecutionFactsWire {
    is_admin: bool,
    is_interactive: bool,
}

/// The staged environment wire. Its `Debug` is redacted.
#[derive(Serialize, Deserialize, Clone)]
pub(crate) enum EnvironmentWire {
    Unavailable,
    Captured {
        user: Option<String>,
        hostname: Option<String>,
        editor: Option<String>,
        shell: Option<String>,
        visual: Option<String>,
    },
}

impl std::fmt::Debug for EnvironmentWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => f.write_str("Unavailable"),
            Self::Captured { .. } => f.write_str("Captured { <redacted> }"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A fixed-value source that counts captures and returns generic values, so
    /// tests can assert the exact number of reads.
    #[derive(Default)]
    struct SpySource {
        captures: Cell<u32>,
    }
    impl FixedEnvironmentSource for &SpySource {
        fn capture(&mut self) -> FixedEnvironment {
            self.captures.set(self.captures.get() + 1);
            FixedEnvironment::new(
                Some("sample-user".into()),
                Some("sample-host".into()),
                Some("sample-editor".into()),
                None,
                None,
            )
        }
    }

    fn origin() -> RenderOrigin {
        RenderOrigin::new("interview[0].prompt")
    }

    #[test]
    fn admit_matrix_captures_exactly_once_when_granted() {
        // none + Deny/GrantIfNeeded/RequireStageGrant -> Unavailable, zero reads.
        for decision in [
            EnvironmentDecision::Deny,
            EnvironmentDecision::GrantIfNeeded,
            EnvironmentDecision::RequireStageGrant,
        ] {
            let spy = SpySource::default();
            let snapshot = admit(&EnvironmentNeed::None, decision, &mut &spy).unwrap();
            assert!(matches!(snapshot, EnvironmentSnapshot::Unavailable));
            assert_eq!(spy.captures.get(), 0, "{decision:?} read a value");
        }
        // none + CarryStageGrant -> Captured, exactly one read.
        let spy = SpySource::default();
        let snapshot = admit(
            &EnvironmentNeed::None,
            EnvironmentDecision::CarryStageGrant,
            &mut &spy,
        )
        .unwrap();
        assert!(matches!(snapshot, EnvironmentSnapshot::Captured(_)));
        assert_eq!(spy.captures.get(), 1);
        // present + Deny -> Unavailable, zero reads.
        let spy = SpySource::default();
        let snapshot = admit(
            &EnvironmentNeed::Needed(origin()),
            EnvironmentDecision::Deny,
            &mut &spy,
        )
        .unwrap();
        assert!(matches!(snapshot, EnvironmentSnapshot::Unavailable));
        assert_eq!(spy.captures.get(), 0);
        // present + GrantIfNeeded/CarryStageGrant -> Captured, one read.
        for decision in [
            EnvironmentDecision::GrantIfNeeded,
            EnvironmentDecision::CarryStageGrant,
        ] {
            let spy = SpySource::default();
            let snapshot = admit(&EnvironmentNeed::Needed(origin()), decision, &mut &spy).unwrap();
            assert!(matches!(snapshot, EnvironmentSnapshot::Captured(_)));
            assert_eq!(spy.captures.get(), 1, "{decision:?}");
        }
        // present + RequireStageGrant -> TrustRequired, zero reads, carries origin.
        let spy = SpySource::default();
        let error = admit(
            &EnvironmentNeed::Needed(origin()),
            EnvironmentDecision::RequireStageGrant,
            &mut &spy,
        )
        .unwrap_err();
        let EnvironmentAdmissionError::TrustRequired { reference } = error;
        assert_eq!(reference, origin());
        assert_eq!(spy.captures.get(), 0);
    }

    #[test]
    fn parse_os_release_reads_name_id_and_id_like() {
        let text = "\
# a comment
NAME=\"Sample Linux\"
ID=sample
ID_LIKE=\"debian ubuntu\"
";
        let (name, id, id_like) = parse_os_release(text);
        assert_eq!(name.as_deref(), Some("Sample Linux"));
        assert_eq!(id.as_deref(), Some("sample"));
        assert_eq!(id_like, vec!["debian".to_owned(), "ubuntu".to_owned()]);
    }

    #[test]
    fn parse_os_release_last_assignment_wins_and_empties_are_none() {
        let (name, id, id_like) =
            parse_os_release("NAME=first\nNAME=second\nID=\"\"\nID_LIKE=\"\"");
        assert_eq!(name.as_deref(), Some("second"));
        assert_eq!(id, None);
        assert!(id_like.is_empty());
    }

    #[test]
    fn parse_os_release_unescapes_double_quotes_without_executing() {
        let (name, _, _) = parse_os_release(r#"NAME="a \"b\" \\ \$c""#);
        assert_eq!(name.as_deref(), Some(r#"a "b" \ $c"#));
    }

    #[test]
    fn absent_or_malformed_os_release_yields_all_unavailable() {
        let (name, id, id_like) = parse_os_release("garbage-with-no-assignments\n\n");
        assert_eq!(name, None);
        assert_eq!(id, None);
        assert!(id_like.is_empty());
    }

    fn current_context() -> InvocationContext {
        let target = crate::staging::canonical_target(std::path::Path::new(".")).unwrap();
        InvocationContext::new(
            target,
            SelectedTemplate::new(
                "github.com/acme/widget".into(),
                "widget".into(),
                vec!["w".into()],
                Some("https://example.invalid/widget".into()),
            ),
            HostFacts::new(
                "linux".into(),
                "x86_64".into(),
                Some("Sample Linux".into()),
                Some("sample".into()),
                vec!["debian".into()],
            ),
            ExecutionFacts::new(true, false),
            EnvironmentSnapshot::Captured(FixedEnvironment::new(
                Some("sample-user".into()),
                None,
                Some("hx".into()),
                None,
                None,
            )),
        )
        .unwrap()
    }

    #[test]
    fn current_context_projects_all_seventeen_names() {
        let mut out = BTreeMap::new();
        current_context().project(&mut out);
        for name in RESERVED_NAMES {
            assert!(out.contains_key(name), "missing {name}");
        }
        assert_eq!(out.len(), 17);
        assert_eq!(out["toha_template_name"], Value::String("widget".into()));
        assert_eq!(out["toha_is_admin"], Value::Bool(true));
        assert_eq!(out["toha_is_interactive"], Value::Bool(false));
        assert_eq!(out["toha_env_user"], Value::String("sample-user".into()));
        // Unavailable fixed values and an absent host field project null.
        assert_eq!(out["toha_env_hostname"], Value::Null);
        assert_eq!(out["toha_env_shell"], Value::Null);
        assert_eq!(
            out["toha_template_aliases"],
            Value::Array(vec![Value::String("w".into())])
        );
    }

    #[test]
    fn legacy_context_projects_no_reserved_names() {
        let target = crate::staging::canonical_target(std::path::Path::new(".")).unwrap();
        let mut out = BTreeMap::new();
        InvocationContext::legacy(target).project(&mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn environment_and_wire_debug_are_redacted() {
        let env = EnvironmentSnapshot::Captured(FixedEnvironment::new(
            Some("secret-user".into()),
            Some("secret-host".into()),
            None,
            None,
            None,
        ));
        let rendered = format!("{env:?}");
        assert!(!rendered.contains("secret-user"));
        assert!(!rendered.contains("secret-host"));
        let wire = env.wire();
        assert!(!format!("{wire:?}").contains("secret"));
    }

    #[test]
    fn context_round_trips_through_wire() {
        let context = current_context();
        let (target, formal, wire) = context.staged_parts();
        assert_eq!(formal.as_deref(), Some("github.com/acme/widget"));
        let restored = InvocationContext::from_wire(target, &formal.unwrap(), wire);
        let mut before = BTreeMap::new();
        let mut after = BTreeMap::new();
        context.project(&mut before);
        restored.project(&mut after);
        assert_eq!(before, after);
    }

    #[test]
    fn empty_identity_is_rejected() {
        let target = crate::staging::canonical_target(std::path::Path::new(".")).unwrap();
        let result = InvocationContext::new(
            target,
            SelectedTemplate::new(String::new(), "x".into(), vec![], None),
            HostFacts::new("linux".into(), "x86_64".into(), None, None, vec![]),
            ExecutionFacts::new(false, true),
            EnvironmentSnapshot::Unavailable,
        );
        assert!(matches!(result, Err(ContextError::EmptyFormalName)));
    }
}
