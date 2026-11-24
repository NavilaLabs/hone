//! Setting up the assistant's agent: a choice from the ACP Registry or a custom command. A
//! choice only fills the fields; nothing starts before Save.

use std::collections::BTreeMap;
use std::time::Duration;

use dioxus::prelude::*;
use serde_json::Value;

use super::Assistant;
use crate::acp::AgentCommand;
use crate::features::common::{Field, ProblemAlert};
use crate::settings::{AssistantSettings, Settings};
use crate::ui::{Button, ButtonVariant, Choice, Combobox, Input, Textarea};

const REGISTRY: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";

/// An agent of the registry that starts through `npx` or `uvx`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistryAgent {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) description: String,
    pub(crate) command: AgentCommand,
}

/// The agents of `registry` that start through `npx` (preferred) or `uvx`, by name. An agent
/// distributed only as an archive to unpack is left out.
pub(crate) fn registry_agents(registry: &Value) -> Vec<RegistryAgent> {
    let npx = if cfg!(windows) { "npx.cmd" } else { "npx" };
    let mut agents: Vec<RegistryAgent> = registry["agents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|agent| {
            let distribution = &agent["distribution"];
            let (mut argv, package) = if distribution["npx"].is_object() {
                // `-y`: npx would otherwise ask on standard input, which is the agent's channel.
                (vec![npx.to_owned(), "-y".to_owned()], &distribution["npx"])
            } else if distribution["uvx"].is_object() {
                (vec!["uvx".to_owned()], &distribution["uvx"])
            } else {
                return None;
            };
            argv.push(package["package"].as_str()?.to_owned());
            argv.extend(
                package["args"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|argument| argument.as_str().map(str::to_owned)),
            );
            let env = package["env"]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_owned())))
                .collect();
            Some(RegistryAgent {
                id: agent["id"].as_str()?.to_owned(),
                name: agent["name"].as_str()?.to_owned(),
                version: agent["version"].as_str().unwrap_or_default().to_owned(),
                description: agent["description"].as_str().unwrap_or_default().to_owned(),
                command: AgentCommand { argv, env },
            })
        })
        .collect();
    agents.sort_by_key(|agent| agent.name.to_lowercase());
    agents
}

