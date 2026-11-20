use std::fmt;

use serde::Serialize;

/// Whether a finding makes its gate fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Level {
    /// The gate fails.
    Error,
    /// The gate still passes.
    Warning,
}

impl Level {
    /// The normative name of the level, `error` or `warning`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warning => "warning",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// The code of a finding: one of the normative codes (concept Section 7, "Findings"), or a code
/// that a plugin chose (concept Section 9).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Code {
    /// The file violates the strict YAML profile or the file shape.
    YamlProfile,
    /// `type` names neither `ticket` nor a kind of the configuration.
    KindUnknown,
    /// `id`, `ticket`, or `type` disagrees with the path.
    PathMismatch,
    /// `id` does not start with the `id_prefix` of the kind.
    IdPrefix,
    /// The declared status is not a status of the kind.
    Status,
    /// The data does not match the schema of its lifecycle and kind.
    Schema,
    /// A key without `x-` that no schema names.
    UnknownField,
    /// A file looks like an artifact but matches no path pattern.
    UnmatchedFile,
    /// Another file carries the same ID.
    IdDuplicate,
    /// A link points to an artifact or a ticket record that does not exist.
    LinkUnresolved,
    /// The ticket has no matching artifact.
    ArtifactMissing,
    /// An artifact of the ticket has an effective status outside the set.
    StatusMismatch,
    /// An artifact has fewer valid approvals than required.
    ApprovalsMissing,
    /// An anchored artifact hit by the changed paths has no binding artifact through it.
    BindingMissing,
    /// The caller supplied no changed paths.
    ChangedPathsMissing,
    /// A blocking thread of the ticket is open.
    ThreadBlockingOpen,
    /// A thread of the ticket opened by the role is not answered.
    ThreadUnanswered,
    /// A required step is neither done nor skipped.
    StepIncomplete,
    /// The ticket's profile is not the result of `select` for its attributes.
    ProfileNotSelected,
    /// The anchor names another repository, which the checked state cannot see.
    OtherRepository,
    /// No file, directory, or match for an anchor.
    AnchorMissing,
    /// The file exists, the language plugin does not find the symbol.
    SymbolMissing,
    /// No language plugin handles the file of a symbol anchor.
    SymbolUnchecked,
    /// The language plugin cannot parse a file of the checked state.
    Unparsable,
    /// The public API of a hit file changed and no binding artifact names the anchored one.
    PublicApiUnbound,
    /// No language plugin handles a hit file.
    ApiUnchecked,
    /// The caller supplied no base state.
    BaseMissing,
    /// The gate needs the process store, which the call does not have.
    ProcessStoreUnavailable,
    /// An event of the ticket, or one whose ticket cannot be read, is invalid.
    EventInvalid,
    /// The `.wasm` file, its lock entry, or the hash disagree.
    PluginHash,
    /// The capabilities of a plugin exceed the approved ones in the lockfile.
    PluginCapabilities,
    /// The plugin could not run or returned an invalid output.
    PluginFailed,
    /// The command of an `exec` gate is not approved.
    ExecUnapproved,
    /// The command of an `exec` gate did not start, timed out, ended through a signal, or failed.
    ExecFailed,
    /// The implementation does not provide this gate.
    Unsupported,
    /// A code that a plugin chose: a key, lowercase with hyphens.
    Plugin(String),
}

impl Code {
    /// The name of the code, for example `path-mismatch`.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Code::YamlProfile => "yaml-profile",
            Code::KindUnknown => "kind-unknown",
            Code::PathMismatch => "path-mismatch",
            Code::IdPrefix => "id-prefix",
            Code::Status => "status",
            Code::Schema => "schema",
            Code::UnknownField => "unknown-field",
            Code::UnmatchedFile => "unmatched-file",
            Code::IdDuplicate => "id-duplicate",
            Code::LinkUnresolved => "link-unresolved",
            Code::ArtifactMissing => "artifact-missing",
            Code::StatusMismatch => "status-mismatch",
            Code::ApprovalsMissing => "approvals-missing",
            Code::BindingMissing => "binding-missing",
            Code::ChangedPathsMissing => "changed-paths-missing",
            Code::ThreadBlockingOpen => "thread-blocking-open",
            Code::ThreadUnanswered => "thread-unanswered",
            Code::StepIncomplete => "step-incomplete",
            Code::ProfileNotSelected => "profile-not-selected",
            Code::OtherRepository => "other-repository",
            Code::AnchorMissing => "anchor-missing",
            Code::SymbolMissing => "symbol-missing",
            Code::SymbolUnchecked => "symbol-unchecked",
            Code::Unparsable => "unparsable",
            Code::PublicApiUnbound => "public-api-unbound",
            Code::ApiUnchecked => "api-unchecked",
            Code::BaseMissing => "base-missing",
            Code::ProcessStoreUnavailable => "process-store-unavailable",
            Code::EventInvalid => "event-invalid",
            Code::PluginHash => "plugin-hash",
            Code::PluginCapabilities => "plugin-capabilities",
            Code::PluginFailed => "plugin-failed",
            Code::ExecUnapproved => "exec-unapproved",
            Code::ExecFailed => "exec-failed",
            Code::Unsupported => "unsupported",
            Code::Plugin(code) => code,
        }
    }
}

impl Serialize for Code {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl fmt::Display for Code {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// One result of a gate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Finding {
    /// Whether the finding makes the gate fail.
    pub level: Level,
    /// Why the gate reports it.
    pub code: Code,
    /// The file, relative to the project root, if the finding concerns one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// A JSON Pointer into the data model of the file, if the finding concerns a value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    /// A human-readable explanation, not normative.
    pub message: String,
}

impl Finding {
    pub(crate) fn error(code: Code, message: impl Into<String>) -> Self {
        Self {
            level: Level::Error,
            code,
            path: None,
            pointer: None,
            message: message.into(),
        }
    }

    pub(crate) fn warning(code: Code, message: impl Into<String>) -> Self {
        Self {
            level: Level::Warning,
            ..Self::error(code, message)
        }
    }

    pub(crate) fn at_path(mut self, path: &str) -> Self {
        self.path = Some(path.to_owned());
        self
    }

    pub(crate) fn at_pointer(mut self, pointer: impl ToString) -> Self {
        self.pointer = Some(pointer.to_string());
        self
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.level, self.code)?;
        if let Some(path) = &self.path {
            write!(formatter, " {path}")?;
        }
        if let Some(pointer) = &self.pointer {
            write!(formatter, " {pointer}")?;
        }
        write!(formatter, ": {}", self.message)
    }
}
