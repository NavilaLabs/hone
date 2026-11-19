use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::{Author, ProcessError};

/// The contexts of this device (concept Section 12, "Contexts"): per tenant a peer, an optional
/// author, and the environment variable of the token, and which project belongs to which
/// context. Kept in `contexts.yaml`; it holds no secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contexts {
    path: PathBuf,
    contexts: BTreeMap<String, Context>,
    projects: BTreeMap<String, Binding>,
}

// A project ID alone is chosen by the repository, so a binding also holds the canonical root of
// the checkout it was made in.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Binding {
    context: String,
    root: PathBuf,
}

/// One context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Context {
    /// Where the context's projects sync.
    pub peer: ContextPeer,
    /// The author of the events written in this context, if set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<Author>,
    /// The environment variable that holds the token for a server, `HONE_TOKEN` if unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_env: Option<String>,
}

/// The peer of a [`Context`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum ContextPeer {
    /// A replication server by base URL.
    Server {
        /// The base URL.
        server: String,
    },
    /// A shared folder.
    Folder {
        /// The folder root.
        folder: PathBuf,
    },
    /// A branch of the project's Git repository.
    Git {
        /// The branch.
        git: String,
        /// The remote.
        remote: String,
    },
}

/// The context bound to a project, as `hone context show` prints it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoundContext {
    /// The context's name.
    pub name: String,
    /// The canonical project root the binding was made in.
    pub root: PathBuf,
    /// The context.
    pub context: Context,
}

impl Contexts {
    /// The contexts of this device from `contexts.yaml` in `HONE_CONFIG_DIR`, otherwise in the
    /// platform's configuration directory followed by `hone`. A missing file has no contexts.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::IoError`] if no configuration directory can be determined or
    /// the file cannot be read, and [`ProcessError::Invalid`] if it is malformed.
    pub fn load() -> Result<Self, ProcessError> {
        let directory = config_directory().ok_or_else(|| ProcessError::IoError {
            message: "no configuration directory: set HONE_CONFIG_DIR".into(),
        })?;
        Self::load_from(&directory.join("contexts.yaml"))
    }

