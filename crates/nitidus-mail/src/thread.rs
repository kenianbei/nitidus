//! Hand-rolled JWZ threading (references-only, no subject grouping):
//! containers keyed by message-id, reference chains linked cycle-safe,
//! phantom containers pruned. Output rows carry envelope ids, not slice
//! indices, so they stay usable while the underlying store mutates.

use std::collections::HashMap;

use crate::types::{EnvelopeId, EnvelopeSummary};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadRow {
    pub id: EnvelopeId,
    pub parent: Option<EnvelopeId>,
    pub root: EnvelopeId,
    pub depth: u8,
    pub has_children: bool,
}

#[derive(Default)]
struct Container {
    envelope: Option<u32>,
    parent: Option<usize>,
    children: Vec<usize>,
}

struct ResolvedNode {
    envelope: u32,
    children: Vec<ResolvedNode>,
}

/// Threads sort by their newest message, descending; within a thread
/// the walk is depth-first chronological (classic reading order).
pub fn compute_thread_rows(envelopes: &[EnvelopeSummary]) -> Vec<ThreadRow> {
    let (containers, roots) = build_containers(envelopes);
    let mut threads: Vec<ResolvedNode> = roots
        .into_iter()
        .flat_map(|root| resolve_grouped(root, &containers, envelopes))
        .collect();
    sort_nodes(&mut threads, envelopes);
    threads.sort_by_key(|node| std::cmp::Reverse(newest_date(node, envelopes)));
    emit_rows(&threads, envelopes)
}

fn build_containers(envelopes: &[EnvelopeSummary]) -> (Vec<Container>, Vec<usize>) {
    let mut containers: Vec<Container> = Vec::with_capacity(envelopes.len());
    let mut by_message_id: HashMap<&str, usize> = HashMap::new();
    for (index, envelope) in envelopes.iter().enumerate() {
        let node = claim_container(&mut containers, &mut by_message_id, envelope);
        containers[node].envelope = Some(index as u32);
        link_references(&mut containers, &mut by_message_id, envelope, node);
    }
    let roots = (0..containers.len())
        .filter(|&node| containers[node].parent.is_none())
        .collect();
    (containers, roots)
}

/// Duplicate or missing message-ids yield fresh anonymous containers —
/// such messages thread by their own references but cannot be referenced.
fn claim_container<'a>(
    containers: &mut Vec<Container>,
    by_message_id: &mut HashMap<&'a str, usize>,
    envelope: &'a EnvelopeSummary,
) -> usize {
    if envelope.message_id.is_empty() {
        containers.push(Container::default());
        return containers.len() - 1;
    }
    match by_message_id.get(envelope.message_id.as_str()) {
        Some(&existing) if containers[existing].envelope.is_none() => existing,
        Some(_duplicate) => {
            containers.push(Container::default());
            containers.len() - 1
        }
        None => {
            containers.push(Container::default());
            let node = containers.len() - 1;
            by_message_id.insert(&envelope.message_id, node);
            node
        }
    }
}

fn link_references<'a>(
    containers: &mut Vec<Container>,
    by_message_id: &mut HashMap<&'a str, usize>,
    envelope: &'a EnvelopeSummary,
    node: usize,
) {
    let mut previous: Option<usize> = None;
    for reference in &envelope.references {
        let referenced = phantom_for(containers, by_message_id, reference);
        if let Some(parent) = previous
            && containers[referenced].parent.is_none()
            && referenced != parent
            && !is_ancestor(containers, referenced, parent)
        {
            attach(containers, parent, referenced);
        }
        previous = Some(referenced);
    }
    if let Some(parent) = previous
        && containers[node].parent.is_none()
        && parent != node
        && !is_ancestor(containers, node, parent)
    {
        attach(containers, parent, node);
    }
}

fn phantom_for<'a>(
    containers: &mut Vec<Container>,
    by_message_id: &mut HashMap<&'a str, usize>,
    message_id: &'a str,
) -> usize {
    if let Some(&existing) = by_message_id.get(message_id) {
        return existing;
    }
    containers.push(Container::default());
    let node = containers.len() - 1;
    by_message_id.insert(message_id, node);
    node
}

fn attach(containers: &mut [Container], parent: usize, child: usize) {
    containers[child].parent = Some(parent);
    containers[parent].children.push(child);
}

fn is_ancestor(containers: &[Container], candidate: usize, of: usize) -> bool {
    let mut current = Some(of);
    while let Some(node) = current {
        if node == candidate {
            return true;
        }
        current = containers[node].parent;
    }
    false
}

/// Splices empty containers out bottom-up. An empty *root* with several
/// real children keeps the siblings together by promoting the oldest to
/// thread root (mutt's pseudo-root behavior, minus the dummy row).
fn resolve_grouped(
    root: usize,
    containers: &[Container],
    envelopes: &[EnvelopeSummary],
) -> Vec<ResolvedNode> {
    let mut resolved = resolve(root, containers);
    if containers[root].envelope.is_none() && resolved.len() > 1 {
        sort_nodes(&mut resolved, envelopes);
        let mut promoted = resolved.remove(0);
        promoted.children.extend(resolved);
        return vec![promoted];
    }
    resolved
}

fn resolve(node: usize, containers: &[Container]) -> Vec<ResolvedNode> {
    let children: Vec<ResolvedNode> = containers[node]
        .children
        .iter()
        .flat_map(|&child| resolve(child, containers))
        .collect();
    match containers[node].envelope {
        Some(envelope) => vec![ResolvedNode { envelope, children }],
        None => children,
    }
}

