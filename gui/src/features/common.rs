use dioxus::prelude::*;

use crate::hone::Failure;
use crate::ui::{Alert, AlertDescription, AlertTitle, Skeleton};

/// The head of a screen: a context line, the title, and the actions on the right.
#[component]
pub(crate) fn PageHeader(
    #[props(into, optional)] context: Option<String>,
    #[props(into)] title: String,
    #[props(optional)] actions: Option<Element>,
) -> Element {
    rsx! {
        header { class: "flex flex-wrap items-end justify-between gap-6",
            div { class: "flex flex-col gap-2 min-w-0",
                if let Some(context) = context {
                    span { class: "font-mono text-[13px] text-ink-muted break-all", "{context}" }
                }
                h1 { class: "m-0 text-[32px] leading-[37px] font-bold tracking-[-0.02em] break-words",
                    "{title}"
                }
            }
            if let Some(actions) = actions {
                div { class: "flex flex-wrap gap-2", {actions} }
            }
        }
    }
}

/// An uppercase section label with optional text on the right.
#[component]
pub(crate) fn SectionLabel(
    #[props(into)] label: String,
    #[props(optional)] aside: Option<String>,
) -> Element {
    rsx! {
        div { class: "flex items-baseline justify-between gap-4 pb-3",
            h2 { class: "m-0 font-mono text-[11px] leading-4 font-medium tracking-[0.08em] uppercase",
                "{label}"
            }
            if let Some(aside) = aside {
                span { class: "text-[13px] text-ink-muted", "{aside}" }
            }
        }
    }
}

/// The meaning a [`StatusMark`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    Done,
    Open,
    Fail,
    Skip,
}

/// A state as a glyph and a word; colour only reinforces it.
#[component]
pub(crate) fn StatusMark(mark: Mark, #[props(into)] word: String) -> Element {
    let (glyph, colour) = match mark {
        Mark::Done => ("■", "text-ok"),
        Mark::Open => ("□", "text-ink"),
        Mark::Fail => ("✕", "text-bad"),
        Mark::Skip => ("⁄", "text-ink-muted line-through"),
    };
    rsx! {
        span { class: "font-mono text-[12.5px] leading-[18px] inline-flex gap-1 items-baseline {colour}",
            span { "aria-hidden": "true", "{glyph}" }
            "{word}"
        }
    }
}

/// Something that did not work: a title and the reason, framed in `bad`, never in the accent.
#[component]
pub(crate) fn ProblemAlert(
    #[props(into)] title: String,
    #[props(into)] message: String,
) -> Element {
    rsx! {
        Alert { class: "border-2 border-bad",
            AlertTitle { class: "text-bad",
                span { "aria-hidden": "true", "✕ " }
                "{title}"
            }
            AlertDescription { class: "font-mono text-[13px] break-words", "{message}" }
        }
    }
}

/// The title of a failed call: a refusal of the request, or a command that could not run.
fn failure_title(status: &str) -> String {
    match status {
        "invalid" | "refused" => format!("hone refused it ({status})"),
        status => format!("hone could not run ({status})"),
    }
}

/// A failed call of the command line: what did not run and why.
#[component]
pub(crate) fn FailureAlert(failure: Failure) -> Element {
    rsx! {
        ProblemAlert {
            title: failure_title(&failure.status),
            message: failure.message,
        }
    }
}

/// Rows of full height while data loads.
#[component]
pub(crate) fn Loading() -> Element {
    rsx! {
        div { class: "flex flex-col gap-2",
            Skeleton { class: "h-11 w-full" }
            Skeleton { class: "h-11 w-full" }
            Skeleton { class: "h-11 w-full" }
        }
    }
}

/// A sentence saying what will appear here and what makes it appear.
#[component]
pub(crate) fn Empty(#[props(into)] text: String) -> Element {
    rsx! {
        p { class: "text-ink-muted m-0", "{text}" }
    }
}

/// A tag: a short fact in a hairline box. `inverted` marks the chosen value, `muted` a detail,
/// `bad` a fact that needs attention.
#[component]
pub(crate) fn Tag(
    #[props(optional)] inverted: bool,
    #[props(optional)] muted: bool,
    #[props(optional)] bad: bool,
    children: Element,
) -> Element {
    let class = if inverted {
        "bg-ink text-bg border-ink"
    } else if bad {
        "border-bad text-bad"
    } else if muted {
        "border-line text-ink-muted"
    } else {
        "border-line"
    };
    rsx! {
        span { class: "h-7 px-2 inline-flex items-center border font-mono text-[12.5px] {class}", {children} }
    }
}

/// Opens an `http` or `https` address in the system browser. Other addresses are ignored, so a
/// value from a tracker or a ticket record cannot start a program.
pub(crate) fn open_in_browser(url: &str) {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return;
    }
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(windows)]
    let mut command = {
        let mut command = std::process::Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    #[cfg(not(any(target_os = "macos", windows)))]
    let mut command = std::process::Command::new("xdg-open");
    let _ = command
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// A labelled field.
#[component]
pub(crate) fn Field(#[props(into)] label: String, children: Element) -> Element {
    rsx! {
        label { class: "flex flex-col gap-1 min-w-0 text-[13px] text-ink-muted",
            "{label}"
            {children}
        }
    }
}

/// The UTC time an event was written, `YYYY-MM-DD HH:MM`, from its ID: the ID of an event is
/// `ev-` plus a UUIDv7, whose first 48 bits are milliseconds since the Unix epoch.
pub(crate) fn event_time(id: &str) -> Option<String> {
    let hex: String = id
        .strip_prefix("ev-")?
        .chars()
        .filter(|c| *c != '-')
        .take(12)
        .collect();
    if hex.len() < 12 || id.chars().nth(17) != Some('7') {
        return None;
    }
    let seconds = i64::try_from(u64::from_str_radix(&hex, 16).ok()? / 1000).ok()?;
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Howard Hinnant's days_from_civil, inverted.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    Some(format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60
    ))
}

#[cfg(test)]
mod tests {
    use super::event_time;

    #[test]
    fn the_time_of_an_event_comes_from_its_uuid_v7() {
        // date -u -d @1763581440 +'%F %R' → 2025-11-19 19:44; 1763581440000 = 0x019a9da53000
        assert_eq!(
            event_time("ev-019a9da5-3000-7000-8000-000000000000").as_deref(),
            Some("2025-11-19 19:44")
        );
        // date -u -d @951782400 → 2000-02-29 00:00; 951782400000 = 0x00dd9aa6e000
        assert_eq!(
            event_time("ev-00dd9aa6-e000-7000-8000-000000000000").as_deref(),
            Some("2000-02-29 00:00")
        );
        assert_eq!(event_time("ev-0193a8f0-9b2c-4e4d-a1f3-000000000001"), None);
        assert_eq!(event_time("th-019a9db0-3700-7000-8000-000000000000"), None);
    }
}
