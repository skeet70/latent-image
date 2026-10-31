//! Owned modal-75 lifecycle actions and menu-frame snapshot services.

mod lifecycle;

const EDITOR_QUERY: u32 = 0x53;
const EDITOR_SESSION_CELL: usize = 0x0095_4d08;
const MODAL_STATE_CELL: usize = 0x0146_761e;
const EDITOR_CANCEL_STATE: ModalState = ModalState::AfterCancel;

#[derive(Clone, Copy, PartialEq, Eq)]
enum QueryAnswer {
    Inactive,
    Active,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorSession {
    Closed,
    Open,
}

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum ModalState {
    AfterCancel = 0x1d,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct TransitionResult(u32);

impl QueryAnswer {
    fn parse(word: u32) -> Self {
        if word == 0 { Self::Inactive } else { Self::Active }
    }
}

impl EditorSession {
    fn parse(word: u32) -> Self {
        if word == 1 { Self::Open } else { Self::Closed }
    }
}

impl ModalState {
    fn word(self) -> u16 {
        self as u16
    }
}

impl TransitionResult {
    fn cancelled(self) -> bool {
        matches!(self.0, 0xffff_ffff | 2)
    }

    fn word(self) -> u32 {
        self.0
    }
}

unsafe extern "C" {
    fn rb_call_modal_query(query: u32) -> u32;
    fn rb_open_editor_cancel_stage();
    fn rb_call_transition() -> u32;
}

// FUN_00c77794: query 0x53 nonzero admits only while 0x00954d08 is 1; results
// -1 and 2 write state 0x1d after stage 0xf/0x1a.
#[no_mangle]
pub unsafe extern "C" fn rb_ui_editor_cancel() -> u32 {
    let gate = QueryAnswer::parse(unsafe { rb_call_modal_query(EDITOR_QUERY) });
    if gate == QueryAnswer::Active
        && EditorSession::parse(unsafe {
            core::ptr::read_volatile(EDITOR_SESSION_CELL as *const u32)
        }) == EditorSession::Closed
    {
        return 0;
    }
    unsafe { rb_open_editor_cancel_stage() };
    let result = TransitionResult(unsafe { rb_call_transition() });
    if result.cancelled() {
        unsafe {
            core::ptr::write_volatile(MODAL_STATE_CELL as *mut u16, EDITOR_CANCEL_STATE.word())
        }
    }
    result.word()
}

/// Every read is a volatile load in the stock bodies' order; the writer stores
/// the detached defaults before the frame words.
pub mod frame {
    use core::mem::MaybeUninit;
    use core::ptr::{addr_of_mut, read_volatile, write_volatile};

    const CURRENT_FRAME_CELL: usize = 0x0095_71d0;
    const CURSOR_CELL: usize = 0x0095_71d8;
    const VISIBLE_ORDINAL_CELL: usize = 0x0095_71dc;
    const SCROLL_CELL: usize = 0x0095_71e0;
    const NAVIGATION_CELL: usize = 0x0095_71e4;
    const CURRENT_ROW_CELL: usize = 0x0095_732c;

    const ROOT_SNAPSHOT: usize = 0x0094_4414;
    const FIRST_DEPTH_SNAPSHOTS: usize = 0x0093_eb74;
    const SECOND_DEPTH_SNAPSHOTS: usize = 0x0094_12d4;

    #[repr(u32)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum FrameDepth {
        Root = 0,
        First = 1,
        Second = 2,
    }

    #[repr(transparent)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct NodeId(u32);

    #[repr(transparent)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct NodeType(u32);

    #[repr(transparent)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct MenuClass(u32);

    #[repr(transparent)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct RowToken(u32);

    #[repr(transparent)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct RowIndex(u32);

    #[repr(transparent)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct ScrollState(u32);

    /// `(rows_per_page + visible_ordinal) / rows_per_page`.
    #[repr(transparent)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct PageNumber(u32);

    #[repr(transparent)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct NavigationWord(u32);

    #[repr(u32)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum SnapshotResult {
        NoDestination = 0,
        Written = 1,
    }

    #[repr(C)]
    pub struct MenuFrameSnapshot {
        depth: FrameDepth,
        node: NodeId,
        node_type: NodeType,
        class: MenuClass,
        cursor_token: RowToken,
        current_token: RowToken,
        cursor: RowIndex,
        visible_ordinal: RowIndex,
        scroll: ScrollState,
        page: PageNumber,
        navigation: NavigationWord,
    }

    const _: () = assert!(core::mem::size_of::<MenuFrameSnapshot>() == 0x2c);

    impl NodeId {
        const ROOT: Self = Self(0);
        const DETACHED: Self = Self(0x86);
    }

    impl NodeType {
        const DETACHED: Self = Self(0xc);
    }

    impl MenuClass {
        const MENU_PAGE: Self = Self(5);
    }

    unsafe extern "C" {
        /// FUN_0004d978; a zero divisor enters the loader's fatal SVC 0x1010.
        fn rb_call_loader_uidiv(numerator: u32, denominator: u32) -> u32;
    }

