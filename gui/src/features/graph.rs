//! The link graph: artifacts and ticket records as nodes, their stored ID links as edges.

use std::collections::{BTreeMap, BTreeSet};

use dioxus::prelude::*;

use super::common::{Empty, FailureAlert, Loading, PageHeader};
use super::{Project, Session};
use crate::hone::CatalogEntry;
use crate::ui::{Choice, Combobox};

const NODE_WIDTH: f64 = 190.0;
const NODE_HEIGHT: f64 = 46.0;
const COLUMN_GAP: f64 = 110.0;
const ROW_GAP: f64 = 18.0;
const MARGIN: f64 = 32.0;
// Room on the left for the bends of links within a column.
const LEFT: f64 = 104.0;
const HEADER: f64 = 28.0;

/// The link types an edge can have: the ID links of concept Section 5 and `ticket`.
pub(crate) const LINK_TYPES: [&str; 6] = [
    "supersedes",
    "affects",
    "verifies",
    "depends-on",
    "relates",
    "ticket",
];

/// A node placed in the graph.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Node {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) title: Option<String>,
    pub(crate) superseded: bool,
    pub(crate) x: f64,
    pub(crate) y: f64,
}

/// A stored link from `from` to `to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Edge {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) kind: String,
}

/// What the graph shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Filter {
    pub(crate) hidden_kinds: BTreeSet<String>,
    pub(crate) hidden_links: BTreeSet<String>,
    /// Only this ticket, its artifacts, and their direct neighbours.
    pub(crate) ticket: Option<String>,
}

/// The placed nodes, the edges between them, the column titles with their x, and the size.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Layout {
    pub(crate) nodes: Vec<Node>,
    pub(crate) edges: Vec<Edge>,
    pub(crate) columns: Vec<(String, f64)>,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

/// Every edge the catalog stores: the ID links of an artifact, and an artifact's ticket.
pub(crate) fn edges_of(entries: &[CatalogEntry]) -> Vec<Edge> {
    let mut edges = Vec::new();
    for entry in entries {
        let Some(from) = entry.id.clone() else {
            continue;
        };
        for (kind, ids) in &entry.links {
            for to in ids {
                edges.push(Edge {
                    from: from.clone(),
                    to: to.clone(),
                    kind: kind.clone(),
                });
            }
        }
        if entry.kind != "ticket"
            && let Some(ticket) = &entry.ticket
        {
            edges.push(Edge {
                from: from.clone(),
                to: ticket.clone(),
                kind: "ticket".into(),
            });
        }
    }
    edges
}

