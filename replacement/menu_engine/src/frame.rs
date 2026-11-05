//! The X-M1 1.50 menu-frame classifier `FUN_0061ca2c` and snapshot
//! `FUN_0061cd14` over any source of the frame cells and node words. The guest
//! service in `owned_menu::frame` reads the live cells; the engine reads its
//! in-progress state.

macro_rules! word {
    ($(#[$meta:meta])* $name:ident $(, $visibility:vis)?) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
        pub struct $name($($visibility)? u32);

        impl $name {
            pub const fn new(raw: u32) -> Self {
                Self(raw)
            }

            pub const fn get(self) -> u32 {
                self.0
            }
        }
    };
}

pub(crate) use word;

word!(
    /// Node record word 0.
    NodeId, pub
);
word!(
    /// Node record word 1.
    NodeType
);
word!(
    /// Node record word 2: a menu class or a property number.
    MenuClass
);
word!(
    /// Word 0 of a `{token, child}` row.
    RowToken, pub
);
word!(
    /// A position in a node's rows array.
    RowIndex
);
word!(
    /// A row's position counting only rows that are not hidden.
    VisibleOrdinal
);
word!(
    /// Rows that are not hidden.
    VisibleCount
);
word!(
    /// First visible ordinal of the displayed page.
    ScrollTop
);
word!(
    /// `(rows_per_page + visible_ordinal) / rows_per_page`.
    PageNumber
);
word!(
    /// Node record word 3.
    RowsPerPage
);

impl NodeId {
    pub const ROOT: Self = Self(0);
    pub const DETACHED: Self = Self(0x86);
}

impl NodeType {
    pub const DETACHED: Self = Self(0xc);
}

impl MenuClass {
    pub const MENU_PAGE: Self = Self(5);
}

impl RowToken {
    /// The token word of the `{0x7ffffffe, 0}` pair that ends a rows array.
    pub const TERMINATOR: Self = Self(0x7fff_fffe);
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameDepth {
    Root = 0,
    First = 1,
    Second = 2,
}

impl FrameDepth {
    pub const fn parse(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Root),
            1 => Some(Self::First),
            2 => Some(Self::Second),
            _ => None,
        }
    }

    pub const fn raw(self) -> u32 {
        self as u32
    }
}

/// The eleven words `FUN_0061cd14` writes, in record order.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MenuFrameSnapshot {
    pub depth: FrameDepth,
    pub node: NodeId,
    pub node_type: NodeType,
    pub class: MenuClass,
    pub cursor_token: RowToken,
    pub current_token: RowToken,
    pub cursor: RowIndex,
    pub visible_ordinal: VisibleOrdinal,
    pub visible_count: VisibleCount,
    pub page: PageNumber,
    pub scroll_top: ScrollTop,
}

const _: () = assert!(core::mem::size_of::<MenuFrameSnapshot>() == 0x2c);

impl MenuFrameSnapshot {
    /// The words written before the frame is read.
    pub const DETACHED: Self = Self {
        depth: FrameDepth::First,
        node: NodeId::DETACHED,
        node_type: NodeType::DETACHED,
        class: MenuClass(0),
        cursor_token: RowToken(0),
        current_token: RowToken(0),
        cursor: RowIndex(0),
        visible_ordinal: VisibleOrdinal(0),
        visible_count: VisibleCount(0),
        page: PageNumber(0),
        scroll_top: ScrollTop(0),
    };

    pub const fn words(&self) -> [u32; 11] {
        [
            self.depth.raw(),
            self.node.0,
            self.node_type.0,
            self.class.0,
            self.cursor_token.0,
            self.current_token.0,
            self.cursor.0,
            self.visible_ordinal.0,
            self.visible_count.0,
            self.page.0,
            self.scroll_top.0,
        ]
    }
}

/// One word store of `FUN_0061cd14`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapshotWord {
    Depth(FrameDepth),
    Node(NodeId),
    NodeType(NodeType),
    Class(MenuClass),
    CursorToken(RowToken),
    CurrentToken(RowToken),
    Cursor(RowIndex),
    VisibleOrdinal(VisibleOrdinal),
    VisibleCount(VisibleCount),
    Page(PageNumber),
    ScrollTop(ScrollTop),
}

pub trait SnapshotSink {
    fn store(&mut self, word: SnapshotWord);
}

impl SnapshotSink for MenuFrameSnapshot {
    fn store(&mut self, word: SnapshotWord) {
        match word {
            SnapshotWord::Depth(value) => self.depth = value,
            SnapshotWord::Node(value) => self.node = value,
            SnapshotWord::NodeType(value) => self.node_type = value,
            SnapshotWord::Class(value) => self.class = value,
            SnapshotWord::CursorToken(value) => self.cursor_token = value,
            SnapshotWord::CurrentToken(value) => self.current_token = value,
            SnapshotWord::Cursor(value) => self.cursor = value,
            SnapshotWord::VisibleOrdinal(value) => self.visible_ordinal = value,
            SnapshotWord::VisibleCount(value) => self.visible_count = value,
            SnapshotWord::Page(value) => self.page = value,
            SnapshotWord::ScrollTop(value) => self.scroll_top = value,
        }
    }
}

/// A node's `{token, child}` pairs, indexed without a bound.
pub trait RowValues: Copy {
    fn is_present(self) -> bool;
    fn token(self, row: RowIndex) -> RowToken;
    fn has_child(self, row: RowIndex) -> bool;
}

