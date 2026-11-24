//! The context a message carries: IDs and paths, never content, because the agent reads the
//! content itself through `hone mcp`.

/// One element of the context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chip {
    pub(crate) kind: ChipKind,
    pub(crate) value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChipKind {
    Ticket,
    Step,
    /// An artifact, a module among them.
    Artifact,
    Thread,
    Run,
    /// A source file, `path` or `path#symbol`.
    Code,
}

const KINDS: [(ChipKind, &str); 6] = [
    (ChipKind::Ticket, "ticket"),
    (ChipKind::Step, "step"),
    (ChipKind::Artifact, "artifact"),
    (ChipKind::Thread, "thread"),
    (ChipKind::Run, "run"),
    (ChipKind::Code, "code"),
];

impl Chip {
    pub(crate) fn new(kind: ChipKind, value: impl Into<String>) -> Self {
        Self {
            kind,
            value: value.into(),
        }
    }

    /// The chip as the message names it, for example `step interfaces`.
    pub(crate) fn text(&self) -> String {
        let name = KINDS
            .iter()
            .find(|(kind, _)| *kind == self.kind)
            .map_or("", |(_, name)| name);
        format!("{name} {}", self.value)
    }

    /// The chip that [`Chip::text`] gave `text`.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let (name, value) = text.split_once(' ')?;
        let kind = KINDS.iter().find(|(_, known)| *known == name)?.0;
        Some(Self::new(kind, value))
    }

    /// The chip's label: an ID speaks for itself, a step, thread, run, or file is named.
    pub(crate) fn label(&self) -> String {
        match self.kind {
            ChipKind::Ticket | ChipKind::Artifact => self.value.clone(),
            _ => self.text(),
        }
    }
}

/// The text sent to the agent: the message, then the context as a list.
pub(crate) fn prompt(message: &str, chips: &[Chip]) -> String {
    let message = message.trim();
    if chips.is_empty() {
        return message.to_owned();
    }
    let context: Vec<String> = chips
        .iter()
        .map(|chip| format!("- {}", chip.text()))
        .collect();
    format!(
        "{message}\n\nContext, to read through the hone MCP server:\n{}",
        context.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_lists_its_context_by_id_and_path() {
        let chips = [
            Chip::new(ChipKind::Ticket, "tk-9f2x4a8m"),
            Chip::new(ChipKind::Step, "interfaces"),
            Chip::new(ChipKind::Code, "src/fee.rs#FeeCalculator::quote"),
        ];
        assert_eq!(
            prompt("  Why is interfaces blocked?\n", &chips),
            "Why is interfaces blocked?\n\nContext, to read through the hone MCP server:\n\
             - ticket tk-9f2x4a8m\n- step interfaces\n- code src/fee.rs#FeeCalculator::quote"
        );
        assert_eq!(prompt("Hello", &[]), "Hello");
        assert_eq!(
            chips.iter().map(Chip::label).collect::<Vec<_>>(),
            [
                "tk-9f2x4a8m",
                "step interfaces",
                "code src/fee.rs#FeeCalculator::quote"
            ]
        );
    }

    #[test]
    fn a_chip_reads_back_from_its_text() {
        for (kind, _) in KINDS {
            let chip = Chip::new(kind, "x y");
            assert_eq!(Chip::parse(&chip.text()), Some(chip));
        }
        assert_eq!(Chip::parse("module mod-9c3d6h4v"), None);
        assert_eq!(Chip::parse("ticket"), None);
    }
}
