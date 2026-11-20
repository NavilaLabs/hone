use dioxus::prelude::*;

use super::common::{Empty, Field, PageHeader, ProblemAlert, SectionLabel, open_in_browser};
use super::{Project, Session};
use crate::hone::CatalogEntry;
use crate::settings::Settings;
use crate::tracker::{Issue, TokenSource, Tracker, suggest_github};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Input, Skeleton, Table, TableBody, TableCell, TableHead,
    TableHeader, TableRow, TableWrapper,
};

/// The hone tickets whose record names `issue` of `tracker` under `external`.
fn linked_tickets(tracker: &Tracker, records: &[CatalogEntry], issue: &str) -> Vec<String> {
    records
        .iter()
        .filter(|record| {
            record
                .external
                .iter()
                .any(|external| tracker.names(&external.system, &external.key, issue))
        })
        .filter_map(|record| record.id.clone())
        .collect()
}

/// Where the project's tickets live outside hone, and a search over them.
#[component]
pub(crate) fn TrackerScreen() -> Element {
    let project = use_context::<Project>();
    let tracker = project
        .settings
        .read()
        .clone()
        .map(|settings| settings.map(|settings| settings.tracker));
    rsx! {
        PageHeader {
            context: project.project_id().map(|id| format!("{id} · stored on this device, never in the project")).unwrap_or_default(),
            title: "Tracker",
        }
        match tracker {
            None => rsx! { Skeleton { class: "h-24 w-full" } },
            Some(Err(message)) => rsx! {
                ProblemAlert { title: "The tracker setting cannot be read", message }
            },
            Some(Ok(None)) => rsx! { TrackerForm { current: None } },
            Some(Ok(Some(tracker))) => rsx! {
                TrackerBrowser { key: "{tracker.describe()}", tracker: tracker.clone() }
                details { class: "border border-line",
                    summary { class: "px-5 h-11 flex items-center cursor-pointer font-mono text-[11px] tracking-[0.08em] uppercase", "Setting and token · {tracker.describe()}" }
                    div { class: "p-5 border-t border-line flex flex-col gap-6",
                        TrackerForm { current: Some(tracker.clone()) }
                        TokenPanel { tracker }
                    }
                }
            },
        }
    }
}

/// The text fields of the tracker form, filled from a setting.
#[derive(Default)]
struct FormFields {
    owner: String,
    repo: String,
    base_url: String,
    project_key: String,
    email: String,
    token_env: String,
}

impl FormFields {
    fn of(tracker: Option<&Tracker>) -> Self {
        match tracker {
            None => Self::default(),
            Some(Tracker::Github {
                owner,
                repo,
                token_env,
            }) => Self {
                owner: owner.clone(),
                repo: repo.clone(),
                token_env: token_env.clone().unwrap_or_default(),
                ..Self::default()
            },
            Some(Tracker::Jira {
                base_url,
                project_key,
                email,
                token_env,
            }) => Self {
                base_url: base_url.clone(),
                project_key: project_key.clone(),
                email: email.clone().unwrap_or_default(),
                token_env: token_env.clone().unwrap_or_default(),
                ..Self::default()
            },
        }
    }
}

