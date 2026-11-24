//! The conversation of one ticket as the panel shows it, built from what the agent sends.

use crate::acp::{Permission, PermissionOption, ToolCall, Update};

/// One element of the conversation, in the order it arrived.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Entry {
    /// The person's message with the context it carried.
    User {
        text: String,
        chips: Vec<String>,
    },
    /// The agent's answer, Markdown.
    Agent(String),
    Thought(String),
    Tool(ToolCall),
    Permission {
        key: u64,
        tool: ToolCall,
        options: Vec<PermissionOption>,
        /// The chosen option's name, or `None` while open.
        answer: Option<Answer>,
    },
    /// A line of the GUI, not of the agent.
    Note(String),
}

/// How a permission request was answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Answer {
    Chosen(String),
    Cancelled,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Chat {
    pub(crate) entries: Vec<Entry>,
}

impl Chat {
    pub(crate) fn apply(&mut self, update: Update) {
        match update {
            Update::UserText(text) => match self.entries.last_mut() {
                Some(Entry::User { text: last, chips }) if chips.is_empty() => last.push_str(&text),
                _ => self.entries.push(Entry::User {
                    text,
                    chips: Vec::new(),
                }),
            },
            Update::AgentText(text) => match self.entries.last_mut() {
                Some(Entry::Agent(last)) => last.push_str(&text),
                _ => self.entries.push(Entry::Agent(text)),
            },
            Update::Thought(text) => match self.entries.last_mut() {
                Some(Entry::Thought(last)) => last.push_str(&text),
                _ => self.entries.push(Entry::Thought(text)),
            },
            Update::ToolCall(call) => match self.tool(&call.id) {
                Some(known) => *known = call,
                None => self.entries.push(Entry::Tool(call)),
            },
            Update::ToolCallUpdate(change) => match self.tool(&change.id) {
                Some(known) => merge(known, change),
                None => self.entries.push(Entry::Tool(change)),
            },
            Update::Other(_) => {}
        }
    }

    pub(crate) fn ask(&mut self, permission: Permission) {
        self.entries.push(Entry::Permission {
            key: permission.key,
            tool: permission.tool,
            options: permission.options,
            answer: None,
        });
    }

    /// Marks the permission request `key` as answered with the option `option`, or as
    /// cancelled with `None`.
    pub(crate) fn answered(&mut self, key: u64, option: Option<&str>) {
        for entry in &mut self.entries {
            if let Entry::Permission {
                key: own,
                options,
                answer,
                ..
            } = entry
                && *own == key
                && answer.is_none()
            {
                *answer = Some(
                    option
                        .and_then(|id| options.iter().find(|choice| choice.id == id))
                        .map_or(Answer::Cancelled, |choice| {
                            Answer::Chosen(choice.name.clone())
                        }),
                );
            }
        }
    }

    /// Marks every open permission request as cancelled.
    pub(crate) fn cancel_open(&mut self) {
        let open: Vec<u64> = self.open_permissions();
        for key in open {
            self.answered(key, None);
        }
    }

    pub(crate) fn open_permissions(&self) -> Vec<u64> {
        self.entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Permission {
                    key, answer: None, ..
                } => Some(*key),
                _ => None,
            })
            .collect()
    }

    fn tool(&mut self, id: &str) -> Option<&mut ToolCall> {
        self.entries.iter_mut().rev().find_map(|entry| match entry {
            Entry::Tool(call) if call.id == id => Some(call),
            _ => None,
        })
    }
}

fn merge(known: &mut ToolCall, change: ToolCall) {
    let ToolCall {
        id: _,
        title,
        kind,
        status,
        input,
        output,
        content,
    } = change;
    known.title = title.or(known.title.take());
    known.kind = kind.or(known.kind.take());
    known.status = status.or(known.status.take());
    known.input = input.or(known.input.take());
    known.output = output.or(known.output.take());
    known.content = content.or(known.content.take());
}

/// A line of a diff: `' '` kept, `'-'` removed, `'+'` added.
pub(crate) type DiffLine<'a> = (char, &'a str);

