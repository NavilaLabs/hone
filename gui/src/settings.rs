//! The settings of the GUI per project and checkout, in `gui.json` of the hone configuration
//! directory (concept Section 12, "Desktop GUI"). This is tool behaviour: nothing lands in the
//! project, and a setting made in one checkout does not apply in another.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ide::Ide;
use crate::tracker::Tracker;

/// The settings file of the GUI.
#[derive(Debug, Clone)]
pub struct Settings {
    path: PathBuf,
}

/// What the GUI keeps for one project in one checkout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSettings {
    /// Where the project's tickets live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracker: Option<Tracker>,
    /// The editor that opens a file at a line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ide: Option<Ide>,
    /// The packages directory, passed as `--packages` to the commands that resolve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packages: Option<PathBuf>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    projects: Vec<Binding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Binding {
    project: String,
    root: PathBuf,
    #[serde(flatten)]
    settings: ProjectSettings,
}

impl Settings {
    /// `gui.json` in `HONE_CONFIG_DIR`, otherwise in the platform's configuration directory
    /// followed by `hone`, the directory of `contexts.yaml`.
    #[must_use]
    pub fn locate() -> Option<Self> {
        let directory = std::env::var_os("HONE_CONFIG_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| platform_config_directory().map(|directory| directory.join("hone")))?;
        Some(Self::at(directory.join("gui.json")))
    }

    /// The settings file `path`.
    #[must_use]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// The file's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The settings of `project` in the checkout `root`, empty when none are kept.
    ///
    /// # Errors
    ///
    /// Returns a message when the file exists but cannot be read, or the root does not exist.
    pub fn project(&self, project: &str, root: &Path) -> Result<ProjectSettings, String> {
        let root = canonical(root)?;
        Ok(self
            .read()?
            .projects
            .into_iter()
            .find(|binding| binding.project == project && binding.root == root)
            .map(|binding| binding.settings)
            .unwrap_or_default())
    }

    /// Changes the settings of `project` in the checkout `root` with `change`. Settings that end
    /// up empty are removed from the file.
    ///
    /// # Errors
    ///
    /// Returns a message when the file cannot be read or written.
    pub fn update(
        &self,
        project: &str,
        root: &Path,
        change: impl FnOnce(&mut ProjectSettings),
    ) -> Result<(), String> {
        let root = canonical(root)?;
        let mut file = self.read()?;
        let index = file
            .projects
            .iter()
            .position(|binding| binding.project == project && binding.root == root);
        let mut settings = index
            .map(|index| file.projects[index].settings.clone())
            .unwrap_or_default();
        change(&mut settings);
        match (index, settings == ProjectSettings::default()) {
            (Some(index), true) => {
                file.projects.remove(index);
            }
            (Some(index), false) => file.projects[index].settings = settings,
            (None, true) => {}
            (None, false) => file.projects.push(Binding {
                project: project.to_owned(),
                root,
                settings,
            }),
        }
        self.write(&file)
    }

    fn read(&self) -> Result<File, String> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("{}: {error}", self.path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(File::default()),
            Err(error) => Err(format!("{}: {error}", self.path.display())),
        }
    }

    fn write(&self, file: &File) -> Result<(), String> {
        let text = serde_json::to_string_pretty(file).map_err(|error| error.to_string())? + "\n";
        let write = |error: std::io::Error| format!("{}: {error}", self.path.display());
        if let Some(directory) = self.path.parent() {
            std::fs::create_dir_all(directory).map_err(write)?;
        }
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, text).map_err(write)?;
        std::fs::rename(&temporary, &self.path).map_err(write)
    }
}

fn canonical(root: &Path) -> Result<PathBuf, String> {
    root.canonicalize()
        .map_err(|error| format!("{}: {error}", root.display()))
}

#[cfg(target_os = "macos")]
fn platform_config_directory() -> Option<PathBuf> {
    std::env::home_dir().map(|home| home.join("Library/Application Support"))
}

#[cfg(windows)]
fn platform_config_directory() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn platform_config_directory() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|directory| directory.is_absolute())
        .or_else(|| std::env::home_dir().map(|home| home.join(".config")))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn github() -> Tracker {
        Tracker::Github {
            owner: "acme".into(),
            repo: "payments".into(),
            token_env: None,
        }
    }

    #[test]
    fn settings_apply_only_to_their_project_and_checkout() {
        let directory =
            std::env::temp_dir().join(format!("hone-gui-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let first = directory.join("first");
        let second = directory.join("second");
        std::fs::create_dir_all(&first).expect("creatable");
        std::fs::create_dir_all(&second).expect("creatable");
        let settings = Settings::at(directory.join("config/gui.json"));
        let project = "prj-8m2k4d7q";

        assert_eq!(
            settings.project(project, &first),
            Ok(ProjectSettings::default())
        );
        settings
            .update(project, &first, |kept| kept.tracker = Some(github()))
            .expect("written");
        settings
            .update(project, &first, |kept| {
                kept.ide = Some(Ide::Zed);
                kept.packages = Some(PathBuf::from("/opt/hone-packages"));
            })
            .expect("written");
        let expected = ProjectSettings {
            tracker: Some(github()),
            ide: Some(Ide::Zed),
            packages: Some(PathBuf::from("/opt/hone-packages")),
        };
        assert_eq!(settings.project(project, &first), Ok(expected));
        assert_eq!(
            settings.project(project, &second),
            Ok(ProjectSettings::default())
        );
        assert_eq!(
            settings.project("prj-2b7n5k1q", &first),
            Ok(ProjectSettings::default())
        );
        let written: Value =
            serde_json::from_slice(&std::fs::read(settings.path()).expect("readable"))
                .expect("JSON");
        assert_eq!(
            written,
            json!({ "projects": [{
                "project": project,
                "root": first.canonicalize().expect("canonical"),
                "tracker": { "kind": "github", "owner": "acme", "repo": "payments" },
                "ide": { "kind": "zed" },
                "packages": "/opt/hone-packages"
            }] })
        );
        settings
            .update(project, &first, |kept| *kept = ProjectSettings::default())
            .expect("written");
        let written: Value =
            serde_json::from_slice(&std::fs::read(settings.path()).expect("readable"))
                .expect("JSON");
        assert_eq!(written, json!({ "projects": [] }));
        std::fs::remove_dir_all(&directory).expect("removable");
    }
}