/// Places the entries `filter` lets through: one column per kind (ticket records first, then
/// the kinds by name), and within a column an order that follows the rows of each node's
/// neighbours, so that edges cross less.
pub(crate) fn layout(entries: &[CatalogEntry], filter: &Filter) -> Layout {
    let edges: Vec<Edge> = edges_of(entries)
        .into_iter()
        .filter(|edge| !filter.hidden_links.contains(&edge.kind))
        .collect();
    let mut shown: Vec<&CatalogEntry> = entries
        .iter()
        .filter(|entry| entry.id.is_some() && !filter.hidden_kinds.contains(&entry.kind))
        .collect();
    if let Some(ticket) = &filter.ticket {
        let core: BTreeSet<&str> = shown
            .iter()
            .filter(|entry| {
                entry.id.as_deref() == Some(ticket.as_str())
                    || entry.ticket.as_deref() == Some(ticket.as_str())
            })
            .filter_map(|entry| entry.id.as_deref())
            .collect();
        let near: BTreeSet<String> = edges
            .iter()
            .filter(|edge| core.contains(edge.from.as_str()) || core.contains(edge.to.as_str()))
            .flat_map(|edge| [edge.from.clone(), edge.to.clone()])
            .chain(core.iter().map(|id| (*id).to_owned()))
            .collect();
        shown.retain(|entry| entry.id.as_ref().is_some_and(|id| near.contains(id)));
    }
    let ids: BTreeSet<String> = shown.iter().filter_map(|entry| entry.id.clone()).collect();
    let edges: Vec<Edge> = edges
        .into_iter()
        .filter(|edge| ids.contains(&edge.from) && ids.contains(&edge.to))
        .collect();

    let kind_of: BTreeMap<&str, &str> = shown
        .iter()
        .filter_map(|entry| Some((entry.id.as_deref()?, entry.kind.as_str())))
        .collect();
    // A kind that stores more links than it receives stands further left, so arrows point right.
    let balance = |kind: &str| -> i64 {
        edges
            .iter()
            .map(|edge| {
                i64::from(kind_of.get(edge.to.as_str()) == Some(&kind))
                    - i64::from(kind_of.get(edge.from.as_str()) == Some(&kind))
            })
            .sum()
    };
    let mut kinds: Vec<String> = shown.iter().map(|entry| entry.kind.clone()).collect();
    kinds.sort();
    kinds.dedup();
    kinds.sort_by_key(|kind| (kind != "ticket", balance(kind)));
    let mut columns: Vec<Vec<String>> = kinds
        .iter()
        .map(|kind| {
            let mut column: Vec<String> = shown
                .iter()
                .filter(|entry| &entry.kind == kind)
                .filter_map(|entry| entry.id.clone())
                .collect();
            column.sort();
            column
        })
        .collect();
    // Two sweeps of the barycentre heuristic: each node takes the mean row of its neighbours in
    // the columns already placed.
    for _ in 0..2 {
        for index in 0..columns.len() {
            let rows: BTreeMap<String, f64> = columns
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != index)
                .flat_map(|(_, column)| {
                    column
                        .iter()
                        .enumerate()
                        .map(|(row, id)| (id.clone(), row as f64))
                })
                .collect();
            let weight = |id: &String| {
                let neighbours: Vec<f64> = edges
                    .iter()
                    .filter_map(|edge| {
                        if &edge.from == id {
                            rows.get(&edge.to).copied()
                        } else if &edge.to == id {
                            rows.get(&edge.from).copied()
                        } else {
                            None
                        }
                    })
                    .collect();
                if neighbours.is_empty() {
                    f64::MAX
                } else {
                    neighbours.iter().sum::<f64>() / neighbours.len() as f64
                }
            };
            let mut keyed: Vec<(f64, String)> = columns[index]
                .iter()
                .map(|id| (weight(id), id.clone()))
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
            columns[index] = keyed.into_iter().map(|(_, id)| id).collect();
        }
    }
    let by_id: BTreeMap<&str, &CatalogEntry> = shown
        .iter()
        .filter_map(|entry| Some((entry.id.as_deref()?, *entry)))
        .collect();
    let mut nodes = Vec::new();
    let mut column_titles = Vec::new();
    let mut tallest = 0usize;
    for (index, column) in columns.iter().enumerate() {
        let x = LEFT + index as f64 * (NODE_WIDTH + COLUMN_GAP);
        column_titles.push((kinds[index].clone(), x));
        tallest = tallest.max(column.len());
        for (row, id) in column.iter().enumerate() {
            let entry = by_id[id.as_str()];
            nodes.push(Node {
                id: id.clone(),
                kind: entry.kind.clone(),
                title: entry.title.clone(),
                superseded: entry.effective.as_deref() == Some("superseded"),
                x,
                y: MARGIN + HEADER + row as f64 * (NODE_HEIGHT + ROW_GAP),
            });
        }
    }
    let width =
        LEFT + MARGIN + columns.len().max(1) as f64 * (NODE_WIDTH + COLUMN_GAP) - COLUMN_GAP;
    let height = MARGIN * 2.0 + HEADER + tallest.max(1) as f64 * (NODE_HEIGHT + ROW_GAP);
    Layout {
        nodes,
        edges,
        columns: column_titles,
        width,
        height,
    }
}

