use std::fmt;

use serde::Serialize;

/// The normative error categories of `resolve` (concept Section 8, "Errors").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    /// A file violates the strict YAML profile.
    YamlProfile,
    /// Unknown or forbidden entry in a layer.
    Layout,
    /// `config.yaml` or `lock.yaml` is missing where required or malformed.
    Manifest,
    /// A `null` removes nothing.
    NullWithoutTarget,
    /// No matching lock entry, or the package is not available.
    PackageMissing,
    /// The package content does not match its pinned hash.
    PackageHash,
    /// Two majors of the same package.
    PackageConflict,
    /// A kind schema path leaves its layer, does not exist, is not JSON, or is inline.
    SchemaPath,
    /// The `.wasm` file or its lock entry is missing, or the hashes differ.
    PluginHash,
    /// Capabilities beyond the approved ones, or `net` not empty.
    PluginCapabilities,
    /// Two language plugins claim the same file extension.
    LanguageConflict,
    /// A ticket override touches `protocol`, `ticket`, `plugins`, or a kind schema.
    OverrideForbidden,
    /// The result does not match the schema of the resolved configuration.
    Shape,
    /// A reference of the resolved document does not resolve.
    Reference,
}

impl Category {
    /// The normative name of the category, for example `null-without-target`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Category::YamlProfile => "yaml-profile",
            Category::Layout => "layout",
            Category::Manifest => "manifest",
            Category::NullWithoutTarget => "null-without-target",
            Category::PackageMissing => "package-missing",
            Category::PackageHash => "package-hash",
            Category::PackageConflict => "package-conflict",
            Category::SchemaPath => "schema-path",
            Category::PluginHash => "plugin-hash",
            Category::PluginCapabilities => "plugin-capabilities",
            Category::LanguageConflict => "language-conflict",
            Category::OverrideForbidden => "override-forbidden",
            Category::Shape => "shape",
            Category::Reference => "reference",
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// The layer a file belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "name")]
pub enum Origin {
    /// The project's `.hone/` directory.
    Project,
    /// A package, by name.
    Package(String),
}

/// Where a problem was found.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Location {
    /// A JSON Pointer (RFC 6901) into the resolved document.
    Pointer(String),
    /// A file or directory of a layer. `path` is relative to the layer root (the package root
    /// or `.hone/`) with `/` as separator, and empty for the layer root itself.
    File {
        /// The layer of the file.
        origin: Origin,
        /// Path relative to the layer root.
        path: String,
    },
}

impl fmt::Display for Location {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Location::Pointer(pointer) => write!(formatter, "#{pointer}"),
            Location::File { origin, path } => {
                match origin {
                    Origin::Project => formatter.write_str(".hone")?,
                    Origin::Package(name) => write!(formatter, "package {name}")?,
                }
                if !path.is_empty() {
                    write!(formatter, "/{path}")?;
                }
                Ok(())
            }
        }
    }
}

/// One error found by `resolve`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// The normative category.
    pub category: Category,
    /// Where the problem was found.
    pub location: Location,
    /// A human-readable explanation. Not normative.
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {}: {}",
            self.category, self.location, self.message
        )
    }
}

/// The failure of `resolve`: at least one problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolveError {
    /// The problems found, never empty.
    pub problems: Vec<Problem>,
}

impl fmt::Display for ResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, problem) in self.problems.iter().enumerate() {
            if index > 0 {
                writeln!(formatter)?;
            }
            write!(formatter, "{problem}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ResolveError {}

pub(crate) fn file_problem(
    category: Category,
    origin: &Origin,
    path: impl Into<String>,
    message: impl Into<String>,
) -> Problem {
    Problem {
        category,
        location: Location::File {
            origin: origin.clone(),
            path: path.into(),
        },
        message: message.into(),
    }
}

pub(crate) fn pointer_problem(
    category: Category,
    pointer: &crate::pointer::Pointer,
    message: impl Into<String>,
) -> Problem {
    Problem {
        category,
        location: Location::Pointer(pointer.to_string()),
        message: message.into(),
    }
}
