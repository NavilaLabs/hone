use dioxus::prelude::*;
use tw_merge::tw_merge;

/// The look of a [`Button`].
#[derive(Default, Clone, PartialEq, Eq)]
pub enum ButtonVariant {
    /// The one primary action of a view.
    #[default]
    Default,
    Outline,
    Ghost,
}

impl ButtonVariant {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "bg-primary text-primary-foreground shadow-xs hover:bg-primary/90",
            Self::Outline => {
                "border bg-background shadow-xs hover:bg-accent hover:text-accent-foreground dark:bg-input/30 dark:border-input dark:hover:bg-input/5"
            }
            Self::Ghost => "hover:bg-accent hover:text-accent-foreground dark:hover:bg-accent/50",
        }
    }
}

/// The height of a [`Button`].
#[derive(Default, Clone, PartialEq, Eq)]
pub enum ButtonSize {
    #[default]
    Default,
    Sm,
}

impl ButtonSize {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "h-9 px-4 py-2 has-[>svg]:px-3",
            Self::Sm => "h-8 rounded-md gap-1.5 px-3 has-[>svg]:px-2.5",
        }
    }
}

/// A button. `onclick` runs unless `disabled` is set.
#[component]
pub fn Button(
    #[props(into, optional)] class: Option<String>,
    #[props(default)] variant: ButtonVariant,
    #[props(default)] size: ButtonSize,
    #[props(optional)] disabled: bool,
    #[props(into, optional)] title: Option<String>,
    #[props(optional)] onclick: Option<EventHandler<MouseEvent>>,
    children: Element,
) -> Element {
    let merged_class = tw_merge!(
        "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-all disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 shrink-0 [&_svg]:shrink-0 outline-none focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive w-fit hover:cursor-pointer active:scale-[0.98] active:opacity-100 touch-manipulation [-webkit-tap-highlight-color:transparent] select-none [-webkit-touch-callout:none]",
        variant.as_str(),
        size.as_str(),
        class.as_deref().unwrap_or("")
    );
    rsx! {
        button {
            class: "{merged_class}",
            r#type: "button",
            disabled,
            title: title.as_deref(),
            onclick: move |event| {
                if let Some(handler) = &onclick {
                    handler.call(event);
                }
            },
            {children}
        }
    }
}