/// The SVG path of an edge from `from` to `to` and the point for its label. Edges leave a node
/// on the side that faces the target; within one column they bend out to the right.
fn route(from: &Node, to: &Node) -> (String, f64, f64) {
    let (y1, y2) = (from.y + NODE_HEIGHT / 2.0, to.y + NODE_HEIGHT / 2.0);
    if to.x - from.x > NODE_WIDTH + COLUMN_GAP + 1.0 {
        // Across a column, the edge runs through the gap between two rows of that column, so it
        // never passes behind a node and looks like that node's link.
        let pitch = NODE_HEIGHT + ROW_GAP;
        let first_gap = MARGIN + HEADER - ROW_GAP / 2.0;
        let lane = first_gap + (((y1 + y2) / 2.0 - first_gap) / pitch).round().max(0.0) * pitch;
        let (x1, x2) = (from.x + NODE_WIDTH, to.x);
        let (enter, leave) = (x1 + COLUMN_GAP - 16.0, x2 - COLUMN_GAP + 16.0);
        let bend = (enter - x1) / 2.0;
        let path = format!(
            "M{x1:.1},{y1:.1} C{:.1},{y1:.1} {:.1},{lane:.1} {enter:.1},{lane:.1} L{leave:.1},{lane:.1} C{:.1},{lane:.1} {:.1},{y2:.1} {x2:.1},{y2:.1}",
            x1 + bend,
            enter - bend,
            leave + bend,
            x2 - bend,
        );
        return (path, (enter + leave) / 2.0, lane);
    }
    let points = if to.x > from.x {
        let (x1, x2) = (from.x + NODE_WIDTH, to.x);
        let bend = (x2 - x1) / 2.0;
        [(x1, y1), (x1 + bend, y1), (x2 - bend, y2), (x2, y2)]
    } else if to.x < from.x {
        let (x1, x2) = (from.x, to.x + NODE_WIDTH);
        let bend = (x1 - x2) / 2.0;
        [(x1, y1), (x1 - bend, y1), (x2 + bend, y2), (x2, y2)]
    } else {
        let x = from.x;
        let bend = (36.0 + (y2 - y1).abs() / 5.0).min(LEFT - 12.0);
        [(x, y1), (x - bend, y1), (x - bend, y2), (x, y2)]
    };
    let [p0, p1, p2, p3] = points;
    let path = format!(
        "M{:.1},{:.1} C{:.1},{:.1} {:.1},{:.1} {:.1},{:.1}",
        p0.0, p0.1, p1.0, p1.1, p2.0, p2.1, p3.0, p3.1
    );
    let label_x = (p0.0 + 3.0 * p1.0 + 3.0 * p2.0 + p3.0) / 8.0;
    let label_y = (p0.1 + 3.0 * p1.1 + 3.0 * p2.1 + p3.1) / 8.0;
    (path, label_x, label_y)
}

fn shorten(text: &str, length: usize) -> String {
    if text.chars().count() <= length {
        text.to_owned()
    } else {
        format!("{}…", text.chars().take(length - 1).collect::<String>())
    }
}

