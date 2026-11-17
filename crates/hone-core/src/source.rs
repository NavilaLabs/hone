use std::path::{Path, PathBuf};

/// Provides the content of packages. Where packages come from (directory, archive, registry)
/// is the tool's business. `resolve` checks every package against its pinned hash.
pub trait PackageSource {
    /// Returns the root directory of the package `name` (for example `hone/standard`) in exactly
    /// `version` (semver), or `None` if it is not available.
    fn locate(&self, name: &str, version: &str) -> Option<PathBuf>;
}

/// Packages in a directory, one subdirectory per package name: `acme/base` lies in
/// `<root>/acme/base/`. Holds a single version of each package, so `version` is not consulted.
#[derive(Debug, Clone)]
pub struct PackageDirectory {
    root: PathBuf,
}

impl PackageDirectory {
    /// Packages below `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl PackageSource for PackageDirectory {
    fn locate(&self, name: &str, _version: &str) -> Option<PathBuf> {
        let directory = self.root.join(Path::new(name));
        directory.is_dir().then_some(directory)
    }
}

/// A source without packages, for projects without `extends`.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoPackages;

impl PackageSource for NoPackages {
    fn locate(&self, _name: &str, _version: &str) -> Option<PathBuf> {
        None
    }
}
