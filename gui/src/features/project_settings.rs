//! The Settings screen: what the GUI keeps for this project in this checkout, apart from the
//! tracker (the IDE and the packages directory).

use std::path::PathBuf;

use dioxus::prelude::*;

use super::common::{Field, PageHeader, ProblemAlert, SectionLabel, WarningNote};
use super::{Project, Session};
use crate::ide::Ide;
use crate::settings::{ProjectSettings, Settings};
use crate::ui::{Button, Input, Textarea};

const PRESETS: [(&str, &str); 5] = [
    ("none", "None"),
    ("vscode", "VS Code"),
    ("zed", "Zed"),
    ("jetbrains", "JetBrains"),
    ("custom", "Custom"),
];

/// The IDE the form fields describe, `None` for "None".
fn ide_of(preset: &str, launcher: &str, argv: &str) -> Option<Ide> {
    match preset {
        "vscode" => Some(Ide::Vscode),
        "zed" => Some(Ide::Zed),
        "jetbrains" => Some(Ide::Jetbrains {
            launcher: launcher.trim().to_owned(),
        }),
        "custom" => Some(Ide::Custom {
            argv: argv
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect(),
        }),
        _ => None,
    }
}

/// The Settings screen.
#[component]
pub(crate) fn ProjectSettingsScreen() -> Element {
    let project = use_context::<Project>();
    let settings = project.settings.read().clone();
    rsx! {
        PageHeader { context: "this project in this checkout", title: "Settings" }
        match settings {
            None => rsx! {},
            Some(Err(message)) => rsx! { ProblemAlert { title: "The settings cannot be read", message } },
            Some(Ok(current)) => rsx! { SettingsForm { current } },
        }
    }
}

#[component]
fn SettingsForm(current: ProjectSettings) -> Element {
    let session = use_context::<Session>();
    let mut project = use_context::<Project>();
    let (preset, launcher, argv) = match &current.ide {
        None => ("none", "idea".to_owned(), String::new()),
        Some(Ide::Vscode) => ("vscode", "idea".to_owned(), String::new()),
        Some(Ide::Zed) => ("zed", "idea".to_owned(), String::new()),
        Some(Ide::Jetbrains { launcher }) => ("jetbrains", launcher.clone(), String::new()),
        Some(Ide::Custom { argv }) => ("custom", "idea".to_owned(), argv.join("\n")),
    };
    let mut preset = use_signal(|| preset);
    let mut launcher = use_signal(|| launcher);
    let mut argv = use_signal(|| argv);
    let mut packages = use_signal(|| {
        current
            .packages
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default()
    });
    let mut outcome = use_signal(|| None::<Result<(), String>>);
    let ide = ide_of(preset(), &launcher(), &argv());
    let problem = ide.as_ref().and_then(Ide::problem);
    let packages_missing = {
        let text = packages();
        let text = text.trim();
        !text.is_empty() && !PathBuf::from(text).is_dir()
    };
    let save = move |_| {
        let Some(id) = project.project_id() else {
            outcome.set(Some(Err(
                "the project cannot be read, so the settings have no key".into(),
            )));
            return;
        };
        let ide = ide_of(preset(), &launcher(), &argv());
        if let Some(problem) = ide.as_ref().and_then(Ide::problem) {
            outcome.set(Some(Err(problem)));
            return;
        }
        let packages =
            Some(PathBuf::from(packages().trim())).filter(|path| !path.as_os_str().is_empty());
        let root = (session.hone)().root().to_path_buf();
        let result = Settings::locate()
            .ok_or_else(|| "no configuration directory: set HONE_CONFIG_DIR".to_owned())
            .and_then(|settings| {
                settings.update(&id, &root, |kept| {
                    kept.ide = ide;
                    kept.packages = packages;
                })
            });
        if result.is_ok() {
            project.settings_revision += 1;
        }
        outcome.set(Some(result));
    };
    rsx! {
        section { class: "flex flex-col gap-4 max-w-[760px]",
            SectionLabel { label: "IDE", aside: "Open in IDE, and Ctrl+click on an anchor" }
            div { class: "flex gap-0 border border-line w-fit", role: "radiogroup", "aria-label": "IDE",
                for (value , label) in PRESETS {
                    button {
                        role: "radio",
                        "aria-checked": if preset() == value { "true" } else { "false" },
                        class: if preset() == value { "h-11 px-5 bg-ink text-bg font-semibold" } else { "h-11 px-5 hover:bg-surface-2" },
                        onclick: move |_| preset.set(value),
                        "{label}"
                    }
                }
            }
            if preset() == "jetbrains" {
                Field { label: "Launcher (idea, rustrover, pycharm, phpstorm, webstorm, …)",
                    Input { class: "font-mono w-60", value: launcher(), invalid: problem.is_some(), oninput: move |value| launcher.set(value) }
                }
            }
            if preset() == "custom" {
                Field { label: "Program and arguments, one per line; {{file}} and {{line}} are replaced",
                    Textarea { class: "font-mono", rows: 4, value: argv(), placeholder: "nvim\n+{{line}}\n{{file}}", oninput: move |value| argv.set(value) }
                }
            }
            if let Some(ide) = &ide {
                p { class: "m-0 font-mono text-[12.5px] text-ink-muted break-all",
                    "runs: {ide.template().join(\" \")}"
                }
                p { class: "m-0 text-[13px] text-ink-muted",
                    "The program runs without a shell, each line one argument, so a file name cannot become a command. On Windows, name the program as it is on disk, for example code.cmd."
                }
            }
            if let Some(problem) = problem.clone() {
                p { class: "m-0 text-[13px] text-bad", "✕ {problem}" }
            }
        }
        section { class: "flex flex-col gap-4 max-w-[760px]",
            SectionLabel { label: "Packages directory", aside: "passed as --packages" }
            Field { label: "Where hone resolve finds the packages that extends names; empty for none",
                Input { class: "font-mono", value: packages(), placeholder: "/home/alice/hone-packages", oninput: move |value| packages.set(value) }
            }
            p { class: "m-0 text-[13px] text-ink-muted",
                "Used by hone resolve, hone ticket start, and hone ticket switch-config."
            }
            if packages_missing {
                WarningNote { text: "This directory does not exist on this device." }
            }
        }
        div { class: "flex flex-wrap items-center gap-3",
            Button { disabled: problem.is_some(), onclick: save, "Save settings" }
            if let Some(settings) = Settings::locate() {
                span { class: "font-mono text-[12.5px] text-ink-muted break-all", "{settings.path().display()}" }
            }
        }
        match outcome() {
            Some(Ok(())) => rsx! { p { class: "m-0 text-[13px] text-ok", role: "status", "■ Saved." } },
            Some(Err(message)) => rsx! { ProblemAlert { title: "The settings were not saved", message } },
            None => rsx! {},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_form_fields_become_a_setting() {
        assert_eq!(ide_of("none", "idea", ""), None);
        assert_eq!(ide_of("zed", "idea", ""), Some(Ide::Zed));
        assert_eq!(
            ide_of("jetbrains", " rustrover ", ""),
            Some(Ide::Jetbrains {
                launcher: "rustrover".into()
            })
        );
        assert_eq!(
            ide_of("custom", "", " nvim \n\n+{line}\n{file}\n"),
            Some(Ide::Custom {
                argv: vec!["nvim".into(), "+{line}".into(), "{file}".into()]
            })
        );
    }
}