/// The link graph of the checked directory, filterable by kind, link type, and ticket.
#[component]
pub(crate) fn LinkGraph() -> Element {
    let mut session = use_context::<Session>();
    let project = use_context::<Project>();
    // Ticket records and their links fill the graph with one line per artifact; they start
    // hidden, and the ticket filter shows one ticket's artifacts.
    let mut filter = use_signal(|| Filter {
        hidden_kinds: BTreeSet::from(["ticket".to_owned()]),
        hidden_links: BTreeSet::from(["ticket".to_owned()]),
        ticket: None,
    });
    let mut hovered = use_signal(|| None::<String>);
    let focus = (session.graph_focus)();
    let catalog = project.catalog.read().clone();
    let Some(catalog) = catalog else {
        return rsx! { PageHeader { title: "Link graph" } Loading {} };
    };
    let catalog = match catalog {
        Ok(catalog) => catalog,
        Err(failure) => {
            return rsx! { PageHeader { title: "Link graph" } FailureAlert { failure } };
        }
    };
    let placed = layout(&catalog.artifacts, &filter());
    let mut kinds: Vec<String> = catalog
        .artifacts
        .iter()
        .map(|entry| entry.kind.clone())
        .collect();
    kinds.sort_by_key(|kind| (kind != "ticket", kind.clone()));
    kinds.dedup();
    let ticket_choices: Vec<Choice> = std::iter::once(Choice::plain("").label("every ticket"))
        .chain(project.ticket_choices())
        .collect();
    let active = hovered().or(focus.clone());
    let few_edges = placed.edges.len() <= 12;
    let nodes: BTreeMap<String, Node> = placed
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node.clone()))
        .collect();
    let neighbours: BTreeSet<String> = active
        .iter()
        .flat_map(|id| {
            placed
                .edges
                .iter()
                .filter(move |edge| &edge.from == id || &edge.to == id)
                .flat_map(|edge| [edge.from.clone(), edge.to.clone()])
        })
        .collect();
    rsx! {
        PageHeader {
            context: format!("{} nodes · {} links · only forward links are stored, the arrows point from the artifact that stores them", placed.nodes.len(), placed.edges.len()),
            title: "Link graph",
        }
        div { class: "flex flex-wrap items-end gap-x-8 gap-y-4",
            fieldset { class: "m-0 p-0 border-0 flex flex-col gap-1",
                legend { class: "text-[13px] text-ink-muted pb-1", "Kinds" }
                div { class: "flex flex-wrap gap-x-4 gap-y-1",
                    for kind in kinds {
                        FilterBox {
                            label: kind.clone(),
                            checked: !filter.read().hidden_kinds.contains(&kind),
                            ontoggle: move |on: bool| filter.with_mut(|filter| {
                                if on { filter.hidden_kinds.remove(&kind); } else { filter.hidden_kinds.insert(kind.clone()); }
                            }),
                        }
                    }
                }
            }
            fieldset { class: "m-0 p-0 border-0 flex flex-col gap-1",
                legend { class: "text-[13px] text-ink-muted pb-1", "Links" }
                div { class: "flex flex-wrap gap-x-4 gap-y-1",
                    for link in LINK_TYPES {
                        FilterBox {
                            label: link.to_owned(),
                            checked: !filter.read().hidden_links.contains(link),
                            ontoggle: move |on: bool| filter.with_mut(|filter| {
                                if on { filter.hidden_links.remove(link); } else { filter.hidden_links.insert(link.to_owned()); }
                            }),
                        }
                    }
                }
            }
            div { class: "flex flex-col gap-1 w-80",
                span { class: "text-[13px] text-ink-muted", "Ticket" }
                Combobox {
                    value: filter.read().ticket.clone().unwrap_or_default(),
                    choices: ticket_choices,
                    placeholder: "every ticket",
                    label: "Ticket",
                    onselect: move |ticket: String| filter.with_mut(|filter| filter.ticket = Some(ticket).filter(|ticket| !ticket.is_empty())),
                }
            }
        }
        if placed.nodes.is_empty() {
            Empty { text: "Nothing to show. Artifacts appear here once their file exists; check the filters." }
        } else {
            div { class: "border border-line bg-surface overflow-auto max-h-[70vh]",
                svg {
                    width: "{placed.width}",
                    height: "{placed.height}",
                    view_box: "0 0 {placed.width} {placed.height}",
                    role: "img",
                    "aria-label": "Link graph",
                    defs {
                        pattern { id: "hone-dots", width: "24", height: "24", "patternUnits": "userSpaceOnUse",
                            circle { cx: "1", cy: "1", r: "1", fill: "var(--hone-line)" }
                        }
                        marker { id: "hone-arrow", view_box: "0 0 10 10", "refX": "10", "refY": "5", "markerWidth": "7", "markerHeight": "7", orient: "auto-start-reverse",
                            path { d: "M0,0 L10,5 L0,10 z", fill: "var(--hone-ink-muted)" }
                        }
                        marker { id: "hone-arrow-active", view_box: "0 0 10 10", "refX": "10", "refY": "5", "markerWidth": "7", "markerHeight": "7", orient: "auto-start-reverse",
                            path { d: "M0,0 L10,5 L0,10 z", fill: "var(--hone-accent)" }
                        }
                    }
                    rect { width: "100%", height: "100%", fill: "url(#hone-dots)" }
                    for (kind , x) in placed.columns.iter() {
                        text { x: "{x}", y: "{MARGIN + 12.0}", fill: "var(--hone-ink-muted)", font_family: "var(--font-mono)", font_size: "11", letter_spacing: "0.08em",
                            {kind.to_uppercase()}
                        }
                    }
                    for edge in placed.edges.iter() {
                        if let (Some(from), Some(to)) = (nodes.get(&edge.from), nodes.get(&edge.to)) {
                            {
                                let (path, label_x, label_y) = route(from, to);
                                let lit = active.as_ref().is_some_and(|id| &edge.from == id || &edge.to == id);
                                let dim = active.is_some() && !lit;
                                rsx! {
                                    g { opacity: if dim { "0.2" } else if lit { "1" } else { "0.6" },
                                        path {
                                            d: "{path}",
                                            fill: "none",
                                            stroke: if lit { "var(--hone-accent)" } else { "var(--hone-ink-muted)" },
                                            stroke_width: "2",
                                            stroke_dasharray: if edge.kind == "ticket" { "4 4" } else { "none" },
                                            marker_end: if lit { "url(#hone-arrow-active)" } else { "url(#hone-arrow)" },
                                        }
                                        if lit || (few_edges && edge.kind != "ticket") {
                                            text {
                                                x: "{label_x}",
                                                y: "{label_y - 4.0}",
                                                text_anchor: "middle",
                                                fill: if lit { "var(--hone-accent)" } else { "var(--hone-ink-muted)" },
                                                font_family: "var(--font-mono)",
                                                font_size: "11",
                                                paint_order: "stroke",
                                                stroke: "var(--hone-surface)",
                                                stroke_width: "4",
                                                "{edge.kind}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    for node in placed.nodes.iter().cloned() {
                        GraphNode {
                            key: "{node.id}",
                            selected: focus.as_deref() == Some(node.id.as_str()),
                            dim: active.is_some() && !neighbours.contains(&node.id) && active.as_deref() != Some(node.id.as_str()),
                            onhover: move |id: Option<String>| hovered.set(id),
                            onopen: move |id: String| {
                                session.graph_focus.set(Some(id.clone()));
                                if id.starts_with("tk-") {
                                    session.open_ticket(id);
                                } else {
                                    session.open_artifact(id);
                                }
                            },
                            node,
                        }
                    }
                }
            }
            p { class: "m-0 text-[13px] text-ink-muted",
                "Hover or focus a node to follow its links and read their types; click or press Enter to open it. Dashed nodes are superseded, dashed lines link an artifact to its ticket."
            }
        }
    }
}

#[component]
fn FilterBox(label: String, checked: bool, ontoggle: EventHandler<bool>) -> Element {
    rsx! {
        label { class: "h-8 inline-flex items-center gap-2 text-[13px] font-mono cursor-pointer",
            input {
                r#type: "checkbox",
                class: "size-4 accent-[var(--hone-accent)]",
                checked,
                onchange: move |event| ontoggle.call(event.checked()),
            }
            "{label}"
        }
    }
}

#[component]
fn GraphNode(
    node: Node,
    selected: bool,
    dim: bool,
    onhover: EventHandler<Option<String>>,
    onopen: EventHandler<String>,
) -> Element {
    let (fill, text, stroke, stroke_width) = if selected {
        (
            "var(--hone-ink)",
            "var(--hone-bg)",
            "var(--hone-accent)",
            "2",
        )
    } else {
        ("var(--hone-bg)", "var(--hone-ink)", "var(--hone-line)", "1")
    };
    let enter = node.id.clone();
    let open = node.id.clone();
    let key_open = node.id.clone();
    let title = node
        .title
        .as_deref()
        .map(|title| shorten(title, 25))
        .unwrap_or_else(|| node.kind.clone());
    rsx! {
        g {
            class: "cursor-pointer outline-none",
            tabindex: "0",
            role: "link",
            "aria-label": "Open {node.id}",
            opacity: if dim { "0.35" } else { "1" },
            onmouseenter: move |_| onhover.call(Some(enter.clone())),
            onmouseleave: move |_| onhover.call(None),
            onfocus: {
                let id = node.id.clone();
                move |_| onhover.call(Some(id.clone()))
            },
            onblur: move |_| onhover.call(None),
            onclick: move |_| onopen.call(open.clone()),
            onkeydown: move |event| {
                if event.key() == Key::Enter {
                    onopen.call(key_open.clone());
                }
            },
            title { "{node.id}" }
            rect {
                x: "{node.x}",
                y: "{node.y}",
                width: "{NODE_WIDTH}",
                height: "{NODE_HEIGHT}",
                fill,
                stroke,
                stroke_width,
                stroke_dasharray: if node.superseded { "5 4" } else { "none" },
            }
            text { x: "{node.x + 12.0}", y: "{node.y + 19.0}", fill: text, font_family: "var(--font-mono)", font_size: "12.5",
                text_decoration: if node.superseded { "line-through" } else { "none" },
                "{node.id}"
            }
            text { x: "{node.x + 12.0}", y: "{node.y + 36.0}", fill: text, opacity: "0.7", font_size: "12", "{title}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        id: &str,
        kind: &str,
        ticket: Option<&str>,
        links: &[(&str, &[&str])],
    ) -> CatalogEntry {
        CatalogEntry {
            id: Some(id.into()),
            kind: kind.into(),
            path: String::new(),
            ticket: ticket.map(str::to_owned),
            title: None,
            status: None,
            effective: None,
            profile: None,
            external: Vec::new(),
            links: links
                .iter()
                .map(|(kind, ids)| {
                    (
                        (*kind).to_owned(),
                        ids.iter().map(|id| (*id).to_owned()).collect(),
                    )
                })
                .collect(),
        }
    }

    fn catalog() -> Vec<CatalogEntry> {
        vec![
            entry(
                "dec-a",
                "decision",
                Some("tk-1"),
                &[("affects", &["mod-b"])],
            ),
            entry(
                "dec-b",
                "decision",
                Some("tk-2"),
                &[("supersedes", &["dec-a"])],
            ),
            entry("mod-a", "module", None, &[]),
            entry("mod-b", "module", None, &[]),
            entry("tk-1", "ticket", None, &[]),
            entry("tk-2", "ticket", None, &[]),
        ]
    }

    fn rows(layout: &Layout, kind: &str) -> Vec<String> {
        layout
            .nodes
            .iter()
            .filter(|node| node.kind == kind)
            .map(|node| node.id.clone())
            .collect()
    }

    #[test]
    fn kinds_that_store_more_links_than_they_receive_stand_left() {
        let mut entries = catalog();
        // Modules now store three links to decisions and receive one.
        entries[2].links = BTreeMap::from([("relates".to_owned(), vec!["dec-a".to_owned()])]);
        entries[3].links = BTreeMap::from([(
            "relates".to_owned(),
            vec!["dec-a".to_owned(), "dec-b".to_owned()],
        )]);
        let placed = layout(&entries, &Filter::default());
        let columns: Vec<&str> = placed
            .columns
            .iter()
            .map(|(kind, _)| kind.as_str())
            .collect();
        assert_eq!(columns, ["ticket", "module", "decision"]);
    }

    #[test]
    fn tickets_come_first_and_kinds_follow_by_name() {
        let placed = layout(&catalog(), &Filter::default());
        let columns: Vec<&str> = placed
            .columns
            .iter()
            .map(|(kind, _)| kind.as_str())
            .collect();
        assert_eq!(columns, ["ticket", "decision", "module"]);
        assert_eq!(placed.edges.len(), 4);
    }

    #[test]
    fn neighbours_pull_a_node_up_in_its_column() {
        let placed = layout(&catalog(), &Filter::default());
        // mod-b is linked from dec-a, mod-a from nothing: mod-b goes first.
        assert_eq!(rows(&placed, "module"), ["mod-b", "mod-a"]);
    }

    #[test]
    fn a_ticket_filter_keeps_the_ticket_its_artifacts_and_their_neighbours() {
        let filter = Filter {
            ticket: Some("tk-1".into()),
            ..Filter::default()
        };
        let placed = layout(&catalog(), &filter);
        let mut ids: Vec<&str> = placed.nodes.iter().map(|node| node.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, ["dec-a", "dec-b", "mod-b", "tk-1"]);
    }

    #[test]
    fn hidden_kinds_and_links_drop_their_nodes_and_edges() {
        let filter = Filter {
            hidden_kinds: BTreeSet::from(["module".to_owned()]),
            hidden_links: BTreeSet::from(["ticket".to_owned()]),
            ticket: None,
        };
        let placed = layout(&catalog(), &filter);
        assert!(placed.nodes.iter().all(|node| node.kind != "module"));
        assert_eq!(
            placed.edges,
            [Edge {
                from: "dec-b".into(),
                to: "dec-a".into(),
                kind: "supersedes".into()
            }]
        );
    }

    #[test]
    fn long_titles_are_shortened() {
        assert_eq!(shorten("Payments run asynchronously", 12), "Payments ru…");
        assert_eq!(shorten("Short", 12), "Short");
    }
}
