//! Where a project's tickets live outside hone, and a search over them (concept Section 12,
//! "Desktop GUI"). This is tool behaviour: the setting lives on this device in `gui.json` of the
//! hone configuration directory, keyed by project ID and canonical root, and the token comes from
//! an environment variable or the OS keyring, never from a file.

use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const KEYRING_SERVICE: &str = "hone";
const GITHUB_API: &str = "https://api.github.com";
const PAGE: &str = "50";

/// The tracker of a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Tracker {
    /// The issues of a GitHub repository.
    Github {
        owner: String,
        repo: String,
        /// The variable that holds the token, `GITHUB_TOKEN` when not set.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token_env: Option<String>,
    },
    /// The issues of a Jira project.
    Jira {
        /// For example `https://acme.atlassian.net`.
        base_url: String,
        /// For example `PAY`.
        project_key: String,
        /// The account of Jira Cloud. With it, the token is an API token sent with the
        /// account; without it, a personal access token of Jira Server or Data Center.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        email: Option<String>,
        /// The variable that holds the token, `JIRA_API_TOKEN` when not set.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token_env: Option<String>,
    },
}

/// A ticket of a tracker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// The key a ticket record names in `external`: the number for GitHub, `PAY-123` for Jira.
    pub key: String,
    pub title: String,
    /// For example `open`, `closed`, or a Jira status.
    pub state: String,
    pub url: String,
}

/// Where a token comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSource {
    Environment(String),
    Keyring,
    None,
}

