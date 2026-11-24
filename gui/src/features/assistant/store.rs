//! The chats the assistant can resume: `assistant.json` in hone's data directory. It is state,
//! not a setting, so it does not live in `gui.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::acp::AgentCommand;

/// The file of the resumable chats.
#[derive(Debug, Clone)]
pub(crate) struct ChatStore {
    path: PathBuf,
}

/// Whose chat: a ticket of a project in one checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatKey {
    pub(crate) project: String,
    /// The canonical root of the checkout.
    pub(crate) root: PathBuf,
    pub(crate) ticket: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    chats: Vec<Saved>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Saved {
    project: String,
    root: PathBuf,
    ticket: String,
    session: String,
    argv: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    env: BTreeMap<String, String>,
}

impl Saved {
    fn belongs_to(&self, key: &ChatKey) -> bool {
        self.project == key.project && self.root == key.root && self.ticket == key.ticket
    }
}

impl ChatStore {
    /// `assistant.json` in `HONE_DATA_DIR`, otherwise in the platform's data directory followed
    /// by `hone`, the directory of the process store.
    pub(crate) fn locate() -> Option<Self> {
        let directory = std::env::var_os("HONE_DATA_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| platform_data_directory().map(|directory| directory.join("hone")))?;
        Some(Self::at(directory.join("assistant.json")))
    }

    pub(crate) fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// The session of `key`'s chat and the command that started it.
    pub(crate) fn find(&self, key: &ChatKey) -> Result<Option<(String, AgentCommand)>, String> {
        Ok(self
            .read()?
            .chats
            .into_iter()
            .find(|saved| saved.belongs_to(key))
            .map(|saved| {
                (
                    saved.session,
                    AgentCommand {
                        argv: saved.argv,
                        env: saved.env,
                    },
                )
            }))
    }

    /// Keeps `session`, started by `command`, as `key`'s chat in place of an earlier one.
    pub(crate) fn save(
        &self,
        key: &ChatKey,
        session: &str,
        command: &AgentCommand,
    ) -> Result<(), String> {
        let mut file = self.read()?;
        file.chats.retain(|saved| !saved.belongs_to(key));
        file.chats.push(Saved {
            project: key.project.clone(),
            root: key.root.clone(),
            ticket: key.ticket.clone(),
            session: session.to_owned(),
            argv: command.argv.clone(),
            env: command.env.clone(),
        });
        self.write(&file)
    }

    /// Forgets `key`'s chat.
    pub(crate) fn forget(&self, key: &ChatKey) -> Result<(), String> {
        let mut file = self.read()?;
        let before = file.chats.len();
        file.chats.retain(|saved| !saved.belongs_to(key));
        if file.chats.len() == before {
            return Ok(());
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

impl ChatKey {
    pub(crate) fn new(project: String, root: &Path, ticket: String) -> Result<Self, String> {
        let root = root
            .canonicalize()
            .map_err(|error| format!("{}: {error}", root.display()))?;
        Ok(Self {
            project,
            root,
            ticket,
        })
    }
}

#[cfg(target_os = "macos")]
fn platform_data_directory() -> Option<PathBuf> {
    std::env::home_dir().map(|home| home.join("Library/Application Support"))
}

#[cfg(windows)]
fn platform_data_directory() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn platform_data_directory() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|directory| directory.is_absolute())
        .or_else(|| std::env::home_dir().map(|home| home.join(".local/share")))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn a_chat_is_kept_per_project_root_and_ticket_with_its_command() {
        let directory = std::env::temp_dir().join(format!("hone-gui-chats-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("creatable");
        let store = ChatStore::at(directory.join("data/assistant.json"));
        let key = |ticket: &str| ChatKey {
            project: "prj-8m2k4d7q".into(),
            root: PathBuf::from("/home/alice/payments"),
            ticket: ticket.into(),
        };
        let gemini = AgentCommand {
            argv: vec![
                "npx".into(),
                "-y".into(),
                "@google/gemini-cli@0.71.1".into(),
            ],
            env: BTreeMap::new(),
        };
        let fast = AgentCommand {
            argv: vec!["uvx".into(), "fast-agent-acp==0.10.66".into()],
            env: BTreeMap::from([("FAST_AGENT_MODEL".into(), "codexplan".into())]),
        };
        assert_eq!(store.find(&key("tk-9f2x4a8m")), Ok(None));
        store
            .save(&key("tk-9f2x4a8m"), "sess-1", &gemini)
            .expect("written");
        store
            .save(&key("tk-y4075sb1"), "sess-2", &gemini)
            .expect("written");
        store
            .save(&key("tk-9f2x4a8m"), "sess-3", &fast)
            .expect("written");
        assert_eq!(
            store.find(&key("tk-9f2x4a8m")),
            Ok(Some(("sess-3".into(), fast)))
        );
        let written: Value = serde_json::from_slice(
            &std::fs::read(directory.join("data/assistant.json")).expect("readable"),
        )
        .expect("JSON");
        assert_eq!(
            written,
            json!({ "chats": [
                {
                    "project": "prj-8m2k4d7q",
                    "root": "/home/alice/payments",
                    "ticket": "tk-y4075sb1",
                    "session": "sess-2",
                    "argv": ["npx", "-y", "@google/gemini-cli@0.71.1"]
                },
                {
                    "project": "prj-8m2k4d7q",
                    "root": "/home/alice/payments",
                    "ticket": "tk-9f2x4a8m",
                    "session": "sess-3",
                    "argv": ["uvx", "fast-agent-acp==0.10.66"],
                    "env": { "FAST_AGENT_MODEL": "codexplan" }
                }
            ] })
        );
        store.forget(&key("tk-9f2x4a8m")).expect("written");
        assert_eq!(store.find(&key("tk-9f2x4a8m")), Ok(None));
        assert_eq!(
            store.find(&key("tk-y4075sb1")),
            Ok(Some(("sess-2".into(), gemini)))
        );
        std::fs::remove_dir_all(&directory).expect("removable");
    }
}