#[component]
fn TrackerForm(current: Option<Tracker>) -> Element {
    let session = use_context::<Session>();
    let mut project = use_context::<Project>();
    let mut kind = use_signal(|| match &current {
        Some(Tracker::Jira { .. }) => "jira",
        _ => "github",
    });
    let initial = FormFields::of(current.as_ref());
    let mut owner = use_signal(|| initial.owner.clone());
    let mut repo = use_signal(|| initial.repo.clone());
    let mut base_url = use_signal(|| initial.base_url.clone());
    let mut project_key = use_signal(|| initial.project_key.clone());
    let mut email = use_signal(|| initial.email.clone());
    let mut token_env = use_signal(|| initial.token_env.clone());
    let mut message = use_signal(|| None::<String>);
    let suggestion = use_resource(move || async move {
        let root = (session.hone)().root().to_path_buf();
        suggest_github(&root).await
    });
    let has_current = current.is_some();
    // A new setting starts from the Git remote when it is a GitHub repository.
    use_effect(move || {
        if !has_current
            && let Some(Some((suggested_owner, suggested_repo))) = &*suggestion.read()
            && owner.peek().is_empty()
            && repo.peek().is_empty()
        {
            owner.set(suggested_owner.clone());
            repo.set(suggested_repo.clone());
        }
    });
    let built = move || {
        let optional = |text: String| Some(text.trim().to_owned()).filter(|text| !text.is_empty());
        if kind() == "jira" {
            Tracker::Jira {
                base_url: base_url().trim().trim_end_matches('/').to_owned(),
                project_key: project_key().trim().to_owned(),
                email: optional(email()),
                token_env: optional(token_env()),
            }
        } else {
            Tracker::Github {
                owner: owner().trim().to_owned(),
                repo: repo().trim().to_owned(),
                token_env: optional(token_env()),
            }
        }
    };
    let mut store = move |tracker: Option<Tracker>| {
        let Some(id) = project.project_id() else {
            message.set(Some(
                "the project cannot be read, so the setting has no key".into(),
            ));
            return;
        };
        if let Some(problem) = tracker.as_ref().and_then(Tracker::problem) {
            message.set(Some(problem));
            return;
        }
        let root = (session.hone)().root().to_path_buf();
        let outcome = Settings::locate()
            .ok_or_else(|| "no configuration directory: set HONE_CONFIG_DIR".to_owned())
            .and_then(|settings| settings.update(&id, &root, |kept| kept.tracker = tracker));
        match outcome {
            Ok(()) => {
                message.set(None);
                project.settings_revision += 1;
            }
            Err(error) => message.set(Some(error)),
        }
    };
    let suggested = match &*suggestion.read() {
        Some(Some((owner, repo))) => Some(format!("{owner}/{repo}")),
        _ => None,
    };
    let default_variable = built().token_variable();
    let invalid_field = move |name: &str| message().is_some_and(|message| message.contains(name));
    rsx! {
        section { class: "flex flex-col gap-4 max-w-[760px]",
            SectionLabel { label: if has_current { "Change the tracker" } else { "Where do this project's tickets live?" } }
            if !has_current {
                p { class: "m-0 text-[15px] leading-[25px] text-ink-muted max-w-[640px]",
                    "Set the tracker once per checkout. hone keeps it on this device, keyed by project and root, so nothing lands in the project and another checkout asks again."
                }
                if let Some(settings) = Settings::locate() {
                    p { class: "m-0 font-mono text-[12.5px] text-ink-muted break-all", "{settings.path().display()}" }
                }
            }
            div { class: "flex gap-0 border border-line w-fit", role: "radiogroup", "aria-label": "Tracker",
                for (value , label) in [("github", "GitHub Issues"), ("jira", "Jira")] {
                    button {
                        role: "radio",
                        "aria-checked": if kind() == value { "true" } else { "false" },
                        class: if kind() == value { "h-11 px-5 bg-ink text-bg font-semibold" } else { "h-11 px-5 hover:bg-surface-2" },
                        onclick: move |_| kind.set(value),
                        "{label}"
                    }
                }
            }
            if kind() == "github" {
                div { class: "flex flex-wrap items-end gap-3",
                    Field { label: "Owner",
                        Input { class: "font-mono w-48", value: owner(), placeholder: "acme", invalid: invalid_field("owner"), oninput: move |value| owner.set(value) }
                    }
                    span { class: "h-9 flex items-center font-mono text-ink-muted", "/" }
                    Field { label: "Repository",
                        Input { class: "font-mono w-56", value: repo(), placeholder: "payments", invalid: invalid_field("repository"), oninput: move |value| repo.set(value) }
                    }
                }
                p { class: "m-0 text-[13px] text-ink-muted",
                    match &suggested {
                        Some(suggested) => rsx! { "The Git remote origin points to {suggested}. " },
                        None => rsx! { "The Git remote origin is no GitHub repository, so enter it by hand. " },
                    }
                    "Issues are read through api.github.com; public repositories work without a token."
                }
            } else {
                div { class: "flex flex-wrap items-end gap-3",
                    Field { label: "Base URL",
                        Input { class: "font-mono w-80", value: base_url(), placeholder: "https://acme.atlassian.net", invalid: invalid_field("base URL"), oninput: move |value| base_url.set(value) }
                    }
                    Field { label: "Project key",
                        Input { class: "font-mono w-28", value: project_key(), placeholder: "PAY", invalid: invalid_field("project key"), oninput: move |value| project_key.set(value) }
                    }
                    Field { label: "Account email (Jira Cloud only)",
                        Input { class: "font-mono w-64", value: email(), placeholder: "alice@acme.example", oninput: move |value| email.set(value) }
                    }
                }
                p { class: "m-0 text-[13px] text-ink-muted",
                    "With an account email, the token is a Jira Cloud API token. Without one, it is a personal access token of Jira Server or Data Center."
                }
            }
            Field { label: "Token variable (optional)",
                Input { class: "font-mono w-64", value: token_env(), placeholder: default_variable, oninput: move |value| token_env.set(value) }
            }
            div { class: "flex flex-wrap gap-2",
                Button { onclick: move |_| store(Some(built())), if has_current { "Save" } else { "Save and browse" } }
                if has_current {
                    Button { variant: ButtonVariant::Outline, onclick: move |_| store(None), "Remove the tracker" }
                }
            }
            if let Some(message) = message() {
                p { class: "m-0 text-[13px] text-bad", role: "alert", "✕ {message}" }
            }
        }
    }
}