    fn cell(address: usize) -> u32 {
        unsafe { read_volatile(address as *const u32) }
    }

    fn row_cell(address: usize) -> RowIndex {
        RowIndex(cell(address))
    }

    /// A stock menu node: id, type, class and rows per page at +0..+0xc,
    /// parent at +0x1c, values at +0x20.
    #[derive(Clone, Copy)]
    struct Frame(*const u32);

    /// `{token, child}` pairs indexed without a bound.
    #[derive(Clone, Copy)]
    struct Values(*const u32);

    impl Frame {
        fn current() -> Option<Self> {
            let frame = Self::reload();
            (!frame.0.is_null()).then_some(frame)
        }

        fn reload() -> Self {
            Self(cell(CURRENT_FRAME_CELL) as *const u32)
        }

        fn word(self, index: usize) -> u32 {
            unsafe { read_volatile(self.0.wrapping_add(index)) }
        }

        fn id(self) -> NodeId {
            NodeId(self.word(0))
        }

        fn node_type(self) -> NodeType {
            NodeType(self.word(1))
        }

        fn class(self) -> MenuClass {
            MenuClass(self.word(2))
        }

        fn rows_per_page(self) -> u32 {
            self.word(3)
        }

        fn parent(self) -> Option<Self> {
            let parent = Self(self.word(7) as *const u32);
            (!parent.0.is_null()).then_some(parent)
        }

        fn values(self) -> Values {
            Values(self.word(8) as *const u32)
        }

        fn cursor_token(self) -> RowToken {
            let cursor = row_cell(CURSOR_CELL);
            self.values().token(cursor)
        }

        fn cursor_has_child(self) -> bool {
            let cursor = row_cell(CURSOR_CELL);
            self.values().child(cursor) != 0
        }

        fn opens_child(self) -> bool {
            match self.parent() {
                None => self.cursor_has_child(),
                Some(parent) if parent.id() == self.id() => false,
                Some(parent) if parent.class() != MenuClass::MENU_PAGE => true,
                Some(_) => self.cursor_has_child(),
            }
        }
    }

    impl Values {
        fn entry(self, row: RowIndex, word: usize) -> u32 {
            let index = (row.0 as usize).wrapping_mul(2).wrapping_add(word);
            unsafe { read_volatile(self.0.wrapping_add(index)) }
        }

        fn token(self, row: RowIndex) -> RowToken {
            RowToken(self.entry(row, 0))
        }

        fn child(self, row: RowIndex) -> u32 {
            self.entry(row, 1)
        }
    }

    fn first_depth(frame: Frame) -> bool {
        match frame.class().0 {
            0x5 | 0x47 | 0xd0 | 0xdf | 0xe0 | 0x185 | 0x1b9 | 0x1bb | 0x220 | 0x240 | 0x245
            | 0x246 | 0x247 => true,
            0x2d | 0x43 | 0xad | 0xdc | 0xe6 | 0xfc | 0x100 | 0x101 | 0x102 => false,
            0xed if frame.cursor_token() == RowToken(3) => false,
            0xe5 if frame.cursor_token() == RowToken(0) => false,
            0x141 if frame.cursor_token() == RowToken(0x1b) => false,
            0x7 if frame.cursor_token() == RowToken(1) => true,
            0x166 if frame.cursor_token() == RowToken(0) => true,
            0x27 if frame.cursor_token() == RowToken(0) => false,
            _ => frame.opens_child(),
        }
    }

    /// FUN_0061ca2c. A missing frame is `First`.
    #[no_mangle]
    #[inline(never)]
    pub extern "C" fn rb_menu_frame_classify() -> FrameDepth {
        let Some(frame) = Frame::current() else {
            return FrameDepth::First;
        };
        let first = first_depth(frame);
        if frame.id() == NodeId::ROOT {
            FrameDepth::Root
        } else if first {
            FrameDepth::First
        } else {
            FrameDepth::Second
        }
    }

    fn store<T>(field: *mut T, value: T) {
        unsafe { write_volatile(field, value) }
    }