async fn fetch_registry() -> Result<Vec<RegistryAgent>, String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("hone-gui/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(REGISTRY)
        .send()
        .await
        .map_err(|error| format!("the ACP Registry cannot be reached: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("the ACP Registry answers {}", response.status()));
    }
    let body = response.bytes().await.map_err(|error| error.to_string())?;
    let registry: Value = serde_json::from_slice(&body)
        .map_err(|error| format!("the ACP Registry is not understood: {error}"))?;
    Ok(registry_agents(&registry))
}

/// The command and environment the form's text describes: one argument per line, and one
/// `NAME=value` per line.
pub(crate) fn command_of(argv: &str, env: &str) -> Result<AgentCommand, String> {
    let argv: Vec<String> = argv
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    if argv.is_empty() {
        return Err("Name the program that starts the agent.".into());
    }
    let mut variables = BTreeMap::new();
    for line in env.lines().map(str::trim).filter(|line| !line.is_empty()) {
        match line.split_once('=') {
            Some((name, value)) if !name.trim().is_empty() => {
                variables.insert(name.trim().to_owned(), value.to_owned());
            }
            _ => return Err(format!("`{line}` is not NAME=value.")),
        }
    }
    Ok(AgentCommand {
        argv,
        env: variables,
    })
}

fn env_text(env: &BTreeMap<String, String>) -> String {
    env.iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The form that sets up the agent.
#[component]
pub(crate) fn Setup(current: Option<AssistantSettings>) -> Element {
    let assistant = use_context::<Assistant>();
    let current = current.unwrap_or_default();
    let mut argv = use_signal(|| current.argv.join("\n"));
    let mut env = use_signal(|| env_text(&current.env));
    let mut author = use_signal(|| current.author.clone());
    let mut role = use_signal(|| current.role.clone());
    let mut chosen = use_signal(String::new);
    let mut outcome = use_signal(|| None::<String>);
    let registry = use_resource(fetch_registry);
    let agents = match &*registry.read() {
        Some(Ok(agents)) => agents.clone(),
        _ => Vec::new(),
    };
    let choices: Vec<Choice> = agents
        .iter()
        .map(|agent| {
            Choice::plain(agent.id.clone())
                .label(format!("{} {}", agent.name, agent.version))
                .note(agent.command.argv[0].clone())
        })
        .collect();
    let described = agents
        .iter()
        .find(|agent| agent.id == chosen())
        .map(|agent| agent.description.clone());
    let command = command_of(&argv(), &env());
    let problem = command.as_ref().err().cloned().or_else(|| {
        (author().trim().is_empty() || role().trim().is_empty())
            .then(|| "Name the author and the role the agent writes as.".to_owned())
    });
    // An empty form is not yet a mistake; it only cannot be saved.
    let shown_problem = problem.clone().filter(|_| !argv().trim().is_empty());
    let save = move |_| {
        let Ok(command) = command_of(&argv(), &env()) else {
            return;
        };
        let settings = AssistantSettings {
            argv: command.argv,
            env: command.env,
            author: author().trim().to_owned(),
            role: role().trim().to_owned(),
        };
        let result = Settings::locate()
            .ok_or_else(|| "no configuration directory: set HONE_CONFIG_DIR".to_owned())
            .and_then(|file| file.set_assistant(Some(settings)));
        match result {
            Ok(()) => assistant.saved(),
            Err(message) => outcome.set(Some(message)),
        }
    };
    rsx! {
        div { class: "flex flex-col gap-4",
            p { class: "m-0 text-ink-muted",
                "Choose the agent the assistant talks to over the Agent Client Protocol. It reaches hone only through hone mcp, as its own author. Its provider, model, and login stay with the agent."
            }
            Field { label: "ACP Registry",
                match &*registry.read() {
                    None => rsx! { p { class: "m-0 text-[13px] text-ink-muted", "Reading the registry …" } },
                    Some(Err(message)) => rsx! {
                        p { class: "m-0 text-[13px] text-warn", "{message}. Enter a custom command below." }
                    },
                    Some(Ok(_)) => rsx! {
                        Combobox {
                            value: chosen(),
                            choices,
                            placeholder: "Search agents started by npx or uvx",
                            label: "Agent of the ACP Registry",
                            onselect: move |id: String| {
                                if let Some(agent) = agents.iter().find(|agent| agent.id == id) {
                                    argv.set(agent.command.argv.join("\n"));
                                    env.set(env_text(&agent.command.env));
                                }
                                chosen.set(id);
                            },
                        }
                    },
                }
            }
            if let Some(description) = described {
                p { class: "m-0 text-[13px] text-ink-muted", "{description}" }
            }
            Field { label: "Command, one argument per line",
                Textarea { class: "font-mono text-[13px]", rows: 4, value: argv(), placeholder: "gemini\n--acp", oninput: move |value| argv.set(value) }
            }
            Field { label: "Environment, one NAME=value per line",
                Textarea { class: "font-mono text-[13px]", rows: 2, value: env(), oninput: move |value| env.set(value) }
            }
            div { class: "grid grid-cols-2 gap-3",
                Field { label: "Writes as",
                    Input { class: "font-mono", value: author(), oninput: move |value| author.set(value) }
                }
                Field { label: "Role",
                    Input { class: "font-mono", value: role(), oninput: move |value| role.set(value) }
                }
            }
            p { class: "m-0 text-[13px] text-ink-muted",
                "A role that the project's actors do not grant leaves the agent reading."
            }
            if let Some(problem) = shown_problem {
                p { class: "m-0 text-[13px] text-bad", "✕ {problem}" }
            }
            div { class: "flex items-center gap-2",
                Button { disabled: problem.is_some(), onclick: save, "Save" }
                if assistant.configured() {
                    Button { variant: ButtonVariant::Ghost, onclick: move |_| assistant.close_setup(), "Cancel" }
                }
            }
            if let Some(message) = outcome() {
                ProblemAlert { title: "The agent was not saved", message }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn registry_agents_start_through_npx_or_uvx() {
        let registry = json!({
            "version": "1.0.0",
            "agents": [
                {
                    "id": "gemini", "name": "Gemini CLI", "version": "0.71.1", "description": "Google's agent",
                    "distribution": { "npx": { "package": "@google/gemini-cli@0.71.1", "args": ["--acp"] } }
                },
                {
                    "id": "fast-agent", "name": "fast-agent", "version": "0.10.66", "description": "",
                    "distribution": { "uvx": { "package": "fast-agent-acp==0.10.66", "args": ["-x"], "env": { "FAST_AGENT_MODEL": "codexplan" } } }
                },
                {
                    "id": "claude-acp", "name": "Claude Agent", "version": "0.110.0", "description": "",
                    "distribution": {
                        "binary": { "linux-x86_64": { "archive": "https://example.org/a.tar.gz", "cmd": "./claude" } },
                        "npx": { "package": "@agentclientprotocol/claude-agent-acp@0.110.0" }
                    }
                },
                {
                    "id": "goose", "name": "Goose", "version": "1.0.0", "description": "",
                    "distribution": { "binary": { "linux-x86_64": { "archive": "https://example.org/g.tar.gz", "cmd": "./goose" } } }
                }
            ]
        });
        let npx = if cfg!(windows) { "npx.cmd" } else { "npx" };
        assert_eq!(
            registry_agents(&registry),
            [
                RegistryAgent {
                    id: "claude-acp".into(),
                    name: "Claude Agent".into(),
                    version: "0.110.0".into(),
                    description: String::new(),
                    command: AgentCommand {
                        argv: vec![
                            npx.into(),
                            "-y".into(),
                            "@agentclientprotocol/claude-agent-acp@0.110.0".into()
                        ],
                        env: BTreeMap::new(),
                    },
                },
                RegistryAgent {
                    id: "fast-agent".into(),
                    name: "fast-agent".into(),
                    version: "0.10.66".into(),
                    description: String::new(),
                    command: AgentCommand {
                        argv: vec!["uvx".into(), "fast-agent-acp==0.10.66".into(), "-x".into()],
                        env: BTreeMap::from([("FAST_AGENT_MODEL".into(), "codexplan".into())]),
                    },
                },
                RegistryAgent {
                    id: "gemini".into(),
                    name: "Gemini CLI".into(),
                    version: "0.71.1".into(),
                    description: "Google's agent".into(),
                    command: AgentCommand {
                        argv: vec![
                            npx.into(),
                            "-y".into(),
                            "@google/gemini-cli@0.71.1".into(),
                            "--acp".into()
                        ],
                        env: BTreeMap::new(),
                    },
                },
            ]
        );
    }

    #[test]
    fn the_form_text_becomes_a_command() {
        assert_eq!(
            command_of(" gemini \n\n--acp\n", "\nA=1\nB = x=y\n"),
            Ok(AgentCommand {
                argv: vec!["gemini".into(), "--acp".into()],
                env: BTreeMap::from([("A".into(), "1".into()), ("B".into(), " x=y".into())]),
            })
        );
        assert_eq!(
            command_of("\n", ""),
            Err("Name the program that starts the agent.".into())
        );
        assert_eq!(
            command_of("gemini", "NOVALUE"),
            Err("`NOVALUE` is not NAME=value.".into())
        );
        assert_eq!(
            command_of("gemini", "=x"),
            Err("`=x` is not NAME=value.".into())
        );
    }
}