#[component]
fn TokenPanel(tracker: Tracker) -> Element {
    let mut token = use_signal(String::new);
    let mut message = use_signal(|| None::<Result<String, String>>);
    let mut revision = use_signal(|| 0u64);
    let source_tracker = tracker.clone();
    let source = use_memo(move || {
        let _ = revision();
        source_tracker.token_source()
    });
    let variable = tracker.token_variable();
    let store_tracker = tracker.clone();
    let remove_tracker = tracker.clone();
    rsx! {
        section { class: "flex flex-col gap-3 max-w-[760px]",
            SectionLabel { label: "Token" }
            p { class: "m-0 text-[13px]",
                match source() {
                    TokenSource::Environment(variable) => rsx! { "■ from the environment variable " span { class: "font-mono", "{variable}" } },
                    TokenSource::Keyring => rsx! { "■ stored in the OS keyring (" span { class: "font-mono", "{variable}" } " is not set)" },
                    TokenSource::None => rsx! { "□ none: set " span { class: "font-mono", "{variable}" } " or store one in the OS keyring" },
                }
            }
            div { class: "flex flex-wrap items-end gap-2",
                label { class: "flex flex-col gap-1 text-[13px] text-ink-muted",
                    "Token for the OS keyring"
                    input {
                        r#type: "password",
                        class: "h-9 w-80 border border-line bg-transparent px-3 font-mono text-[13px] text-ink dark:bg-input/30",
                        autocomplete: "off",
                        value: token(),
                        oninput: move |event| token.set(event.value()),
                    }
                }
                Button {
                    variant: ButtonVariant::Outline,
                    disabled: token().trim().is_empty(),
                    onclick: move |_| {
                        let outcome = store_tracker.store_token(token().trim());
                        token.set(String::new());
                        message.set(Some(outcome.map(|()| "stored in the OS keyring".to_owned())));
                        revision += 1;
                    },
                    "Store"
                }
                Button {
                    variant: ButtonVariant::Ghost,
                    onclick: move |_| {
                        message.set(Some(remove_tracker.remove_token().map(|()| "removed from the OS keyring".to_owned())));
                        revision += 1;
                    },
                    "Remove stored token"
                }
            }
            match message() {
                Some(Ok(text)) => rsx! { p { class: "m-0 text-[13px] text-ok", "■ {text}" } },
                Some(Err(text)) => rsx! { p { class: "m-0 text-[13px] text-bad", role: "alert", "✕ {text}" } },
                None => rsx! {},
            }
            p { class: "m-0 text-[13px] text-ink-muted", "A token is never written to a file. The variable wins over the keyring." }
        }
    }
}