    /// # Safety
    /// `dest` is null, or 4-byte aligned and writable for 0x2c bytes.
    #[inline(always)]
    unsafe fn write_snapshot(dest: *mut MenuFrameSnapshot) -> SnapshotResult {
        if dest.is_null() {
            return SnapshotResult::NoDestination;
        }
        let depth = unsafe { addr_of_mut!((*dest).depth) };
        let node = unsafe { addr_of_mut!((*dest).node) };
        let node_type = unsafe { addr_of_mut!((*dest).node_type) };
        let class = unsafe { addr_of_mut!((*dest).class) };
        let cursor_token = unsafe { addr_of_mut!((*dest).cursor_token) };
        let current_token = unsafe { addr_of_mut!((*dest).current_token) };
        let cursor = unsafe { addr_of_mut!((*dest).cursor) };
        let visible_ordinal = unsafe { addr_of_mut!((*dest).visible_ordinal) };
        let scroll = unsafe { addr_of_mut!((*dest).scroll) };
        let page = unsafe { addr_of_mut!((*dest).page) };
        let navigation = unsafe { addr_of_mut!((*dest).navigation) };

        store(depth, FrameDepth::First);
        store(node, NodeId::DETACHED);
        store(node_type, NodeType::DETACHED);
        store(class, MenuClass(0));
        store(cursor_token, RowToken(0));
        store(current_token, RowToken(0));
        store(cursor, RowIndex(0));
        store(visible_ordinal, RowIndex(0));
        store(scroll, ScrollState(0));
        store(page, PageNumber(0));
        store(navigation, NavigationWord(0));
        if Frame::current().is_none() {
            return SnapshotResult::Written;
        }

        store(depth, rb_menu_frame_classify());
        let frame = Frame::reload();
        store(node, frame.id());
        store(node_type, frame.node_type());
        store(class, frame.class());
        let values = frame.values();
        if !values.0.is_null() {
            store(cursor_token, values.token(row_cell(CURSOR_CELL)));
            let current = row_cell(CURRENT_ROW_CELL);
            store(current_token, frame.values().token(current));
        }
        store(cursor, row_cell(CURSOR_CELL));
        let visible = row_cell(VISIBLE_ORDINAL_CELL);
        store(visible_ordinal, visible);
        store(scroll, ScrollState(cell(SCROLL_CELL)));
        let rows = frame.rows_per_page();
        store(page, PageNumber(unsafe { rb_call_loader_uidiv(rows.wrapping_add(visible.0), rows) }));
        store(navigation, NavigationWord(cell(NAVIGATION_CELL)));
        SnapshotResult::Written
    }

    /// FUN_0061cd14.
    ///
    /// # Safety
    /// `dest` is null, or 4-byte aligned and writable for 0x2c bytes.
    #[no_mangle]
    #[inline(never)]
    pub unsafe extern "C" fn rb_menu_frame_snapshot(dest: *mut MenuFrameSnapshot) -> SnapshotResult {
        unsafe { write_snapshot(dest) }
    }

    fn indexed_slot(table: usize, root: NodeId) -> *mut MenuFrameSnapshot {
        table.wrapping_add((root.0 as usize).wrapping_mul(0x2c)) as *mut MenuFrameSnapshot
    }

    #[no_mangle]
    #[inline(never)]
    extern "C" fn rb_menu_frame_store_root_depth() -> SnapshotResult {
        unsafe { write_snapshot(ROOT_SNAPSHOT as *mut MenuFrameSnapshot) }
    }

    #[no_mangle]
    #[inline(never)]
    extern "C" fn rb_menu_frame_store_first_depth(root: NodeId) -> SnapshotResult {
        unsafe { rb_menu_frame_snapshot(indexed_slot(FIRST_DEPTH_SNAPSHOTS, root)) }
    }

    #[no_mangle]
    #[inline(never)]
    extern "C" fn rb_menu_frame_store_second_depth(root: NodeId) -> SnapshotResult {
        unsafe { rb_menu_frame_snapshot(indexed_slot(SECOND_DEPTH_SNAPSHOTS, root)) }
    }

    /// FUN_0061ceac: saves a fresh snapshot in the slot for the frame's depth,
    /// indexed by `root` without a bound at depths one and two.
    #[no_mangle]
    pub extern "C" fn rb_menu_frame_store(root: NodeId) -> SnapshotResult {
        match rb_menu_frame_classify() {
            FrameDepth::Root => rb_menu_frame_store_root_depth(),
            FrameDepth::First => rb_menu_frame_store_first_depth(root),
            FrameDepth::Second => rb_menu_frame_store_second_depth(root),
        }
    }

    fn fresh() -> MenuFrameSnapshot {
        let mut snapshot = MaybeUninit::<MenuFrameSnapshot>::uninit();
        unsafe {
            rb_menu_frame_snapshot(snapshot.as_mut_ptr());
            snapshot.assume_init()
        }
    }

    /// FUN_0061d0e4.
    #[no_mangle]
    pub extern "C" fn rb_menu_frame_depth() -> FrameDepth {
        fresh().depth
    }

    /// FUN_0061d154.
    #[no_mangle]
    pub extern "C" fn rb_menu_frame_node() -> NodeId {
        fresh().node
    }

    /// FUN_0061d1a4.
    #[no_mangle]
    pub extern "C" fn rb_menu_frame_node_type() -> NodeType {
        fresh().node_type
    }

    /// FUN_0061d1f4.
    #[no_mangle]
    pub extern "C" fn rb_menu_frame_class() -> MenuClass {
        fresh().class
    }

    /// FUN_0061d244.
    #[no_mangle]
    pub extern "C" fn rb_menu_frame_cursor_token() -> RowToken {
        fresh().cursor_token
    }

    /// FUN_0061d294.
    #[no_mangle]
    pub extern "C" fn rb_menu_frame_current_token() -> RowToken {
        fresh().current_token
    }

    /// FUN_0061d2e4.
    #[no_mangle]
    pub extern "C" fn rb_menu_frame_page() -> PageNumber {
        fresh().page
    }

    /// FUN_0061d334.
    #[no_mangle]
    pub extern "C" fn rb_menu_frame_navigation() -> NavigationWord {
        fresh().navigation
    }
}
