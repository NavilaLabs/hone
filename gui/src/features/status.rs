//! The status bar: the open ticket's state, the sync, the device, and the theme.

use dioxus::prelude::*;

use super::common::short_hash;
use super::progress::needs_changed_paths;
use super::{Project, Session};

/// The bottom line of the window.
#[component]
pub(crate) fn StatusBar() -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut theme = use_signal(|| "system");
    let ticket = (session.ticket)();
    let summary = ticket.as_deref().and_then(|ticket| project.summary(ticket));
    let config = match &*project.overview.read() {
        Some(Some(Ok(overview))) => Some(overview.config.clone()),
        _ => None,
    };
    // The window's own pure check; a gate that misses only changed paths is not failed.
    let failed = match &*project.check.read() {
        Some(Some(Ok(run))) => Some(
            run.gates
                .iter()
                .filter(|gate| !gate.ok && !needs_changed_paths(gate))
                .count(),
        ),
        _ => None,
    };
    let device = match &*project.log.read() {
        Some(Ok(log)) => log.device.clone(),
        _ => None,
    };
    let sync = (session.last_sync)().map_or_else(
        || "not synced in this window".to_owned(),
        |time| format!("synced {time} UTC"),
    );
    rsx! {
        footer { class: "h-8 px-4 border-t border-line flex items-center gap-6 font-mono text-[12px] text-ink-muted whitespace-nowrap overflow-hidden",
            if let Some(ticket) = ticket {
                span { class: "text-ink", "{ticket}" }
                if let Some(summary) = &summary {
                    if let Some(profile) = &summary.profile {
                        span { "profile {profile}" }
                    }
                }
                if let Some(config) = config {
                    span { "config {short_hash(&config)}" }
                }
                if let Some(summary) = summary {
                    if let Some(current) = &summary.current {
                        {
                            let position = summary.steps.iter().position(|step| &step.step == current).map_or(0, |index| index + 1);
                            rsx! { span { class: "text-signal", "step {position}/{summary.steps.len()} {current}" } }
                        }
                    }
                    if let Some(failed) = failed {
                        if failed == 0 {
                            span { class: "text-ok", "■ pure gates passing" }
                        } else {
                            span { class: "text-bad",
                                {format!("× {failed} gate{} failing", if failed == 1 { "" } else { "s" })}
                            }
                        }
                    }
                    span { "{summary.threads.open} threads · {summary.threads.blocking} blocking" }
                }
            }
            span { class: "ml-auto", "{sync}" }
            if let Some(device) = device {
                span { title: "This device's ID in the project's store", "{device}" }
            }
            button {
                class: "hover:text-ink",
                title: "Switch between system, light, and dark",
                onclick: move |_| {
                    let next = match theme() {
                        "system" => "light",
                        "light" => "dark",
                        _ => "system",
                    };
                    theme.set(next);
                    let script = if next == "system" {
                        "document.documentElement.removeAttribute('data-theme')".to_owned()
                    } else {
                        format!("document.documentElement.setAttribute('data-theme', '{next}')")
                    };
                    document::eval(&script);
                },
                "{theme}"
            }
        }
    }
}