impl Tracker {
    /// The value of `system` in a ticket record's `external`.
    #[must_use]
    pub fn system(&self) -> &'static str {
        match self {
            Self::Github { .. } => "github",
            Self::Jira { .. } => "jira",
        }
    }

    /// A short name, for example `github acme/payments` or `jira PAY`.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Github { owner, repo, .. } => format!("github {owner}/{repo}"),
            Self::Jira { project_key, .. } => format!("jira {project_key}"),
        }
    }

    /// The variable the token is read from.
    #[must_use]
    pub fn token_variable(&self) -> String {
        match self {
            Self::Github { token_env, .. } => token_env.clone(),
            Self::Jira { token_env, .. } => token_env.clone(),
        }
        .filter(|variable| !variable.is_empty())
        .unwrap_or_else(|| {
            match self {
                Self::Github { .. } => "GITHUB_TOKEN",
                Self::Jira { .. } => "JIRA_API_TOKEN",
            }
            .to_owned()
        })
    }

    /// The address of the issue `key`.
    #[must_use]
    pub fn issue_url(&self, key: &str) -> String {
        match self {
            Self::Github { owner, repo, .. } => {
                format!("https://github.com/{owner}/{repo}/issues/{key}")
            }
            Self::Jira { base_url, .. } => {
                format!("{}/browse/{key}", base_url.trim_end_matches('/'))
            }
        }
    }

    /// Whether `system` and `key` of a ticket record's `external` name `issue` of this tracker.
    #[must_use]
    pub fn names(&self, system: &str, key: &str, issue: &str) -> bool {
        system.eq_ignore_ascii_case(self.system())
            && match self {
                Self::Github { .. } => key.trim_start_matches('#') == issue,
                Self::Jira { .. } => key.eq_ignore_ascii_case(issue),
            }
    }

    /// The line to add under `external` of a ticket record to link `issue`.
    #[must_use]
    pub fn external_entry(&self, issue: &Issue) -> String {
        format!(
            "- {{ system: \"{}\", key: \"{}\", url: \"{}\" }}",
            self.system(),
            issue.key,
            issue.url
        )
    }

    /// Why the setting cannot be used, or `None` when it can.
    #[must_use]
    pub fn problem(&self) -> Option<String> {
        let name = |text: &str| {
            !text.is_empty()
                && text
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        };
        match self {
            Self::Github { owner, repo, .. } => (!name(owner) || !name(repo))
                .then(|| "owner and repository are names like acme and payments".to_owned()),
            Self::Jira {
                base_url,
                project_key,
                ..
            } => {
                if !(base_url.starts_with("https://") || base_url.starts_with("http://"))
                    || Url::parse(base_url).is_err()
                {
                    Some("the base URL is an address like https://acme.atlassian.net".to_owned())
                } else if !project_key
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                    || project_key.is_empty()
                {
                    Some("the project key is upper case, like PAY".to_owned())
                } else {
                    None
                }
            }
        }
    }

    fn keyring_account(&self) -> String {
        match self {
            Self::Github { .. } => format!("tracker {GITHUB_API}"),
            Self::Jira { base_url, .. } => format!("tracker {}", base_url.trim_end_matches('/')),
        }
    }

    /// Where the token would come from now: the variable when it is set and not empty,
    /// otherwise the keyring.
    #[must_use]
    pub fn token_source(&self) -> TokenSource {
        let variable = self.token_variable();
        if std::env::var(&variable).is_ok_and(|token| !token.is_empty()) {
            return TokenSource::Environment(variable);
        }
        match self.stored_token() {
            Ok(Some(_)) => TokenSource::Keyring,
            _ => TokenSource::None,
        }
    }

    fn token(&self) -> Result<Option<String>, String> {
        if let Ok(token) = std::env::var(self.token_variable())
            && !token.is_empty()
        {
            return Ok(Some(token));
        }
        self.stored_token()
    }

    fn stored_token(&self) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, &self.keyring_account())
            .map_err(|error| error.to_string())?;
        match entry.get_password() {
            Ok(token) => Ok(Some(token).filter(|token| !token.is_empty())),
            Err(keyring::Error::NoEntry | keyring::Error::NoDefaultStore) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    /// Stores `token` in the OS keyring for this tracker.
    ///
    /// # Errors
    ///
    /// Returns the keyring's message when it cannot store the token.
    pub fn store_token(&self, token: &str) -> Result<(), String> {
        keyring::Entry::new(KEYRING_SERVICE, &self.keyring_account())
            .and_then(|entry| entry.set_password(token))
            .map_err(|error| format!("the OS keyring cannot store the token: {error}"))
    }

    /// Removes the token of this tracker from the OS keyring.
    ///
    /// # Errors
    ///
    /// Returns the keyring's message when it cannot remove the token.
    pub fn remove_token(&self) -> Result<(), String> {
        match keyring::Entry::new(KEYRING_SERVICE, &self.keyring_account())
            .and_then(|entry| entry.delete_credential())
        {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(format!("the OS keyring cannot remove the token: {error}")),
        }
    }

    /// The tickets that match `query`, or the most recent open ones for an empty query.
    ///
    /// # Errors
    ///
    /// Returns a message for a person when the tracker cannot be reached or refuses.
    pub async fn search(&self, query: &str) -> Result<Vec<Issue>, String> {
        let token = self.token()?;
        let request = self.request(query.trim(), token.as_deref())?;
        let client = reqwest::Client::builder()
            .user_agent(concat!("hone-gui/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|error| error.to_string())?;
        let mut builder = client
            .get(request.url.clone())
            .header("Accept", "application/json");
        if let Some(authorization) = request.authorization {
            builder = builder.header("Authorization", authorization);
        }
        let response = builder.send().await.map_err(|error| {
            format!(
                "{} cannot be reached: {error}",
                request.url.origin().ascii_serialization()
            )
        })?;
        let status = response.status();
        let body = response.bytes().await.map_err(|error| error.to_string())?;
        if !status.is_success() {
            return Err(refusal(status.as_u16(), &body, &self.token_variable()));
        }
        let value: Value = serde_json::from_slice(&body)
            .map_err(|error| format!("the tracker's answer is not JSON: {error}"))?;
        Ok(self.issues(&value, request.single))
    }

    /// The ticket `key` of the tracker, or `None` when the tracker does not know it.
    ///
    /// # Errors
    ///
    /// Returns a message for a person when the tracker cannot be reached or refuses.
    pub async fn issue(&self, key: &str) -> Result<Option<Issue>, String> {
        let key = key.trim_start_matches('#');
        let query = match self {
            Self::Github { .. } => format!("#{key}"),
            Self::Jira { .. } => key.to_owned(),
        };
        Ok(self
            .search(&query)
            .await?
            .into_iter()
            .find(|issue| issue.key.eq_ignore_ascii_case(key)))
    }

    fn request(&self, query: &str, token: Option<&str>) -> Result<Request, String> {
        match self {
            Self::Github { owner, repo, .. } => {
                let authorization = token.map(|token| format!("Bearer {token}"));
                let number = query.trim_start_matches('#');
                if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) {
                    let url = Url::parse(&format!(
                        "{GITHUB_API}/repos/{owner}/{repo}/issues/{number}"
                    ))
                    .map_err(invalid)?;
                    return Ok(Request {
                        url,
                        authorization,
                        single: true,
                    });
                }
                // The search leaves pull requests out, which the issue list of a repository
                // mixes in; without words it lists the open issues updated last.
                let words = if query.is_empty() { "is:open" } else { query };
                let url = Url::parse_with_params(
                    &format!("{GITHUB_API}/search/issues"),
                    [
                        (
                            "q",
                            format!("repo:{owner}/{repo} is:issue {words}").as_str(),
                        ),
                        ("sort", "updated"),
                        ("per_page", PAGE),
                    ],
                )
                .map_err(invalid)?;
                Ok(Request {
                    url,
                    authorization,
                    single: false,
                })
            }
            Self::Jira {
                base_url,
                project_key,
                email,
                ..
            } => {
                let token = token.ok_or_else(|| {
                    format!(
                        "no token for Jira: set {} or store one in the OS keyring below",
                        self.token_variable()
                    )
                })?;
                let mut jql = format!("project = \"{project_key}\"");
                if query.is_empty() {
                    jql.push_str(" AND statusCategory != Done");
                } else {
                    let quoted = query.replace('\\', "\\\\").replace('"', "\\\"");
                    if is_jira_key(query) {
                        jql.push_str(&format!(" AND (key = \"{quoted}\" OR text ~ \"{quoted}\")"));
                    } else {
                        jql.push_str(&format!(" AND text ~ \"{quoted}*\""));
                    }
                }
                jql.push_str(" ORDER BY updated DESC");
                let base = base_url.trim_end_matches('/');
                let (path, authorization) = match email {
                    Some(email) if !email.is_empty() => (
                        "rest/api/3/search/jql",
                        format!("Basic {}", base64(&format!("{email}:{token}"))),
                    ),
                    _ => ("rest/api/2/search", format!("Bearer {token}")),
                };
                let url = Url::parse_with_params(
                    &format!("{base}/{path}"),
                    [
                        ("jql", jql.as_str()),
                        ("fields", "summary,status"),
                        ("maxResults", PAGE),
                    ],
                )
                .map_err(invalid)?;
                Ok(Request {
                    url,
                    authorization: Some(authorization),
                    single: false,
                })
            }
        }
    }

    fn issues(&self, value: &Value, single: bool) -> Vec<Issue> {
        match self {
            Self::Github { .. } => {
                let items: Vec<&Value> = if single {
                    vec![value]
                } else {
                    value["items"].as_array().into_iter().flatten().collect()
                };
                items
                    .into_iter()
                    .filter(|item| item.get("pull_request").is_none())
                    .filter_map(|item| {
                        Some(Issue {
                            key: item["number"].as_u64()?.to_string(),
                            title: item["title"].as_str().unwrap_or_default().to_owned(),
                            state: item["state"].as_str().unwrap_or_default().to_owned(),
                            url: item["html_url"].as_str().unwrap_or_default().to_owned(),
                        })
                    })
                    .collect()
            }
            Self::Jira { .. } => value["issues"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| {
                    let key = item["key"].as_str()?.to_owned();
                    Some(Issue {
                        title: item["fields"]["summary"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                        state: item["fields"]["status"]["name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                        url: self.issue_url(&key),
                        key,
                    })
                })
                .collect(),
        }
    }
}

fn invalid(error: impl std::fmt::Display) -> String {
    format!("the tracker's address is invalid: {error}")
}

struct Request {
    url: Url,
    authorization: Option<String>,
    single: bool,
}

fn is_jira_key(text: &str) -> bool {
    let Some((project, number)) = text.split_once('-') else {
        return false;
    };
    project
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
        && project
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

fn refusal(status: u16, body: &[u8], variable: &str) -> String {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let message = value["message"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            value["errorMessages"]
                .as_array()
                .and_then(|messages| messages.first())
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default();
    let hint = match status {
        401 => format!(" Check the token in {variable} or the OS keyring."),
        403 | 429 => format!(" Without a token the rate limit is low: set {variable}."),
        404 => " Check the repository or project, and that the token may read it.".to_owned(),
        _ => String::new(),
    };
    format!("the tracker answered {status}: {message}{hint}")
        .replace(": .", ".")
        .trim_end()
        .to_owned()
}

fn base64(text: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = text.as_bytes();
    let mut encoded = String::new();
    for chunk in bytes.chunks(3) {
        let triple = chunk.iter().enumerate().fold(0u32, |acc, (index, byte)| {
            acc | u32::from(*byte) << (16 - 8 * index)
        });
        for index in 0..4 {
            if index <= chunk.len() {
                encoded.push(char::from(
                    ALPHABET[(triple >> (18 - 6 * index)) as usize & 63],
                ));
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

/// A GitHub repository suggested from the Git remote `origin` of `root`, as `(owner, repo)`.
pub async fn suggest_github(root: &Path) -> Option<(String, String)> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["remote", "get-url", "origin"])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| github_repository(String::from_utf8_lossy(&output.stdout).trim()))
        .flatten()
}

/// The owner and repository of a GitHub remote URL: `https://github.com/o/r(.git)`,
/// `git@github.com:o/r.git`, `ssh://git@github.com/o/r`, or an SSH host alias whose name
/// contains `github` and no dot (`git@github-work:o/r.git`).
#[must_use]
pub fn github_repository(remote: &str) -> Option<(String, String)> {
    let (host, path) = if let Some(rest) = remote
        .strip_prefix("https://")
        .or_else(|| remote.strip_prefix("http://"))
        .or_else(|| remote.strip_prefix("ssh://"))
        .or_else(|| remote.strip_prefix("git://"))
    {
        let rest = rest.rsplit_once('@').map_or(rest, |(_, rest)| rest);
        rest.split_once('/')?
    } else {
        let rest = remote.rsplit_once('@').map_or(remote, |(_, rest)| rest);
        rest.split_once(':')?
    };
    let host = host.split(':').next().unwrap_or(host).to_lowercase();
    let github = host == "github.com"
        || host == "www.github.com"
        || (host.contains("github") && !host.contains('.'));
    if !github {
        return None;
    }
    let mut parts = path.trim_matches('/').split('/');
    let owner = parts.next()?.to_owned();
    let repo = parts.next()?.trim_end_matches(".git").to_owned();
    (parts.next().is_none() && !owner.is_empty() && !repo.is_empty()).then_some((owner, repo))
}

/// The settings file of the GUI.
#[derive(Debug, Clone)]
pub struct Settings {
    path: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    trackers: Vec<Binding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Binding {
    project: String,
    root: PathBuf,
    tracker: Tracker,
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

    /// The tracker of `project` in the checkout `root`. A setting made in another checkout does
    /// not apply.
    ///
    /// # Errors
    ///
    /// Returns a message when the file exists but cannot be read.
    pub fn tracker(&self, project: &str, root: &Path) -> Result<Option<Tracker>, String> {
        let root = canonical(root)?;
        Ok(self
            .read()?
            .trackers
            .into_iter()
            .find(|binding| binding.project == project && binding.root == root)
            .map(|binding| binding.tracker))
    }

    /// Sets the tracker of `project` in the checkout `root`, or removes it with `None`.
    ///
    /// # Errors
    ///
    /// Returns a message when the file cannot be read or written.
    pub fn set_tracker(
        &self,
        project: &str,
        root: &Path,
        tracker: Option<Tracker>,
    ) -> Result<(), String> {
        let root = canonical(root)?;
        let mut file = self.read()?;
        file.trackers
            .retain(|binding| !(binding.project == project && binding.root == root));
        if let Some(tracker) = tracker {
            file.trackers.push(Binding {
                project: project.to_owned(),
                root,
                tracker,
            });
        }
        let text = serde_json::to_string_pretty(&file).map_err(|error| error.to_string())? + "\n";
        let write = |error: std::io::Error| format!("{}: {error}", self.path.display());
        if let Some(directory) = self.path.parent() {
            std::fs::create_dir_all(directory).map_err(write)?;
        }
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, text).map_err(write)?;
        std::fs::rename(&temporary, &self.path).map_err(write)
    }

    fn read(&self) -> Result<File, String> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("{}: {error}", self.path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(File::default()),
            Err(error) => Err(format!("{}: {error}", self.path.display())),
        }
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
    use serde_json::json;

    use super::*;

    fn github() -> Tracker {
        Tracker::Github {
            owner: "acme".into(),
            repo: "payments".into(),
            token_env: None,
        }
    }

    fn jira(email: Option<&str>) -> Tracker {
        Tracker::Jira {
            base_url: "https://acme.atlassian.net/".into(),
            project_key: "PAY".into(),
            email: email.map(str::to_owned),
            token_env: None,
        }
    }

    #[test]
    fn github_remotes_give_owner_and_repository() {
        let pair = Some(("acme".to_owned(), "payments".to_owned()));
        assert_eq!(
            github_repository("https://github.com/acme/payments.git"),
            pair
        );
        assert_eq!(
            github_repository("https://user@github.com/acme/payments"),
            pair
        );
        assert_eq!(github_repository("git@github.com:acme/payments.git"), pair);
        assert_eq!(
            github_repository("ssh://git@github.com:22/acme/payments"),
            pair
        );
        assert_eq!(github_repository("git@github-work:acme/payments.git"), pair);
        assert_eq!(github_repository("git@gitlab.com:acme/payments.git"), None);
        assert_eq!(
            github_repository("https://github.example.com/acme/payments"),
            None
        );
        assert_eq!(github_repository("https://github.com/acme"), None);
        assert_eq!(github_repository("/srv/git/payments.git"), None);
    }

    #[test]
    fn github_requests_list_search_or_fetch_one_issue() {
        let tracker = github();
        let list = tracker.request("", None).expect("a request");
        assert_eq!(
            list.url.as_str(),
            "https://api.github.com/search/issues?q=repo%3Aacme%2Fpayments+is%3Aissue+is%3Aopen&sort=updated&per_page=50"
        );
        assert_eq!(list.authorization, None);
        let search = tracker
            .request("queue fails", Some("t0k"))
            .expect("a request");
        assert_eq!(
            search.url.as_str(),
            "https://api.github.com/search/issues?q=repo%3Aacme%2Fpayments+is%3Aissue+queue+fails&sort=updated&per_page=50"
        );
        assert_eq!(search.authorization.as_deref(), Some("Bearer t0k"));
        let one = tracker.request("#12", None).expect("a request");
        assert_eq!(
            one.url.as_str(),
            "https://api.github.com/repos/acme/payments/issues/12"
        );
        assert!(one.single);
    }

    #[test]
    fn jira_cloud_sends_the_account_and_server_a_bearer_token() {
        let cloud = jira(Some("alice@acme.example"))
            .request("PAY-12", Some("t0k"))
            .expect("a request");
        assert_eq!(
            cloud.url.as_str(),
            "https://acme.atlassian.net/rest/api/3/search/jql?jql=project+%3D+%22PAY%22+AND+%28key+%3D+%22PAY-12%22+OR+text+%7E+%22PAY-12%22%29+ORDER+BY+updated+DESC&fields=summary%2Cstatus&maxResults=50"
        );
        // printf 'alice@acme.example:t0k' | base64
        assert_eq!(
            cloud.authorization.as_deref(),
            Some("Basic YWxpY2VAYWNtZS5leGFtcGxlOnQwaw==")
        );
        let server = jira(None)
            .request("say \"hi\"", Some("t0k"))
            .expect("a request");
        assert_eq!(
            server.url.as_str(),
            "https://acme.atlassian.net/rest/api/2/search?jql=project+%3D+%22PAY%22+AND+text+%7E+%22say+%5C%22hi%5C%22*%22+ORDER+BY+updated+DESC&fields=summary%2Cstatus&maxResults=50"
        );
        assert_eq!(server.authorization.as_deref(), Some("Bearer t0k"));
        let missing = jira(None).request("", None).err().expect("no token");
        assert_eq!(
            missing,
            "no token for Jira: set JIRA_API_TOKEN or store one in the OS keyring below"
        );
    }

    #[test]
    fn answers_become_issues_and_pull_requests_are_left_out() {
        let listed = json!({ "items": [
            { "number": 12, "title": "Queue fails", "state": "open", "html_url": "https://github.com/acme/payments/issues/12" },
            { "number": 13, "title": "A PR", "state": "open", "html_url": "u", "pull_request": {} }
        ] });
        assert_eq!(
            github().issues(&listed, false),
            [Issue {
                key: "12".into(),
                title: "Queue fails".into(),
                state: "open".into(),
                url: "https://github.com/acme/payments/issues/12".into()
            }]
        );
        let one = json!({ "number": 7, "title": "T", "state": "closed", "html_url": "v" });
        assert_eq!(github().issues(&one, true)[0].key, "7");
        let jira_answer = json!({ "issues": [{ "key": "PAY-12", "fields": { "summary": "Async", "status": { "name": "In Progress" } } }] });
        assert_eq!(
            jira(None).issues(&jira_answer, false),
            [Issue {
                key: "PAY-12".into(),
                title: "Async".into(),
                state: "In Progress".into(),
                url: "https://acme.atlassian.net/browse/PAY-12".into()
            }]
        );
    }

    #[test]
    fn external_entries_name_an_issue_of_their_tracker() {
        assert!(github().names("github", "#12", "12"));
        assert!(github().names("GitHub", "12", "12"));
        assert!(!github().names("jira", "12", "12"));
        assert!(jira(None).names("jira", "pay-12", "PAY-12"));
        let issue = Issue {
            key: "12".into(),
            title: String::new(),
            state: String::new(),
            url: "https://github.com/acme/payments/issues/12".into(),
        };
        assert_eq!(
            github().external_entry(&issue),
            "- { system: \"github\", key: \"12\", url: \"https://github.com/acme/payments/issues/12\" }"
        );
    }

    #[test]
    fn a_setting_needs_valid_names() {
        assert_eq!(github().problem(), None);
        assert_eq!(jira(None).problem(), None);
        let bad = Tracker::Github {
            owner: "acme corp".into(),
            repo: "payments".into(),
            token_env: None,
        };
        assert!(bad.problem().is_some());
        let lower = Tracker::Jira {
            base_url: "https://acme.atlassian.net".into(),
            project_key: "pay".into(),
            email: None,
            token_env: None,
        };
        assert_eq!(
            lower.problem().as_deref(),
            Some("the project key is upper case, like PAY")
        );
    }

    #[test]
    fn refusals_name_the_status_message_and_a_hint() {
        assert_eq!(
            refusal(401, br#"{"message":"Bad credentials"}"#, "GITHUB_TOKEN"),
            "the tracker answered 401: Bad credentials Check the token in GITHUB_TOKEN or the OS keyring."
        );
        assert_eq!(
            refusal(
                400,
                br#"{"errorMessages":["The value 'X' does not exist"]}"#,
                "JIRA_API_TOKEN"
            ),
            "the tracker answered 400: The value 'X' does not exist"
        );
    }

    #[test]
    fn a_setting_applies_only_to_its_project_and_checkout() {
        let directory =
            std::env::temp_dir().join(format!("hone-gui-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let first = directory.join("first");
        let second = directory.join("second");
        std::fs::create_dir_all(&first).expect("creatable");
        std::fs::create_dir_all(&second).expect("creatable");
        let settings = Settings::at(directory.join("config/gui.json"));

        assert_eq!(settings.tracker("prj-8m2k4d7q", &first), Ok(None));
        settings
            .set_tracker("prj-8m2k4d7q", &first, Some(github()))
            .expect("written");
        assert_eq!(settings.tracker("prj-8m2k4d7q", &first), Ok(Some(github())));
        assert_eq!(settings.tracker("prj-8m2k4d7q", &second), Ok(None));
        assert_eq!(settings.tracker("prj-2b7n5k1q", &first), Ok(None));
        let written: Value =
            serde_json::from_slice(&std::fs::read(settings.path()).expect("readable"))
                .expect("JSON");
        assert_eq!(
            written,
            json!({ "trackers": [{
                "project": "prj-8m2k4d7q",
                "root": first.canonicalize().expect("canonical"),
                "tracker": { "kind": "github", "owner": "acme", "repo": "payments" }
            }] })
        );
        settings
            .set_tracker("prj-8m2k4d7q", &first, None)
            .expect("written");
        assert_eq!(settings.tracker("prj-8m2k4d7q", &first), Ok(None));
        std::fs::remove_dir_all(&directory).expect("removable");
    }
}