fn sort_nodes(nodes: &mut [ResolvedNode], envelopes: &[EnvelopeSummary]) {
    nodes.sort_by_key(|node| {
        let envelope = &envelopes[node.envelope as usize];
        (envelope.date_epoch_secs, envelope.id.clone())
    });
    for node in nodes {
        sort_nodes(&mut node.children, envelopes);
    }
}

fn newest_date(node: &ResolvedNode, envelopes: &[EnvelopeSummary]) -> i64 {
    let own = envelopes[node.envelope as usize].date_epoch_secs;
    node.children
        .iter()
        .map(|child| newest_date(child, envelopes))
        .max()
        .map_or(own, |newest| newest.max(own))
}

fn emit_rows(threads: &[ResolvedNode], envelopes: &[EnvelopeSummary]) -> Vec<ThreadRow> {
    let mut rows = Vec::with_capacity(envelopes.len());
    for thread in threads {
        let root = envelopes[thread.envelope as usize].id.clone();
        emit_subtree(thread, envelopes, None, &root, 0, &mut rows);
    }
    rows
}

fn emit_subtree(
    node: &ResolvedNode,
    envelopes: &[EnvelopeSummary],
    parent: Option<&EnvelopeId>,
    root: &EnvelopeId,
    depth: u8,
    rows: &mut Vec<ThreadRow>,
) {
    let id = envelopes[node.envelope as usize].id.clone();
    rows.push(ThreadRow {
        id: id.clone(),
        parent: parent.cloned(),
        root: root.clone(),
        depth,
        has_children: !node.children.is_empty(),
    });
    for child in &node.children {
        emit_subtree(child, envelopes, Some(&id), root, depth.saturating_add(1), rows);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::types::Flags;

    fn envelope(id: &str, date: i64, message_id: &str, references: &[&str]) -> EnvelopeSummary {
        EnvelopeSummary {
            id: EnvelopeId::new(id),
            subject: format!("subject {id}"),
            from_display: String::new(),
            from_addr: String::new(),
            date_epoch_secs: date,
            flags: Flags::default(),
            message_id: message_id.to_owned(),
            references: references.iter().map(|r| (*r).to_owned()).collect(),
        }
    }

    fn shape(rows: &[ThreadRow]) -> Vec<(&str, u8, &str)> {
        rows.iter()
            .map(|row| (row.id.as_str(), row.depth, row.root.as_str()))
            .collect()
    }

    #[test]
    fn linear_chain_threads_in_order() {
        let envelopes = vec![
            envelope("c", 300, "c@x", &["a@x", "b@x"]),
            envelope("a", 100, "a@x", &[]),
            envelope("b", 200, "b@x", &["a@x"]),
        ];
        let rows = compute_thread_rows(&envelopes);
        assert_eq!(
            shape(&rows),
            vec![("a", 0, "a"), ("b", 1, "a"), ("c", 2, "a")]
        );
        assert_eq!(rows[2].parent.as_ref().unwrap().as_str(), "b");
        assert!(rows[0].has_children);
        assert!(!rows[2].has_children);
    }

    #[test]
    fn branches_walk_chronologically() {
        let envelopes = vec![
            envelope("root", 100, "r@x", &[]),
            envelope("late-reply", 300, "l@x", &["r@x"]),
            envelope("early-reply", 200, "e@x", &["r@x"]),
        ];
        let rows = compute_thread_rows(&envelopes);
        assert_eq!(
            shape(&rows),
            vec![
                ("root", 0, "root"),
                ("early-reply", 1, "root"),
                ("late-reply", 1, "root")
            ]
        );
    }

    #[test]
    fn threads_sort_by_newest_message_descending() {
        let envelopes = vec![
            envelope("old-root", 100, "or@x", &[]),
            envelope("fresh-reply", 400, "fr@x", &["or@x"]),
            envelope("lone", 300, "lo@x", &[]),
        ];
        let rows = compute_thread_rows(&envelopes);
        assert_eq!(
            shape(&rows),
            vec![
                ("old-root", 0, "old-root"),
                ("fresh-reply", 1, "old-root"),
                ("lone", 0, "lone")
            ],
            "thread with the newest reply (400) outranks the lone 300"
        );
    }

    #[test]
    fn missing_parent_groups_siblings_under_oldest() {
        let envelopes = vec![
            envelope("second", 200, "s@x", &["ghost@x"]),
            envelope("first", 100, "f@x", &["ghost@x"]),
        ];
        let rows = compute_thread_rows(&envelopes);
        assert_eq!(shape(&rows), vec![("first", 0, "first"), ("second", 1, "first")]);
    }

    #[test]
    fn reference_cycles_do_not_hang_or_drop_messages() {
        let envelopes = vec![
            envelope("a", 100, "a@x", &["b@x"]),
            envelope("b", 200, "b@x", &["a@x"]),
        ];
        let rows = compute_thread_rows(&envelopes);
        assert_eq!(rows.len(), 2, "{:?}", shape(&rows));
    }

    #[test]
    fn duplicate_and_missing_message_ids_all_surface() {
        let envelopes = vec![
            envelope("dup1", 100, "dup@x", &[]),
            envelope("dup2", 200, "dup@x", &[]),
            envelope("anon1", 300, "", &[]),
            envelope("anon2", 400, "", &[]),
        ];
        let rows = compute_thread_rows(&envelopes);
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|row| row.depth == 0));
    }

    #[test]
    fn self_reference_is_ignored() {
        let envelopes = vec![envelope("selfie", 100, "s@x", &["s@x"])];
        let rows = compute_thread_rows(&envelopes);
        assert_eq!(shape(&rows), vec![("selfie", 0, "selfie")]);
    }
}
