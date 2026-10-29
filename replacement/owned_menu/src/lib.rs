//! The owned modal-75 editor-cancel action, replacing FUN_00c77794.

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