    fn load_from(path: &Path) -> Result<Self, ProcessError> {
        let mut contexts = Self {
            path: path.to_path_buf(),
            contexts: BTreeMap::new(),
            projects: BTreeMap::new(),
        };
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(contexts),
            Err(error) => return Err(contexts.io(&error)),
        };
        let malformed = |message: String| ProcessError::Invalid {
            message: format!("{}: {message}", path.display()),
        };
        let document = crate::yaml::parse(&bytes).map_err(|violations| {
            malformed(
                violations
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;
        let Value::Object(document) = document else {
            return Err(malformed("the file must be a map".into()));
        };
        for (key, value) in &document {
            match key.as_str() {
                "contexts" => {
                    for (name, entry) in value.as_object().into_iter().flatten() {
                        let context = parse_context(entry)
                            .map_err(|message| malformed(format!("context `{name}`: {message}")))?;
                        contexts.contexts.insert(name.clone(), context);
                    }
                }
                "projects" => {
                    for (project, binding) in value.as_object().into_iter().flatten() {
                        let (Some(name), Some(root)) =
                            (binding["context"].as_str(), binding["root"].as_str())
                        else {
                            return Err(malformed(format!(
                                "project `{project}` must be `{{ context, root }}`"
                            )));
                        };
                        contexts.projects.insert(
                            project.clone(),
                            Binding {
                                context: name.to_owned(),
                                root: PathBuf::from(root),
                            },
                        );
                    }
                }
                other => return Err(malformed(format!("unknown key `{other}`"))),
            }
        }
        if let Some((project, name)) = contexts
            .projects
            .iter()
            .find(|(_, binding)| !contexts.contexts.contains_key(&binding.context))
        {
            return Err(malformed(format!(
                "project `{project}` names the unknown context `{}`",
                name.context
            )));
        }
        Ok(contexts)
    }

    /// Creates or replaces the context `name`.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Invalid`] if `name` is not a lowercase key.
    pub fn set(&mut self, name: &str, context: Context) -> Result<(), ProcessError> {
        if !is_key(name) {
            return Err(ProcessError::Invalid {
                message: format!(
                    "`{name}` is not a context name: lowercase letters, digits, and `-`"
                ),
            });
        }
        self.contexts.insert(name.to_owned(), context);
        Ok(())
    }

    /// The context `name`, if it exists.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Context> {
        self.contexts.get(name)
    }

    /// Binds `project`, checked out at `root`, to the context `name`. A project has one
    /// binding; binding it again moves it to the new root.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Invalid`] if there is no context `name`, and
    /// [`ProcessError::IoError`] if `root` cannot be canonicalized.
    pub fn bind(&mut self, project: &str, root: &Path, name: &str) -> Result<(), ProcessError> {
        if !self.contexts.contains_key(name) {
            return Err(ProcessError::Invalid {
                message: format!("no context `{name}`: create it with hone context set"),
            });
        }
        let binding = Binding {
            context: name.to_owned(),
            root: canonical(root)?,
        };
        self.projects.insert(project.to_owned(), binding);
        Ok(())
    }

    /// Removes the binding of `project` and returns the name of the context it was bound to,
    /// or `None` if it had none. The context itself stays.
    pub fn unbind(&mut self, project: &str) -> Option<String> {
        self.projects.remove(project).map(|binding| binding.context)
    }

    /// The context bound to `project` checked out at `root`, or `None` if the project has no
    /// binding.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Invalid`] if the project is bound in another checkout, because
    /// a repository chooses its project ID itself and could otherwise claim another project's
    /// context, and [`ProcessError::IoError`] if `root` cannot be canonicalized.
    pub fn of_project(
        &self,
        project: &str,
        root: &Path,
    ) -> Result<Option<BoundContext>, ProcessError> {
        let Some(binding) = self.projects.get(project) else {
            return Ok(None);
        };
        let here = canonical(root)?;
        if here != binding.root {
            return Err(ProcessError::Invalid {
                message: format!(
                    "the project `{project}` is bound to the context `{}` in {}, not in {}: bind this checkout with hone context use",
                    binding.context,
                    binding.root.display(),
                    here.display()
                ),
            });
        }
        Ok(self
            .contexts
            .get(&binding.context)
            .map(|context| BoundContext {
                name: binding.context.clone(),
                root: binding.root.clone(),
                context: context.clone(),
            }))
    }

    /// Writes the file anew, creating its directory.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::IoError`] if it cannot be written.
    pub fn save(&self) -> Result<(), ProcessError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| self.io(&error))?;
        }
        std::fs::write(&self.path, self.render()).map_err(|error| self.io(&error))
    }

    // Double-quoted strings and flow maps keep the file inside the strict YAML profile.
    fn render(&self) -> String {
        let quote = |text: &str| serde_json::to_string(text).expect("a string serializes");
        let mut text = String::from("contexts:\n");
        if self.contexts.is_empty() {
            text = "contexts: {}\n".into();
        }
        for (name, context) in &self.contexts {
            text.push_str(&format!("  {name}:\n"));
            let peer = match &context.peer {
                ContextPeer::Server { server } => format!("{{ server: {} }}", quote(server)),
                ContextPeer::Folder { folder } => {
                    format!("{{ folder: {} }}", quote(&folder.display().to_string()))
                }
                ContextPeer::Git { git, remote } => {
                    format!("{{ git: {}, remote: {} }}", quote(git), quote(remote))
                }
            };
            text.push_str(&format!("    peer: {peer}\n"));
            if let Some(author) = &context.author {
                text.push_str(&format!(
                    "    author: {{ name: {}, role: {} }}\n",
                    quote(&author.name),
                    quote(&author.role)
                ));
            }
            if let Some(variable) = &context.token_env {
                text.push_str(&format!("    token_env: {}\n", quote(variable)));
            }
        }
        if self.projects.is_empty() {
            text.push_str("projects: {}\n");
        } else {
            text.push_str("projects:\n");
            for (project, binding) in &self.projects {
                text.push_str(&format!(
                    "  {project}: {{ context: {}, root: {} }}\n",
                    quote(&binding.context),
                    quote(&binding.root.display().to_string())
                ));
            }
        }
        text
    }

    fn io(&self, error: &std::io::Error) -> ProcessError {
        ProcessError::IoError {
            message: format!("{}: {error}", self.path.display()),
        }
    }
}

fn canonical(root: &Path) -> Result<PathBuf, ProcessError> {
    root.canonicalize().map_err(|error| ProcessError::IoError {
        message: format!("{}: {error}", root.display()),
    })
}

fn parse_context(entry: &Value) -> Result<Context, String> {
    let map = entry.as_object().ok_or("must be a map")?;
    if let Some(key) = map
        .keys()
        .find(|key| !["peer", "author", "token_env"].contains(&key.as_str()))
    {
        return Err(format!("unknown key `{key}`"));
    }
    let text = |value: &Value, what: &str| {
        value
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("`{what}` must be text"))
    };
    let peer = &map.get("peer").ok_or("`peer` is missing")?;
    let peer = match (peer.get("server"), peer.get("folder"), peer.get("git")) {
        (Some(server), None, None) => ContextPeer::Server {
            server: text(server, "server")?,
        },
        (None, Some(folder), None) => ContextPeer::Folder {
            folder: PathBuf::from(text(folder, "folder")?),
        },
        (None, None, Some(git)) => ContextPeer::Git {
            git: text(git, "git")?,
            remote: match peer.get("remote") {
                Some(remote) => text(remote, "remote")?,
                None => "origin".into(),
            },
        },
        _ => return Err("`peer` must hold exactly one of `server`, `folder`, `git`".into()),
    };
    let author = match map.get("author") {
        None => None,
        Some(author) => Some(Author {
            name: text(&author["name"], "author.name")?,
            role: text(&author["role"], "author.role")?,
        }),
    };
    let token_env = match map.get("token_env") {
        None => None,
        Some(variable) => Some(text(variable, "token_env")?),
    };
    Ok(Context {
        peer,
        author,
        token_env,
    })
}

