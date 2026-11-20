//! The editor a project's files open in at a line. This is tool behaviour of the GUI: the
//! setting lives in `gui.json`, and the editor runs as a program with arguments, never through
//! a shell, so a file name cannot become a command.

use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

/// An editor, as a preset or as a program with arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Ide {
    /// Visual Studio Code: `code --goto <file>:<line>`.
    Vscode,
    /// Zed: `zed <file>:<line>`.
    Zed,
    /// An IDE of JetBrains: `<launcher> --line <line> <file>`.
    Jetbrains {
        /// The launcher script, for example `idea`, `rustrover`, or `pycharm`.
        launcher: String,
    },
    /// A program and its arguments, in which `{file}` and `{line}` are replaced.
    Custom { argv: Vec<String> },
}

impl Ide {
    /// The program and arguments with `{file}` and `{line}` in place of the file and the line.
    #[must_use]
    pub fn template(&self) -> Vec<String> {
        let words: &[&str] = match self {
            Self::Vscode => &["code", "--goto", "{file}:{line}"],
            Self::Zed => &["zed", "{file}:{line}"],
            Self::Jetbrains { launcher } => {
                return [launcher.as_str(), "--line", "{line}", "{file}"]
                    .map(str::to_owned)
                    .to_vec();
            }
            Self::Custom { argv } => return argv.clone(),
        };
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    /// What is wrong with the setting, if anything.
    #[must_use]
    pub fn problem(&self) -> Option<String> {
        let template = self.template();
        match template.first() {
            None => Some("name the program on the first line".to_owned()),
            Some(program) if program.trim().is_empty() => {
                Some("the program must not be empty".to_owned())
            }
            Some(_) if !template.iter().any(|word| word.contains("{file}")) => {
                Some("an argument must contain {file}".to_owned())
            }
            Some(_) => None,
        }
    }

    /// Opens `path` (relative to `root`) at `line` (from 1).
    ///
    /// # Errors
    ///
    /// Returns a message for a path that is no regular file below the root (a symbolic link
    /// may point anywhere), a setting with a problem, or a program that does not start.
    pub fn open(&self, root: &Path, path: &str, line: usize) -> Result<(), String> {
        let argv = self.arguments(root, path, line)?;
        let (program, arguments) = argv
            .split_first()
            .expect("a checked setting names a program");
        Command::new(program)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(drop)
            .map_err(|error| format!("{program} did not start: {error}"))
    }

    fn arguments(&self, root: &Path, path: &str, line: usize) -> Result<Vec<String>, String> {
        if let Some(problem) = self.problem() {
            return Err(problem);
        }
        let outside = || format!("{path} is no file below the project root");
        let root = root.canonicalize().map_err(|_| outside())?;
        let file = root.join(path).canonicalize().map_err(|_| outside())?;
        if !file.starts_with(&root) || !file.is_file() {
            return Err(outside());
        }
        let file = file.to_str().ok_or_else(outside)?;
        let line = line.max(1).to_string();
        Ok(self
            .template()
            .iter()
            .map(|word| word.replace("{file}", file).replace("{line}", &line))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::Ide;

    fn project(name: &str) -> std::path::PathBuf {
        let directory =
            std::env::temp_dir().join(format!("hone-gui-ide-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(directory.join("root/src")).expect("creatable");
        std::fs::write(directory.join("root/src/fee.rs"), "fn add() {}\n").expect("writable");
        std::fs::write(directory.join("secret.rs"), "").expect("writable");
        directory
    }

    #[test]
    fn presets_and_templates_put_the_file_and_the_line_in_their_arguments() {
        let directory = project("arguments");
        let root = directory.join("root");
        let file = root.join("src/fee.rs").canonicalize().expect("canonical");
        let file = file.to_str().expect("UTF-8");
        let arguments = |ide: Ide, line| ide.arguments(&root, "src/fee.rs", line).expect("valid");
        assert_eq!(
            arguments(Ide::Vscode, 12),
            ["code", "--goto", &format!("{file}:12")]
        );
        assert_eq!(arguments(Ide::Zed, 12), ["zed", &format!("{file}:12")]);
        assert_eq!(
            arguments(
                Ide::Jetbrains {
                    launcher: "rustrover".into()
                },
                12
            ),
            ["rustrover", "--line", "12", file]
        );
        let custom = Ide::Custom {
            argv: vec!["nvim".into(), "+{line}".into(), "{file}".into()],
        };
        assert_eq!(arguments(custom, 0), ["nvim", "+1", file]);
        std::fs::remove_dir_all(&directory).expect("removable");
    }

    #[test]
    fn a_file_outside_the_root_or_a_broken_setting_opens_nothing() {
        let directory = project("refused");
        let root = directory.join("root");
        #[cfg(unix)]
        std::os::unix::fs::symlink(directory.join("secret.rs"), root.join("src/link.rs"))
            .expect("linkable");
        assert_eq!(
            Ide::Zed.arguments(&root, "../secret.rs", 1),
            Err("../secret.rs is no file below the project root".to_owned())
        );
        #[cfg(unix)]
        assert_eq!(
            Ide::Zed.arguments(&root, "src/link.rs", 1),
            Err("src/link.rs is no file below the project root".to_owned())
        );
        assert_eq!(
            Ide::Zed.arguments(&root, "src", 1),
            Err("src is no file below the project root".to_owned())
        );
        let no_file = Ide::Custom {
            argv: vec!["vim".into()],
        };
        assert_eq!(
            no_file.arguments(&root, "src/fee.rs", 1),
            Err("an argument must contain {file}".to_owned())
        );
        assert_eq!(
            Ide::Custom { argv: Vec::new() }.problem(),
            Some("name the program on the first line".to_owned())
        );
        std::fs::remove_dir_all(&directory).expect("removable");
    }
}