#[component]
fn TrackerBrowser(tracker: Tracker) -> Element {
    let project = use_context::<Project>();
    let mut query = use_signal(String::new);
    let mut submitted = use_signal(String::new);
    let searched = tracker.clone();
    let results = use_resource(move || {
        let tracker = searched.clone();
        async move { tracker.search(&submitted()).await }
    });
    let records: Vec<CatalogEntry> = match &*project.catalog.read() {
        Some(Ok(catalog)) => catalog.tickets().cloned().collect(),
        _ => Vec::new(),
    };
    rsx! {
        section { class: "flex flex-col gap-4",
            form {
                class: "flex flex-wrap items-end gap-2",
                onsubmit: move |event| {
                    event.prevent_default();
                    submitted.set(query());
                },
                Field { label: "Search {tracker.describe()}",
                    Input {
                        class: "w-[420px]",
                        value: query(),
                        placeholder: match tracker { Tracker::Github { .. } => "words, or #12", Tracker::Jira { .. } => "words, or PAY-123" },
                        oninput: move |value| query.set(value),
                    }
                }
                Button { onclick: move |_| submitted.set(query()), "Search" }
                if !submitted().is_empty() {
                    Button { variant: ButtonVariant::Ghost, onclick: move |_| { query.set(String::new()); submitted.set(String::new()); }, "Recent open" }
                }
            }
            match &*results.read() {
                None => rsx! { Skeleton { class: "h-11 w-full" } Skeleton { class: "h-11 w-full" } },
                Some(Err(message)) => rsx! {
                    ProblemAlert { title: format!("{} could not be searched", tracker.describe()), message: message.clone() }
                },
                Some(Ok(issues)) if issues.is_empty() => rsx! {
                    Empty { text: if submitted().is_empty() { "No open ticket in the tracker." } else { "No ticket matches. Try other words or the ticket's key." } }
                },
                Some(Ok(issues)) => rsx! {
                    SectionLabel {
                        label: if submitted().is_empty() { "Recent open tickets" } else { "Matches" },
                        aside: format!("{} shown · linked ones open in hone", issues.len()),
                    }
                    TableWrapper {
                        Table {
                            TableHeader { class: "bg-surface",
                                TableRow {
                                    TableHead { "Key" }
                                    TableHead { "Title" }
                                    TableHead { "State" }
                                    TableHead { "In hone" }
                                }
                            }
                            TableBody {
                                for issue in issues.clone() {
                                    IssueRow {
                                        key: "{issue.key}",
                                        linked: linked_tickets(&tracker, &records, &issue.key),
                                        tracker: tracker.clone(),
                                        issue,
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}

#[component]
fn IssueRow(tracker: Tracker, issue: Issue, linked: Vec<String>) -> Element {
    let mut session = use_context::<Session>();
    let mut copied = use_signal(|| false);
    let url = issue.url.clone();
    let entry = tracker.external_entry(&issue);
    rsx! {
        TableRow {
            TableCell {
                button {
                    class: "font-mono underline whitespace-nowrap",
                    title: "Open in the browser",
                    onclick: move |_| open_in_browser(&url),
                    match tracker { Tracker::Github { .. } => rsx! { "#{issue.key} ↗" }, Tracker::Jira { .. } => rsx! { "{issue.key} ↗" } }
                }
            }
            TableCell { "{issue.title}" }
            TableCell { class: "font-mono text-[12.5px] text-ink-muted", "{issue.state}" }
            TableCell {
                if linked.is_empty() {
                    div { class: "flex items-center gap-2 whitespace-nowrap",
                        span { class: "text-[13px] text-ink-muted", "not in hone" }
                        Button {
                            variant: ButtonVariant::Outline,
                            size: ButtonSize::Sm,
                            title: "Start a hone ticket that names this tracker ticket",
                            onclick: {
                                let seed = super::new_ticket::Seed::from_issue(&tracker, &issue);
                                move |_| session.new_ticket.set(Some(seed.clone()))
                            },
                            "Start ticket"
                        }
                        Button {
                            variant: ButtonVariant::Outline,
                            size: ButtonSize::Sm,
                            title: "Copy the entry for external in a ticket record",
                            onclick: move |_| {
                                let script = format!(
                                    "navigator.clipboard.writeText({})",
                                    serde_json::to_string(&entry).unwrap_or_default()
                                );
                                document::eval(&script);
                                copied.set(true);
                            },
                            if copied() { "Copied" } else { "Copy external entry" }
                        }
                    }
                } else {
                    div { class: "flex flex-wrap gap-2",
                        for ticket in linked {
                            Button {
                                variant: ButtonVariant::Outline,
                                size: ButtonSize::Sm,
                                onclick: {
                                    let ticket = ticket.clone();
                                    move |_| session.open_ticket(ticket.clone())
                                },
                                "Open {ticket}"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hone::ExternalTicket;

    #[test]
    fn a_record_links_the_issues_it_names() {
        let tracker = Tracker::Github {
            owner: "acme".into(),
            repo: "payments".into(),
            token_env: None,
        };
        let record = |id: &str, system: &str, key: &str| CatalogEntry {
            id: Some(id.into()),
            kind: "ticket".into(),
            path: String::new(),
            ticket: None,
            title: None,
            status: None,
            effective: None,
            profile: None,
            external: vec![ExternalTicket {
                system: system.into(),
                key: key.into(),
                url: None,
            }],
            links: std::collections::BTreeMap::new(),
        };
        let records = [
            record("tk-a", "github", "12"),
            record("tk-b", "jira", "12"),
            record("tk-c", "github", "#12"),
        ];
        assert_eq!(linked_tickets(&tracker, &records, "12"), ["tk-a", "tk-c"]);
        assert!(linked_tickets(&tracker, &records, "13").is_empty());
    }
}
