//! The trees the stats window shows: attributes in groups, requirements, what affects an item.

use std::collections::HashSet;

use crate::icons::Icon;

use super::Target;

/// One line of a tree, with what hangs below it.
#[derive(Debug, Clone, Default)]
pub struct Node {
    /// Unique within its tree; remembers whether the node is open.
    pub key: String,
    pub icon: Option<Icon>,
    pub label: String,
    /// Further columns, for trees that are tables too.
    pub cells: Vec<String>,
    pub children: Vec<Node>,
    /// The item this line stands for, whose own stats its menu opens, labelled with its kind.
    pub link: Option<(String, Target)>,
}

impl Node {
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            ..Self::default()
        }
    }

    pub fn icon(mut self, icon: Option<Icon>) -> Self {
        self.icon = icon;
        self
    }
}

/// Which nodes of a tree are open: all or none by default, flipped one by one.
#[derive(Debug, Clone, Default)]
pub struct Openness {
    pub default_open: bool,
    toggled: HashSet<String>,
}

impl Openness {
    pub fn new(default_open: bool) -> Self {
        Self {
            default_open,
            toggled: HashSet::new(),
        }
    }

    pub fn is_open(&self, key: &str) -> bool {
        self.default_open != self.toggled.contains(key)
    }

    pub fn toggle(&mut self, key: &str) {
        if !self.toggled.remove(key) {
            self.toggled.insert(key.to_owned());
        }
    }

    /// Open or close every node.
    pub fn set_all(&mut self, open: bool) {
        self.default_open = open;
        self.toggled.clear();
    }
}

/// A visible line: the node, how deep it sits, and whether it is open.
pub struct Line<'a> {
    pub node: &'a Node,
    pub depth: usize,
    pub open: bool,
}

/// The lines to draw, top to bottom, skipping what sits under closed nodes.
pub fn lines<'a>(nodes: &'a [Node], openness: &Openness) -> Vec<Line<'a>> {
    fn walk<'a>(nodes: &'a [Node], depth: usize, openness: &Openness, out: &mut Vec<Line<'a>>) {
        for node in nodes {
            let open = !node.children.is_empty() && openness.is_open(&node.key);
            out.push(Line { node, depth, open });
            if open {
                walk(&node.children, depth + 1, openness, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(nodes, 0, openness, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_nodes_hide_their_children() {
        let mut parent = Node::new("p", "parent");
        parent.children.push(Node::new("c", "child"));
        let tree = vec![parent, Node::new("q", "other")];
        let mut openness = Openness::new(false);
        assert_eq!(lines(&tree, &openness).len(), 2);
        openness.toggle("p");
        let shown: Vec<_> = lines(&tree, &openness)
            .iter()
            .map(|l| (l.node.label.as_str(), l.depth))
            .collect();
        assert_eq!(shown, [("parent", 0), ("child", 1), ("other", 0)]);
        openness.set_all(false);
        assert_eq!(lines(&tree, &openness).len(), 2);
    }
}