/// A node record: id, type, class and rows per page at +0..+0xc, parent at
/// +0x1c, values at +0x20.
pub trait FrameNode: Copy {
    type Values: RowValues;
    fn id(self) -> NodeId;
    fn node_type(self) -> NodeType;
    fn class(self) -> MenuClass;
    fn rows_per_page(self) -> RowsPerPage;
    fn parent(self) -> Option<Self>;
    fn values(self) -> Self::Values;
}

/// The cells `FUN_0061ca2c` and `FUN_0061cd14` read: the open node
/// `0x009571d0`, cursor `0x009571d8`, visible ordinal `0x009571dc`, visible
/// count `0x009571e0`, scroll top `0x009571e4` and saved cursor `0x0095732c`.
pub trait FrameCells {
    type Node: FrameNode;
    fn current(&self) -> Option<Self::Node>;
    /// The open node read again after it was found present.
    fn reload(&self) -> Self::Node;
    fn cursor(&self) -> RowIndex;
    fn visible_ordinal(&self) -> VisibleOrdinal;
    fn visible_count(&self) -> VisibleCount;
    fn scroll_top(&self) -> ScrollTop;
    fn saved_cursor(&self) -> RowIndex;
    fn page(&self, rows: RowsPerPage, visible: VisibleOrdinal) -> PageNumber;

    fn classify(&self) -> FrameDepth
    where
        Self: Sized,
    {
        classify(self)
    }
}

fn cursor_token<C: FrameCells>(cells: &C, frame: C::Node) -> RowToken {
    let cursor = cells.cursor();
    frame.values().token(cursor)
}

fn cursor_has_child<C: FrameCells>(cells: &C, frame: C::Node) -> bool {
    let cursor = cells.cursor();
    frame.values().has_child(cursor)
}

fn opens_child<C: FrameCells>(cells: &C, frame: C::Node) -> bool {
    match frame.parent() {
        None => cursor_has_child(cells, frame),
        Some(parent) if parent.id() == frame.id() => false,
        Some(parent) if parent.class() != MenuClass::MENU_PAGE => true,
        Some(_) => cursor_has_child(cells, frame),
    }
}

fn first_depth<C: FrameCells>(cells: &C, frame: C::Node) -> bool {
    match frame.class().0 {
        0x5 | 0x47 | 0xd0 | 0xdf | 0xe0 | 0x185 | 0x1b9 | 0x1bb | 0x220 | 0x240 | 0x245 | 0x246
        | 0x247 => true,
        0x2d | 0x43 | 0xad | 0xdc | 0xe6 | 0xfc | 0x100 | 0x101 | 0x102 => false,
        0xed if cursor_token(cells, frame) == RowToken(3) => false,
        0xe5 if cursor_token(cells, frame) == RowToken(0) => false,
        0x141 if cursor_token(cells, frame) == RowToken(0x1b) => false,
        0x7 if cursor_token(cells, frame) == RowToken(1) => true,
        0x166 if cursor_token(cells, frame) == RowToken(0) => true,
        0x27 if cursor_token(cells, frame) == RowToken(0) => false,
        _ => opens_child(cells, frame),
    }
}

/// FUN_0061ca2c. A missing frame is `First`.
#[inline(always)]
pub fn classify<C: FrameCells>(cells: &C) -> FrameDepth {
    let Some(frame) = cells.current() else {
        return FrameDepth::First;
    };
    let first = first_depth(cells, frame);
    if frame.id() == NodeId::ROOT {
        FrameDepth::Root
    } else if first {
        FrameDepth::First
    } else {
        FrameDepth::Second
    }
}

/// FUN_0061cd14 after its null-destination check: the detached defaults, then
/// the frame words when a frame is open.
#[inline(always)]
pub fn fill<C: FrameCells, S: SnapshotSink>(cells: &C, sink: &mut S) {
    let detached = MenuFrameSnapshot::DETACHED;
    sink.store(SnapshotWord::Depth(detached.depth));
    sink.store(SnapshotWord::Node(detached.node));
    sink.store(SnapshotWord::NodeType(detached.node_type));
    sink.store(SnapshotWord::Class(detached.class));
    sink.store(SnapshotWord::CursorToken(detached.cursor_token));
    sink.store(SnapshotWord::CurrentToken(detached.current_token));
    sink.store(SnapshotWord::Cursor(detached.cursor));
    sink.store(SnapshotWord::VisibleOrdinal(detached.visible_ordinal));
    sink.store(SnapshotWord::VisibleCount(detached.visible_count));
    sink.store(SnapshotWord::Page(detached.page));
    sink.store(SnapshotWord::ScrollTop(detached.scroll_top));
    if cells.current().is_none() {
        return;
    }

    sink.store(SnapshotWord::Depth(cells.classify()));
    let frame = cells.reload();
    sink.store(SnapshotWord::Node(frame.id()));
    sink.store(SnapshotWord::NodeType(frame.node_type()));
    sink.store(SnapshotWord::Class(frame.class()));
    let values = frame.values();
    if values.is_present() {
        sink.store(SnapshotWord::CursorToken(values.token(cells.cursor())));
        let current = cells.saved_cursor();
        sink.store(SnapshotWord::CurrentToken(frame.values().token(current)));
    }
    sink.store(SnapshotWord::Cursor(cells.cursor()));
    let visible = cells.visible_ordinal();
    sink.store(SnapshotWord::VisibleOrdinal(visible));
    sink.store(SnapshotWord::VisibleCount(cells.visible_count()));
    let rows = frame.rows_per_page();
    sink.store(SnapshotWord::Page(cells.page(rows, visible)));
    sink.store(SnapshotWord::ScrollTop(cells.scroll_top()));
}