/// The lines of `new` against `old` (`None` for a new file), with the longest common
/// subsequence kept. Past a size where that costs too much, every old line is removed and every
/// new one added.
pub(crate) fn diff_lines<'a>(old: Option<&'a str>, new: &'a str) -> Vec<DiffLine<'a>> {
    const LIMIT: usize = 4_000_000;
    let old: Vec<&str> = old.map(|old| old.lines().collect()).unwrap_or_default();
    let new: Vec<&str> = new.lines().collect();
    if old.len().saturating_mul(new.len()) > LIMIT {
        return old
            .iter()
            .map(|line| ('-', *line))
            .chain(new.iter().map(|line| ('+', *line)))
            .collect();
    }
    // common[i][j]: the length of the longest common subsequence of old[i..] and new[j..].
    let mut common = vec![vec![0usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            common[i][j] = if old[i] == new[j] {
                common[i + 1][j + 1] + 1
            } else {
                common[i + 1][j].max(common[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut lines = Vec::new();
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            lines.push((' ', old[i]));
            i += 1;
            j += 1;
        } else if i < old.len() && (j == new.len() || common[i + 1][j] >= common[i][j + 1]) {
            lines.push(('-', old[i]));
            i += 1;
        } else {
            lines.push(('+', new[j]));
            j += 1;
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::acp::ToolContent;

    fn option(id: &str, name: &str) -> PermissionOption {
        PermissionOption {
            id: id.into(),
            name: name.into(),
            kind: "allow_once".into(),
        }
    }

    #[test]
    fn chunks_join_until_another_kind_arrives() {
        let mut chat = Chat::default();
        chat.entries.push(Entry::User {
            text: "Why?".into(),
            chips: vec!["tk-9f2x4a8m".into()],
        });
        chat.apply(Update::UserText("replayed ".into()));
        chat.apply(Update::UserText("question".into()));
        chat.apply(Update::Thought("Let me ".into()));
        chat.apply(Update::Thought("look.".into()));
        chat.apply(Update::AgentText("The gate ".into()));
        chat.apply(Update::AgentText("fails.".into()));
        chat.apply(Update::Other("plan".into()));
        chat.apply(Update::AgentText(" Really.".into()));
        assert_eq!(
            chat.entries,
            [
                Entry::User {
                    text: "Why?".into(),
                    chips: vec!["tk-9f2x4a8m".into()]
                },
                Entry::User {
                    text: "replayed question".into(),
                    chips: Vec::new()
                },
                Entry::Thought("Let me look.".into()),
                Entry::Agent("The gate fails. Really.".into()),
            ]
        );
    }

    #[test]
    fn a_tool_call_update_changes_only_the_fields_it_names() {
        let mut chat = Chat::default();
        chat.apply(Update::ToolCall(ToolCall {
            id: "call-1".into(),
            title: Some("thread_reply".into()),
            kind: Some("edit".into()),
            status: Some("pending".into()),
            input: Some(json!({ "body": "ok" })),
            ..ToolCall::default()
        }));
        chat.apply(Update::AgentText("Asking.".into()));
        chat.apply(Update::ToolCallUpdate(ToolCall {
            id: "call-1".into(),
            status: Some("completed".into()),
            content: Some(vec![ToolContent::Text("written".into())]),
            ..ToolCall::default()
        }));
        chat.apply(Update::ToolCallUpdate(ToolCall {
            id: "call-2".into(),
            status: Some("failed".into()),
            ..ToolCall::default()
        }));
        assert_eq!(
            chat.entries,
            [
                Entry::Tool(ToolCall {
                    id: "call-1".into(),
                    title: Some("thread_reply".into()),
                    kind: Some("edit".into()),
                    status: Some("completed".into()),
                    input: Some(json!({ "body": "ok" })),
                    output: None,
                    content: Some(vec![ToolContent::Text("written".into())]),
                }),
                Entry::Agent("Asking.".into()),
                Entry::Tool(ToolCall {
                    id: "call-2".into(),
                    status: Some("failed".into()),
                    ..ToolCall::default()
                }),
            ]
        );
    }

    #[test]
    fn a_permission_keeps_the_name_of_its_answer() {
        let mut chat = Chat::default();
        for key in [1, 2, 3] {
            chat.ask(Permission {
                key,
                session: "s-1".into(),
                tool: ToolCall::default(),
                options: vec![option("allow", "Allow once"), option("reject", "Reject")],
            });
        }
        chat.answered(1, Some("reject"));
        chat.answered(1, Some("allow"));
        chat.answered(2, Some("unknown"));
        assert_eq!(chat.open_permissions(), [3]);
        chat.cancel_open();
        let answers: Vec<Option<Answer>> = chat
            .entries
            .iter()
            .map(|entry| match entry {
                Entry::Permission { answer, .. } => answer.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(
            answers,
            [
                Some(Answer::Chosen("Reject".into())),
                Some(Answer::Cancelled),
                Some(Answer::Cancelled)
            ]
        );
    }

    #[test]
    fn a_diff_keeps_the_common_lines() {
        assert_eq!(
            diff_lines(Some("a\nb\nc\nd\n"), "a\nc\nx\nd\n"),
            [(' ', "a"), ('-', "b"), (' ', "c"), ('+', "x"), (' ', "d")]
        );
        assert_eq!(diff_lines(None, "new\n"), [('+', "new")]);
        assert_eq!(
            diff_lines(Some("status: draft\n"), "status: proposed\n"),
            [('-', "status: draft"), ('+', "status: proposed")]
        );
    }
}
