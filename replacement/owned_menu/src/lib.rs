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

/// The guest snapshot service over `menu_engine::frame`: every read is a
/// volatile load and every store a volatile store, in the stock bodies' order.
pub mod frame {
    use core::mem::MaybeUninit;
    use core::ptr::{addr_of_mut, read_volatile, write_volatile};

    pub use crate::menu_engine::frame::{
        FrameDepth, MenuClass, MenuFrameSnapshot, NodeId, NodeType, PageNumber, RowToken, ScrollTop, VisibleCount,
    };
    use crate::menu_engine::frame::{
        self as shared, FrameCells, FrameNode, RowIndex, RowValues, RowsPerPage, SnapshotSink, SnapshotWord,
        VisibleOrdinal,
    };

    const CURRENT_FRAME_CELL: usize = 0x0095_71d0;
    const CURSOR_CELL: usize = 0x0095_71d8;
    const VISIBLE_ORDINAL_CELL: usize = 0x0095_71dc;
    const VISIBLE_COUNT_CELL: usize = 0x0095_71e0;
    const SCROLL_TOP_CELL: usize = 0x0095_71e4;
    const CURRENT_ROW_CELL: usize = 0x0095_732c;

    const ROOT_SNAPSHOT: usize = 0x0094_4414;
    const FIRST_DEPTH_SNAPSHOTS: usize = 0x0093_eb74;
    const SECOND_DEPTH_SNAPSHOTS: usize = 0x0094_12d4;

    #[repr(u32)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum SnapshotResult {
        NoDestination = 0,
        Written = 1,
    }

    unsafe extern "C" {
        /// FUN_0004d978; a zero divisor enters the loader's fatal SVC 0x1010.
        fn rb_call_loader_uidiv(numerator: u32, denominator: u32) -> u32;
    }

    fn cell(address: usize) -> u32 {
        unsafe { read_volatile(address as *const u32) }
    }

    /// A stock node record.
    #[derive(Clone, Copy)]
    struct Frame(*const u32);

    /// `{token, child}` pairs indexed without a bound.
    #[derive(Clone, Copy)]
    struct Values(*const u32);

    impl Frame {
        fn word(self, index: usize) -> u32 {
            unsafe { read_volatile(self.0.wrapping_add(index)) }
        }
    }

    impl Values {
        fn entry(self, row: RowIndex, word: usize) -> u32 {
            let index = (row.get() as usize).wrapping_mul(2).wrapping_add(word);
            unsafe { read_volatile(self.0.wrapping_add(index)) }
        }
    }

    impl RowValues for Values {
        fn is_present(self) -> bool {
            !self.0.is_null()
        }

        fn token(self, row: RowIndex) -> RowToken {
            RowToken::new(self.entry(row, 0))
        }

        fn has_child(self, row: RowIndex) -> bool {
            self.entry(row, 1) != 0
        }
    }

    impl FrameNode for Frame {
        type Values = Values;

        fn id(self) -> NodeId {
            NodeId::new(self.word(0))
        }

        fn node_type(self) -> NodeType {
            NodeType::new(self.word(1))
        }

        fn class(self) -> MenuClass {
            MenuClass::new(self.word(2))
        }

        fn rows_per_page(self) -> RowsPerPage {
            RowsPerPage::new(self.word(3))
        }

        fn parent(self) -> Option<Self> {
            let parent = Self(self.word(7) as *const u32);
            (!parent.0.is_null()).then_some(parent)
        }

        fn values(self) -> Values {
            Values(self.word(8) as *const u32)
        }
    }

    struct Live;

    impl FrameCells for Live {
        type Node = Frame;

        fn current(&self) -> Option<Frame> {
            let frame = self.reload();
            (!frame.0.is_null()).then_some(frame)
        }

        fn reload(&self) -> Frame {
            Frame(cell(CURRENT_FRAME_CELL) as *const u32)
        }

        fn cursor(&self) -> RowIndex {
            RowIndex::new(cell(CURSOR_CELL))
        }

        fn visible_ordinal(&self) -> VisibleOrdinal {
            VisibleOrdinal::new(cell(VISIBLE_ORDINAL_CELL))
        }

        fn visible_count(&self) -> VisibleCount {
            VisibleCount::new(cell(VISIBLE_COUNT_CELL))
        }

        fn scroll_top(&self) -> ScrollTop {
            ScrollTop::new(cell(SCROLL_TOP_CELL))
        }

        fn saved_cursor(&self) -> RowIndex {
            RowIndex::new(cell(CURRENT_ROW_CELL))
        }

        fn page(&self, rows: RowsPerPage, visible: VisibleOrdinal) -> PageNumber {
            PageNumber::new(unsafe { rb_call_loader_uidiv(rows.get().wrapping_add(visible.get()), rows.get()) })
        }

        fn classify(&self) -> FrameDepth {
            rb_menu_frame_classify()
        }
    }

    /// FUN_0061ca2c. A missing frame is `First`.
    #[no_mangle]
    #[inline(never)]
    pub extern "C" fn rb_menu_frame_classify() -> FrameDepth {
        shared::classify(&Live)
    }

    struct Destination(*mut MenuFrameSnapshot);

    impl SnapshotSink for Destination {
        fn store(&mut self, word: SnapshotWord) {
            let dest = self.0;
            unsafe {
                match word {
                    SnapshotWord::Depth(value) => write_volatile(addr_of_mut!((*dest).depth), value),
                    SnapshotWord::Node(value) => write_volatile(addr_of_mut!((*dest).node), value),
                    SnapshotWord::NodeType(value) => write_volatile(addr_of_mut!((*dest).node_type), value),
                    SnapshotWord::Class(value) => write_volatile(addr_of_mut!((*dest).class), value),
                    SnapshotWord::CursorToken(value) => write_volatile(addr_of_mut!((*dest).cursor_token), value),
                    SnapshotWord::CurrentToken(value) => write_volatile(addr_of_mut!((*dest).current_token), value),
                    SnapshotWord::Cursor(value) => write_volatile(addr_of_mut!((*dest).cursor), value),
                    SnapshotWord::VisibleOrdinal(value) => write_volatile(addr_of_mut!((*dest).visible_ordinal), value),
                    SnapshotWord::VisibleCount(value) => write_volatile(addr_of_mut!((*dest).visible_count), value),
                    SnapshotWord::Page(value) => write_volatile(addr_of_mut!((*dest).page), value),
                    SnapshotWord::ScrollTop(value) => write_volatile(addr_of_mut!((*dest).scroll_top), value),
                }
            }
        }
    }

    /// # Safety
    /// `dest` is null, or 4-byte aligned and writable for 0x2c bytes.
    #[inline(always)]
    unsafe fn write_snapshot(dest: *mut MenuFrameSnapshot) -> SnapshotResult {
        if dest.is_null() {
            return SnapshotResult::NoDestination;
        }
        shared::fill(&Live, &mut Destination(dest));
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
        table.wrapping_add((root.get() as usize).wrapping_mul(0x2c)) as *mut MenuFrameSnapshot
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
    pub extern "C" fn rb_menu_frame_navigation() -> ScrollTop {
        fresh().scroll_top
    }
}
