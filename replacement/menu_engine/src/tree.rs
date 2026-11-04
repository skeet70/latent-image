//! The declarative menu tree generated from the decoded image-4 menu tables by
//! `//replacement/modal_runtime:menu-engine-tree`. Stock record addresses are
//! in `block::RecordTable`, not here.

use super::frame::{word, MenuClass, NodeId, NodeType, RowToken, RowsPerPage};

/// Slots of the registry `0x0081babc`.
pub const REGISTRY_SLOTS: usize = 0x86;

/// A position in `Tree::nodes`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NodeRef(u16);

impl NodeRef {
    pub const fn new(index: u16) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A registry slot number; stage events name nodes by it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NodeIndex(u8);

impl NodeIndex {
    pub const fn parse(raw: u32) -> Option<Self> {
        if (raw as usize) < REGISTRY_SLOTS { Some(Self(raw as u8)) } else { None }
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    pub const fn raw(self) -> u32 {
        self.0 as u32
    }
}

/// Node word `+0x10`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wrap {
    Wraps,
    Stops,
}

/// Node word `+0x14`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Availability {
    Queried,
    Fixed,
}

/// Node word `+0x18`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Remember {
    Off,
    On,
}

/// Halfword `+0x28`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NodeWord10(u16);

impl NodeWord10 {
    pub const fn new(raw: u16) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

word!(
    /// A label string id; ids from `0x83c` are not in the English table.
    StringId
);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Row {
    pub token: RowToken,
    pub child: Option<NodeRef>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Label {
    pub preview_icon: StringId,
    pub list_icon: StringId,
    pub help: StringId,
    pub name: StringId,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Node {
    pub id: NodeId,
    pub node_type: NodeType,
    pub class: MenuClass,
    /// Never zero in a generated tree.
    pub rows_per_page: RowsPerPage,
    pub wrap: Wrap,
    pub availability: Availability,
    pub remember: Remember,
    pub parent: Option<NodeRef>,
    pub word10: NodeWord10,
    /// `None` for a null values pointer.
    pub rows: Option<&'static [Row]>,
}

/// SHA-256 of the generated tree text without its identity line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TreeIdentity(pub [u8; 32]);

#[derive(Clone, Copy, Debug)]
pub struct Tree {
    pub nodes: &'static [Node],
    pub registry: [Option<NodeRef>; REGISTRY_SLOTS],
    pub identity: TreeIdentity,
}

impl Tree {
    pub fn node(&self, node: NodeRef) -> Option<&Node> {
        self.nodes.get(node.index())
    }

    pub fn registered(&self, index: NodeIndex) -> Option<NodeRef> {
        self.registry.get(index.index()).copied().flatten()
    }
}
