//! A Git branch as a shared folder (concept Section 12, "Process store in Git"). This is tool
//! code: the protocol knows only the shared folder, and Git moves it.

use std::path::{Path, PathBuf};
use std::process::Command;

/// How often a rejected push is retried after a rebase.
const PUSH_ATTEMPTS: usize = 3;

/// A worktree of `branch` of the repository at `repository`, kept at `worktree`.
pub(crate) struct Branch<'a> {
    pub(crate) repository: &'a Path,
    pub(crate) remote: &'a str,
    pub(crate) branch: &'a str,
    pub(crate) worktree: PathBuf,
}

impl Branch<'_> {
    /// The worktree, reset to the remote branch, or a new orphan branch if the remote does not
    /// have it yet. Returns the worktree root, the shared folder.
    ///
    /// # Errors
    ///
    /// Returns Git's message when a Git command fails.
    pub(crate) fn checkout(&self) -> Result<&Path, String> {
        let listed = git(
            self.repository,
            &["ls-remote", "--heads", self.remote, self.branch],
        )?;
        let exists_remotely = !listed.trim().is_empty();
        let tracking = self.tracking();
        if exists_remotely {
            self.fetch()?;
        }
        if self.worktree.join(".git").exists() {
            if exists_remotely {
                git(
                    &self.worktree,
                    &["checkout", "--quiet", "-B", self.branch, &tracking],
                )?;
                git(&self.worktree, &["reset", "--quiet", "--hard", &tracking])?;
            }
            return Ok(&self.worktree);
        }
        if let Some(parent) = self.worktree.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        // A stale registration of a removed worktree would block `worktree add`.
        git(self.repository, &["worktree", "prune"])?;
        let worktree = self.worktree.display().to_string();
        let mut arguments = vec!["worktree", "add", "--quiet"];
        if exists_remotely {
            arguments.extend(["-B", self.branch, &worktree, &tracking]);
        } else {
            arguments.extend(["--orphan", "-B", self.branch, &worktree]);
        }
        git(self.repository, &arguments)?;
        Ok(&self.worktree)
    }

    /// Commits `path` (relative to the worktree) if it changed, and pushes the branch. A
    /// rejected push is retried after a rebase onto the remote branch.
    ///
    /// # Errors
    ///
    /// Returns Git's message when a Git command fails or every push is rejected.
    pub(crate) fn publish(&self, path: &str, message: &str) -> Result<(), String> {
        git(&self.worktree, &["add", "--all", "--", path])?;
        let staged = git(&self.worktree, &["diff", "--cached", "--name-only"])?;
        if !staged.trim().is_empty() {
            git(&self.worktree, &["commit", "--quiet", "-m", message])?;
        }
        let refspec = format!("HEAD:refs/heads/{}", self.branch);
        let mut last = String::new();
        for _ in 0..PUSH_ATTEMPTS {
            match git(&self.worktree, &["push", "--quiet", self.remote, &refspec]) {
                Ok(_) => return Ok(()),
                Err(message) => last = message,
            }
            self.fetch()?;
            git(&self.worktree, &["rebase", "--quiet", &self.tracking()])?;
        }
        Err(last)
    }

    // A named ref, unlike `FETCH_HEAD`, is shared by the repository and all its worktrees.
    fn tracking(&self) -> String {
        format!("refs/remotes/{}/{}", self.remote, self.branch)
    }

    fn fetch(&self) -> Result<(), String> {
        let refspec = format!("+refs/heads/{}:{}", self.branch, self.tracking());
        git(
            self.repository,
            &["fetch", "--quiet", self.remote, &refspec],
        )
        .map(drop)
    }
}

/// The worktree directory of `branch` below `store`: `/` in the branch name becomes `-`.
pub(crate) fn worktree_of(store: &Path, branch: &str) -> PathBuf {
    store.join("git").join(branch.replace('/', "-"))
}

fn git(directory: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .output()
        .map_err(|error| format!("git cannot be run: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "git {}: {}",
            arguments.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}