fn is_key(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

fn config_directory() -> Option<PathBuf> {
    if let Some(directory) = std::env::var_os("HONE_CONFIG_DIR").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(directory));
    }
    platform_config_directory().map(|directory| directory.join("hone"))
}

#[cfg(target_os = "macos")]
fn platform_config_directory() -> Option<PathBuf> {
    std::env::home_dir().map(|home| home.join("Library").join("Application Support"))
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
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("hone-contexts-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        directory
    }

    #[test]
    fn a_saved_file_reads_back_the_same() {
        let directory = scratch("round-trip");
        let path = directory.join("contexts.yaml");
        let mut contexts = Contexts::load_from(&path).expect("a missing file is empty");
        contexts
            .set(
                "acme",
                Context {
                    peer: ContextPeer::Git {
                        git: "hone/process".into(),
                        remote: "origin".into(),
                    },
                    author: Some(Author {
                        name: "alice \"a\"".into(),
                        role: "architect".into(),
                    }),
                    token_env: Some("HONE_TOKEN_ACME".into()),
                },
            )
            .expect("a valid name");
        let checkout = directory.join("checkout");
        std::fs::create_dir_all(checkout.join("sub")).expect("creatable");
        contexts
            .bind("prj-8m2k4d7q", &checkout.join("sub/.."), "acme")
            .expect("known");
        contexts.save().expect("written");
        let root = checkout.canonicalize().expect("canonical");
        assert_eq!(
            std::fs::read_to_string(&path).expect("readable"),
            format!(
                "contexts:\n  acme:\n    peer: {{ git: \"hone/process\", remote: \"origin\" }}\n    author: {{ name: \"alice \\\"a\\\"\", role: \"architect\" }}\n    token_env: \"HONE_TOKEN_ACME\"\nprojects:\n  prj-8m2k4d7q: {{ context: \"acme\", root: {} }}\n",
                serde_json::to_string(&root.display().to_string()).expect("a string")
            )
        );
        let read = Contexts::load_from(&path).expect("readable");
        assert_eq!(read, contexts);
        let bound = read
            .of_project("prj-8m2k4d7q", &checkout)
            .expect("same root");
        assert_eq!(bound.map(|bound| bound.name).as_deref(), Some("acme"));
        let elsewhere = read.of_project("prj-8m2k4d7q", &directory);
        assert!(
            matches!(elsewhere, Err(ProcessError::Invalid { .. })),
            "another checkout with the same project ID gets no context: {elsewhere:?}"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn malformed_files_and_names_are_rejected() {
        let directory = scratch("malformed");
        std::fs::create_dir_all(&directory).expect("creatable");
        let path = directory.join("contexts.yaml");
        let message = |text: &str| {
            std::fs::write(&path, text).expect("writable");
            match Contexts::load_from(&path) {
                Err(ProcessError::Invalid { message }) => message
                    .trim_start_matches(&format!("{}: ", path.display()))
                    .to_owned(),
                other => panic!("expected an invalid file, got {other:?}"),
            }
        };
        assert_eq!(
            message("contexts:\n  a:\n    peer: { server: \"x\", folder: \"y\" }\n"),
            "context `a`: `peer` must hold exactly one of `server`, `folder`, `git`"
        );
        assert_eq!(
            message(
                "contexts: {}\nprojects:\n  prj-8m2k4d7q: { context: \"ghost\", root: \"/x\" }\n"
            ),
            "project `prj-8m2k4d7q` names the unknown context `ghost`"
        );
        assert_eq!(message("token: \"secret\"\n"), "unknown key `token`");
        let mut contexts = Contexts::load_from(&directory.join("none.yaml")).expect("empty");
        let peer = ContextPeer::Folder {
            folder: "/x".into(),
        };
        let error = contexts
            .set(
                "Acme",
                Context {
                    peer,
                    author: None,
                    token_env: None,
                },
            )
            .expect_err("uppercase");
        assert!(matches!(error, ProcessError::Invalid { .. }));
        let _ = std::fs::remove_dir_all(&directory);
    }
}
