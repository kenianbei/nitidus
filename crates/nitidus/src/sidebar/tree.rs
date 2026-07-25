//! Pure sidebar tree building: display paths split on `/` into nested
//! rows, per-account sections, synthetic parents for path components
//! without a real folder, and unread aggregation onto collapsed nodes.

use std::collections::{BTreeMap, HashSet};

use nitidus_mail::{AccountId, FolderId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderEntry {
    pub id: FolderId,
    /// Display path (`[Gmail]/Sent Mail`).
    pub path: String,
    pub unread: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountSection {
    pub account: AccountId,
    pub label: String,
    pub entries: Vec<FolderEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowKind {
    AccountHeader,
    Folder(FolderId),
    /// A path component with no folder of its own; selecting it only
    /// toggles collapse.
    Synthetic,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarRow {
    pub account: AccountId,
    pub path: String,
    pub label: String,
    pub kind: RowKind,
    pub depth: u8,
    pub has_children: bool,
    pub is_collapsed: bool,
    pub unread: u32,
}

impl SidebarRow {
    pub fn is_selectable(&self) -> bool {
        !matches!(self.kind, RowKind::AccountHeader)
    }
}

#[derive(Default)]
struct PathNode {
    folder: Option<FolderId>,
    unread: u32,
    children: BTreeMap<String, PathNode>,
}

impl PathNode {
    fn subtree_unread(&self) -> u32 {
        self.unread
            + self
                .children
                .values()
                .map(PathNode::subtree_unread)
                .sum::<u32>()
    }
}

/// Section order follows the input (config order); an account header
/// row leads each section when more than one section exists.
pub fn build_rows(
    sections: &[AccountSection],
    collapsed: &HashSet<(AccountId, String)>,
) -> Vec<SidebarRow> {
    let mut rows = Vec::new();
    for section in sections {
        if sections.len() > 1 {
            rows.push(SidebarRow {
                account: section.account.clone(),
                path: String::new(),
                label: section.label.clone(),
                kind: RowKind::AccountHeader,
                depth: 0,
                has_children: false,
                is_collapsed: false,
                unread: 0,
            });
        }
        let root = build_node_tree(&section.entries);
        push_children(&root, section, collapsed, "", 0, &mut rows);
    }
    rows
}

fn build_node_tree(entries: &[FolderEntry]) -> PathNode {
    let mut root = PathNode::default();
    for entry in entries {
        let mut node = &mut root;
        for component in entry.path.split('/') {
            node = node.children.entry(component.to_owned()).or_default();
        }
        node.folder = Some(entry.id.clone());
        node.unread = entry.unread;
    }
    root
}

fn push_children(
    node: &PathNode,
    section: &AccountSection,
    collapsed: &HashSet<(AccountId, String)>,
    prefix: &str,
    depth: u8,
    rows: &mut Vec<SidebarRow>,
) {
    for (label, child) in ordered_children(node, depth) {
        let path = join_path(prefix, label);
        let has_children = !child.children.is_empty();
        let is_collapsed =
            has_children && collapsed.contains(&(section.account.clone(), path.clone()));
        rows.push(SidebarRow {
            account: section.account.clone(),
            path: path.clone(),
            label: label.clone(),
            kind: match &child.folder {
                Some(id) => RowKind::Folder(id.clone()),
                None => RowKind::Synthetic,
            },
            depth,
            has_children,
            is_collapsed,
            unread: if is_collapsed {
                child.subtree_unread()
            } else {
                child.unread
            },
        });
        if !is_collapsed {
            push_children(child, section, collapsed, &path, depth + 1, rows);
        }
    }
}

/// INBOX leads at the top level; everything else keeps `BTreeMap`'s
/// lexicographic order.
fn ordered_children(node: &PathNode, depth: u8) -> Vec<(&String, &PathNode)> {
    let mut children: Vec<_> = node.children.iter().collect();
    if depth == 0 {
        children.sort_by_key(|(label, _)| (label.as_str() != super::INBOX_NAME, label.as_str()));
    }
    children
}

fn join_path(prefix: &str, label: &str) -> String {
    if prefix.is_empty() {
        label.to_owned()
    } else {
        format!("{prefix}/{label}")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn entry(path: &str, unread: u32) -> FolderEntry {
        FolderEntry {
            id: FolderId::new(format!(".{}", path.replace('/', "."))),
            path: path.to_owned(),
            unread,
        }
    }

    fn section(name: &str, entries: Vec<FolderEntry>) -> AccountSection {
        AccountSection {
            account: AccountId::new(name),
            label: name.to_owned(),
            entries,
        }
    }

    fn labels(rows: &[SidebarRow]) -> Vec<&str> {
        rows.iter().map(|row| row.label.as_str()).collect()
    }

    #[test]
    fn inbox_sorts_first_and_paths_nest() {
        let sections = [section(
            "a",
            vec![
                entry("Work", 2),
                entry("INBOX", 5),
                entry("Archive/2024", 0),
                entry("Archive", 1),
            ],
        )];
        let rows = build_rows(&sections, &HashSet::new());
        assert_eq!(labels(&rows), vec!["INBOX", "Archive", "2024", "Work"]);
        assert_eq!(rows[1].depth, 0);
        assert_eq!(rows[2].depth, 1);
        assert_eq!(rows[2].path, "Archive/2024");
        assert!(rows[1].has_children && !rows[2].has_children);
    }

    #[test]
    fn missing_parents_become_synthetic_rows() {
        let sections = [section(
            "a",
            vec![entry("INBOX", 0), entry("[Gmail]/Sent", 3)],
        )];
        let rows = build_rows(&sections, &HashSet::new());
        assert_eq!(labels(&rows), vec!["INBOX", "[Gmail]", "Sent"]);
        assert_eq!(rows[1].kind, RowKind::Synthetic);
        assert!(
            rows[1].is_selectable(),
            "synthetic parents must be reachable to expand"
        );
        assert!(matches!(rows[2].kind, RowKind::Folder(_)));
    }

    #[test]
    fn collapse_prunes_descendants_and_aggregates_unread() {
        let account = AccountId::new("a");
        let sections = [section(
            "a",
            vec![
                entry("INBOX", 1),
                entry("Archive", 2),
                entry("Archive/Old", 7),
            ],
        )];
        let collapsed = HashSet::from([(account, "Archive".to_owned())]);
        let rows = build_rows(&sections, &collapsed);
        assert_eq!(labels(&rows), vec!["INBOX", "Archive"]);
        assert!(rows[1].is_collapsed);
        assert_eq!(rows[1].unread, 9, "collapsed parent sums its subtree");
    }

    #[test]
    fn multiple_accounts_get_headers_single_account_does_not() {
        let two = [
            section("first", vec![entry("INBOX", 0)]),
            section("second", vec![entry("INBOX", 0)]),
        ];
        let rows = build_rows(&two, &HashSet::new());
        assert_eq!(labels(&rows), vec!["first", "INBOX", "second", "INBOX"]);
        assert_eq!(rows[0].kind, RowKind::AccountHeader);
        assert!(!rows[0].is_selectable());

        let one = [section("only", vec![entry("INBOX", 0)])];
        assert_eq!(labels(&build_rows(&one, &HashSet::new())), vec!["INBOX"]);
    }
}
