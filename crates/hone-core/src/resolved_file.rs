use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::{CheckError, PackageSource, Problem, Resolved, resolve};

/// The state of `.hone/resolved.json` after [`write_resolved`] or [`compare_resolved`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ResolvedFile {
    /// The file was written.
    Written {
        /// The hash of the resolved configuration.
        hash: String,
    },
    /// The file matches its sources.
    UpToDate {
        /// The hash of the resolved configuration.
        hash: String,
    },
    /// The file is missing or does not match its sources.
    OutOfDate {
        /// The hash the file should have.
        hash: String,
    },
}

/// [`write_resolved`] or [`compare_resolved`] could not finish.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ResolvedFileError {
    /// The resolution failed.
    Failed {
        /// The problems of the first resolution step that found any.
        problems: Vec<Problem>,
    },
    /// The file could not be written.
    IoError {
        /// The path and the cause.
        message: String,
    },
}

impl fmt::Display for ResolvedFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed { problems } => {
                let error = crate::ResolveError {
                    problems: problems.clone(),
                };
                write!(formatter, "{error}")
            }
            Self::IoError { message } => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for ResolvedFileError {}

/// Resolves the configuration of the project in `project_root` outside a ticket and writes
/// `.hone/resolved.json`.
///
/// # Errors
///
/// Returns [`ResolvedFileError::Failed`] if the resolution fails and
/// [`ResolvedFileError::IoError`] if the file cannot be written.
pub fn write_resolved(
    project_root: &Path,
    packages: &dyn PackageSource,
) -> Result<ResolvedFile, ResolvedFileError> {
    let resolved = resolve_project(project_root, packages)?;
    let target = target(project_root);
    std::fs::write(&target, resolved.file_contents()).map_err(|io| ResolvedFileError::IoError {
        message: format!("{}: {io}", target.display()),
    })?;
    Ok(ResolvedFile::Written {
        hash: resolved.hash(),
    })
}

/// Resolves the configuration of the project in `project_root` outside a ticket and compares
/// the result with `.hone/resolved.json` byte for byte, without writing anything.
///
/// # Errors
///
/// Returns [`ResolvedFileError::Failed`] if the resolution fails.
pub fn compare_resolved(
    project_root: &Path,
    packages: &dyn PackageSource,
) -> Result<ResolvedFile, ResolvedFileError> {
    let resolved = resolve_project(project_root, packages)?;
    let hash = resolved.hash();
    Ok(match std::fs::read(target(project_root)) {
        Ok(existing) if existing == resolved.file_contents() => ResolvedFile::UpToDate { hash },
        _ => ResolvedFile::OutOfDate { hash },
    })
}

impl Resolved {
    /// Reads `.hone/resolved.json` of the project in `project_root`.
    ///
    /// # Errors
    ///
    /// Returns a [`CheckError`] if the file is missing, not JSON, or not a valid resolved
    /// configuration.
    pub fn load(project_root: &Path) -> Result<Self, CheckError> {
        read_resolved(&target(project_root))
    }
}

pub(crate) fn read_resolved(path: &Path) -> Result<Resolved, CheckError> {
    let error = |message: String| CheckError {
        message: format!("{}: {message}", path.display()),
    };
    let bytes = std::fs::read(path).map_err(|io| error(io.to_string()))?;
    let document: Value =
        serde_json::from_slice(&bytes).map_err(|json| error(format!("not JSON: {json}")))?;
    Resolved::from_document(document).map_err(|problems| error(problems.to_string()))
}

fn resolve_project(
    project_root: &Path,
    packages: &dyn PackageSource,
) -> Result<Resolved, ResolvedFileError> {
    resolve(project_root, packages, None).map_err(|error| ResolvedFileError::Failed {
        problems: error.problems,
    })
}

fn target(project_root: &Path) -> PathBuf {
    project_root.join(".hone").join("resolved.json")
}
