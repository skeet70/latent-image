use super::{EditorSession, QueryAnswer, TransitionResult, EDITOR_QUERY, EDITOR_SESSION_CELL};
use core::ptr::{read_volatile, write_volatile};

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct FrameClass(u32);
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct RowToken(u32);
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct NodeId(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct ModalState(u16);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct Notification(u32);
#[repr(u32)]
#[derive(Clone, Copy)]
enum StageKind { Direct = 2, Enter = 14, Exit = 15 }
#[repr(u32)]
#[derive(Clone, Copy)]
enum StageId { Cancel = 26, Accept = 29 }
#[repr(transparent)]
#[derive(Clone, Copy)]
struct PropertyId(u32);
#[repr(transparent)]
struct PropertyCode(u32);
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct ModeWord(u32);

#[repr(u32)]
#[derive(Clone, Copy)]
enum BinaryChoice { Off = 0, On = 1 }
#[repr(u32)]
#[derive(Clone, Copy)]
enum ModeRequest { Select = 1, Apply = 2 }
#[repr(u32)]
#[derive(Clone, Copy)]
enum PostludeKind { Menu = 9 }
#[repr(transparent)]
struct EditorId(u32);
#[repr(transparent)]
struct ByteCount(u32);
#[repr(transparent)]
struct PropertyIndex(u32);
#[repr(transparent)]
struct FillWord(u32);
#[repr(transparent)]
struct ModeCode(u32);
#[repr(transparent)]
struct OperationId(u32);

struct WordCell(usize);
impl WordCell {
    fn read(&self) -> ModeWord { ModeWord(unsafe { read_volatile(self.0 as *const u32) }) }
    fn write(&self, word: ModeWord) { unsafe { write_volatile(self.0 as *mut u32, word.0) } }
    fn enabled(&self) -> bool { self.read() == ModeWord(1) }
}
const ACTIVATION: WordCell = WordCell(0x0095_44f8);
const MODE_A: WordCell = WordCell(0x0095_0474);
const MODE_B: WordCell = WordCell(0x0095_0478);
const MODE_C: WordCell = WordCell(0x0095_4500);
const EDIT_CHOICE: WordCell = WordCell(0x0095_4cf4);
const BLINK: WordCell = WordCell(0x0095_04a8);

unsafe extern "C" {
    fn rb_call_modal_query(query: u32) -> u32;
    fn rb_call_transition() -> u32;
    fn rb_call_stage_open(kind: StageKind, id: StageId);
    fn rb_call_redraw(notification: Notification);
    fn rb_call_postlude(kind: PostludeKind);
    fn rb_lifecycle_class() -> FrameClass;
    fn rb_lifecycle_token() -> RowToken;
    fn rb_lifecycle_node() -> NodeId;
    fn rb_lifecycle_mode_d0() -> u32;
    fn rb_lifecycle_mode_e8() -> u32;
    fn rb_lifecycle_confirm(query: u32);
    fn rb_lifecycle_current(property: PropertyId, index: PropertyIndex) -> PropertyCode;
    fn rb_lifecycle_reset(value: BinaryChoice);
    fn rb_lifecycle_prepare();
    fn rb_lifecycle_refresh();
    fn rb_lifecycle_enable(value: BinaryChoice);
    fn rb_lifecycle_mode_refresh_a();
    fn rb_lifecycle_mode_refresh_b();
    fn rb_lifecycle_mode_apply(value: ModeRequest);
    fn rb_lifecycle_mode_flag_a(value: BinaryChoice);
    fn rb_lifecycle_mode_flag_b(value: BinaryChoice);
    fn rb_lifecycle_mode_flag_c(value: BinaryChoice);
    fn rb_lifecycle_select_29(value: BinaryChoice);
    fn rb_lifecycle_select_30(value: BinaryChoice);
    fn rb_lifecycle_prepare_2a(value: OperationId);
    fn rb_lifecycle_edit_prepare(value: EditorId);
    fn rb_lifecycle_edit_open(value: EditorId);
    fn rb_lifecycle_buffer_copy(destination: *mut u8, source: *const u8, length: ByteCount);
    fn rb_lifecycle_buffer_clear(destination: *mut u8, value: FillWord, length: ByteCount);
    fn rb_lifecycle_words_clear(destination: *mut u8, value: FillWord, length: ByteCount);
    fn rb_lifecycle_mode_set(value: ModeCode);
    fn rb_lifecycle_wb_ready() -> u32;
    fn rb_lifecycle_wb_enable(value: BinaryChoice);
}
fn class(class: FrameClass) -> bool { unsafe { rb_lifecycle_class() == class } }
fn token(token: RowToken) -> bool { unsafe { rb_lifecycle_token() == token } }
fn row(class_id: FrameClass, row: RowToken) -> bool { class(class_id) && token(row) }
fn node(node: NodeId) -> bool { unsafe { rb_lifecycle_node() == node } }
fn mode_d0() -> bool { unsafe { rb_lifecycle_mode_d0() != 0 } }
fn mode_e8() -> bool { unsafe { rb_lifecycle_mode_e8() != 0 } }
fn state(state: ModalState) { unsafe { write_volatile(0x0146_761e as *mut u16, state.0) } }
fn notify(notification: Notification) {
    unsafe { rb_call_redraw(notification) };
    BLINK.write(ModeWord(1));
}
fn transition(kind: StageKind, id: StageId) -> TransitionResult {
    unsafe { rb_call_stage_open(kind, id) };
    TransitionResult(unsafe { rb_call_transition() })
}
fn enter() -> TransitionResult { transition(StageKind::Enter, StageId::Accept) }
fn enter_state(next: ModalState) -> u32 {
    let result = enter();
    if result.cancelled() { state(next); notify(Notification(0x92a)); }
    result.word()
}
fn denied() -> bool {
    QueryAnswer::parse(unsafe { rb_call_modal_query(EDITOR_QUERY) }) == QueryAnswer::Active
}
fn white_balance() -> bool { row(FrameClass(5), RowToken(0x1c)) || class(FrameClass(0xed)) }

#[no_mangle]
pub unsafe extern "C" fn rb_ui_editor_confirm() -> u32 {
    let query = unsafe { rb_call_modal_query(EDITOR_QUERY) };
    if QueryAnswer::parse(query) == QueryAnswer::Active
        && EditorSession::parse(unsafe { read_volatile(EDITOR_SESSION_CELL as *const u32) }) == EditorSession::Closed
    { return 0; }
    unsafe { rb_lifecycle_confirm(query) };
    let result = TransitionResult(unsafe { rb_call_transition() });
    if result.cancelled() { unsafe { rb_call_postlude(PostludeKind::Menu) }; }
    result.word()
}

#[no_mangle]
pub unsafe extern "C" fn rb_ui_leave() -> u32 {
    if denied() { return 0; }
    if node(NodeId(0)) { return transition(StageKind::Direct, StageId::Accept).word(); }
    if class(FrameClass(5)) || node(NodeId(0x5b)) {
        return transition(StageKind::Direct, StageId::Accept).word();
    }
    if ACTIVATION.enabled() || MODE_A.enabled() || MODE_B.enabled() {
        if class(FrameClass(0xee)) { return transition(StageKind::Exit, StageId::Accept).word(); }
        if !white_balance() || (!mode_d0() && !mode_d0()) {
            return transition(StageKind::Direct, StageId::Cancel).word();
        }
        return transition(StageKind::Direct, StageId::Accept).word();
    }
    if class(FrameClass(0x167)) {
        let result = transition(StageKind::Exit, StageId::Accept);
        if result.cancelled() { notify(Notification(0x933)); }
        return result.word();
    }
    if class(FrameClass(0xf5)) { return transition(StageKind::Exit, StageId::Cancel).word(); }
    if mode_d0() && white_balance() { return transition(StageKind::Direct, StageId::Accept).word(); }
    if class(FrameClass(0xee)) { return transition(StageKind::Exit, StageId::Accept).word(); }
    if !white_balance() {
        if (class(FrameClass(5)) && (token(RowToken(0x13)) || token(RowToken(0x27)))) || class(FrameClass(0xfe)) {
            return transition(StageKind::Exit, StageId::Cancel).word();
        }
        let result = transition(StageKind::Exit, StageId::Cancel);
        if result.cancelled() && !node(NodeId(0x60)) && !node(NodeId(0x37)) && !node(NodeId(0x61)) {
            notify(Notification(0x933));
        }
        return result.word();
    }
    if !mode_d0() && !mode_d0() { return transition(StageKind::Exit, StageId::Cancel).word(); }
    let result = transition(StageKind::Exit, StageId::Accept);
    if result.cancelled() { notify(Notification(0x930)); }
    result.word()
}

#[no_mangle]
pub unsafe extern "C" fn rb_ui_set() -> u32 {
    if denied() { return 0; }
    if node(NodeId(0)) { return enter().word(); }
    if class(FrameClass(5)) { return set_menu_page(); }
    if mode_d0() && node(NodeId(0x5b)) {
        if row(FrameClass(0xdf), RowToken(0)) { return transition(StageKind::Direct, StageId::Accept).word(); }
        if row(FrameClass(0xdf), RowToken(1)) { return set_node_5b(BinaryChoice::Off); }
        if !class(FrameClass(0xdf)) { return 0; }
        if token(RowToken(2)) { return set_node_5b(BinaryChoice::On); }
        return 0;
    }
    if class(FrameClass(0x184)) || row(FrameClass(5), RowToken(0x2d)) { return enter_state(ModalState(3)); }
    if row(FrameClass(0x141), RowToken(0x1b)) && (!mode_e8() || WordCell(0x0095_6934).read() != ModeWord(2)) {
        return enter_state(ModalState(1));
    }
    if class(FrameClass(0xf5)) { return transition(StageKind::Exit, StageId::Accept).word(); }
    if row(FrameClass(5), RowToken(0x2c)) { return open_dialog(); }
    if row(FrameClass(0x167), RowToken(0)) {
        EDIT_CHOICE.write(ModeWord(0));
        let result = enter();
        if result.word() == u32::MAX {
            state(ModalState(0x20));
            if EDIT_CHOICE.enabled() {
                let code = unsafe { rb_lifecycle_current(PropertyId(0x34), PropertyIndex(0)) };
                let offset = if code.0 == 0 { 0xe0f8 } else { 0xe0f0 };
                let index = unsafe { read_volatile((0x0094_4580 + offset) as *const u16) };
                let source = (0x0095_2030 + usize::from(index) * 2) as *const u8;
                unsafe { rb_lifecycle_buffer_copy(0x0095_43ae as *mut u8, source, ByteCount(0x19)) };
            } else {
                unsafe { rb_lifecycle_buffer_clear(0x0095_43ae as *mut u8, FillWord(0), ByteCount(0x19)) };
            }
            unsafe { rb_lifecycle_words_clear(0x0095_2690 as *mut u8, FillWord(0), ByteCount(0xc)) };
            notify(Notification(0x92a));
        }
        return result.word();
    }
    if row(FrameClass(0x167), RowToken(1)) { return open_edit(ModeWord(1)); }
    if row(FrameClass(0x167), RowToken(2)) { return open_edit(ModeWord(2)); }
    if class(FrameClass(0x26)) { return enter_state(ModalState(4)); }
    if row(FrameClass(0x19), RowToken(0)) {
        WordCell(0x0095_047c).write(ModeWord(0));
        return enter_state(ModalState(0x29));
    }
    if row(FrameClass(0x19), RowToken(1)) {
        WordCell(0x0095_047c).write(ModeWord(1));
        return enter_state(ModalState(0x29));
    }
    if !white_balance() {
        if class(FrameClass(0xee)) { return set_white_balance_exit(); }
        if ACTIVATION.enabled() && (!class(FrameClass(0xed)) || (!token(RowToken(1)) && !token(RowToken(2)))) {
            return transition(StageKind::Direct, StageId::Accept).word();
        }
        if MODE_A.enabled() || MODE_B.enabled() { return transition(StageKind::Direct, StageId::Accept).word(); }
        if MODE_C.enabled() {
            if row(FrameClass(0x47), RowToken(0)) {
                unsafe { rb_lifecycle_mode_set(ModeCode(4)) };
                WordCell(0x0095_37f8).write(ModeWord(1));
                WordCell(0x0095_0108).write(ModeWord(1));
                unsafe { rb_lifecycle_mode_apply(ModeRequest::Apply) };
                return 1;
            }
            let result = enter();
            if result.cancelled() { notify(Notification(0x92b)); }
            return result.word();
        }
        let result = transition(StageKind::Exit, StageId::Accept);
        if result.cancelled() && !row(FrameClass(5), RowToken(0x35)) && !class(FrameClass(0xe8)) {
            notify(Notification(0x930));
        }
        return result.word();
    }
    if class(FrameClass(0xed)) && (token(RowToken(1)) || token(RowToken(2)))
        && unsafe { rb_lifecycle_wb_ready() } == 1 {
        state(ModalState(0xe));
        unsafe {
            rb_call_postlude(PostludeKind::Menu);
            rb_lifecycle_wb_enable(BinaryChoice::On);
            rb_lifecycle_prepare();
            rb_lifecycle_refresh();
            rb_call_redraw(Notification(0x135));
        }
        return 1;
    }
    if row(FrameClass(0xed), RowToken(3)) { return enter().word(); }
    set_white_balance_exit()
}

fn set_node_5b(selection: BinaryChoice) -> u32 {
    let result = transition(StageKind::Direct, StageId::Accept);
    if result.cancelled() {
        unsafe {
            rb_lifecycle_mode_apply(ModeRequest::Apply);
            rb_lifecycle_mode_flag_a(BinaryChoice::On);
            rb_lifecycle_mode_flag_b(selection);
            rb_lifecycle_mode_flag_c(BinaryChoice::On);
        }
        return 1;
    }
    result.word()
}
fn set_white_balance_exit() -> u32 {
    if mode_d0() {
        let result = enter();
        if result.cancelled() { state(ModalState(9)); }
        return result.word();
    }
    if !mode_d0() {
        let result = transition(StageKind::Exit, StageId::Accept);
        if result.cancelled() { notify(Notification(0x930)); }
        return result.word();
    }
    transition(StageKind::Direct, StageId::Accept).word()
}
fn open_dialog() -> u32 {
    state(ModalState(0xd));
    unsafe { rb_lifecycle_prepare(); rb_lifecycle_refresh(); }
    WordCell(0x0095_03c0).write(ModeWord(0));
    unsafe { rb_call_redraw(Notification(0x13d)); rb_call_postlude(PostludeKind::Menu); }
    1
}
fn open_edit(choice: ModeWord) -> u32 {
    state(ModalState(0x26));
    EDIT_CHOICE.write(choice);
    unsafe { rb_lifecycle_edit_prepare(EditorId(0x28)); rb_lifecycle_edit_open(EditorId(0x28)); }
    notify(Notification(0x92a));
    u32::MAX
}
fn set_menu_page() -> u32 {
    if row(FrameClass(5), RowToken(0x32)) || row(FrameClass(0xe4), RowToken(4))
        || row(FrameClass(5), RowToken(0x33)) || row(FrameClass(5), RowToken(0x36))
        || row(FrameClass(0xe4), RowToken(0)) || row(FrameClass(5), RowToken(0x37))
        || row(FrameClass(0xe4), RowToken(3))
        || ((row(FrameClass(0xe5), RowToken(0)) || row(FrameClass(0xe4), RowToken(2))
            || row(FrameClass(5), RowToken(0x7c)) || class(FrameClass(0xe6))) && !mode_e8())
        || row(FrameClass(5), RowToken(0x38)) || class(FrameClass(0xe4)) {
        let result = enter();
        if result.cancelled() { unsafe { rb_lifecycle_reset(BinaryChoice::Off) }; }
        return result.word();
    }
    if mode_e8() && (row(FrameClass(0xe5), RowToken(0)) || row(FrameClass(0xe4), RowToken(2))
        || row(FrameClass(5), RowToken(0x7c)) || class(FrameClass(0xe6))) {
        let result = enter();
        if result.cancelled() {
            state(ModalState(0x2d));
            notify(Notification(0x92c));
        }
        return result.word();
    }
    if white_balance() || (class(FrameClass(5)) && (token(RowToken(0x13)) || token(RowToken(0x27))))
        || class(FrameClass(0xfe)) {
        let result = enter();
        if result.cancelled() { notify(Notification(0x92c)); }
        return result.word();
    }
    if row(FrameClass(5), RowToken(0x14)) || class(FrameClass(0xf5)) { return enter().word(); }
    if row(FrameClass(5), RowToken(0x1e)) || class(FrameClass(0x112)) { return enter_state(ModalState(9)); }
    if row(FrameClass(5), RowToken(8)) { return enter_state(ModalState(10)); }
    if row(FrameClass(5), RowToken(0x17)) { return enter_state(ModalState(2)); }
    if class(FrameClass(5)) && (token(RowToken(0x46)) || token(RowToken(0x68)) || token(RowToken(0xa1))) {
        return enter_state(ModalState(0x10));
    }
    if row(FrameClass(5), RowToken(0x28)) || row(FrameClass(0x1e7), RowToken(1)) || class(FrameClass(299)) {
        return enter_state(ModalState(0x15));
    }
    if class(FrameClass(0x184)) || row(FrameClass(5), RowToken(0x2d)) { return enter_state(ModalState(3)); }
    if row(FrameClass(5), RowToken(0x2c)) { return open_dialog(); }
    if mode_e8() && (row(FrameClass(5), RowToken(0x99)) || row(FrameClass(0x240), RowToken(0))
        || row(FrameClass(0x166), RowToken(0)) || row(FrameClass(5), RowToken(0x83))) {
        WordCell(0x0095_201c).write(ModeWord(1));
        unsafe { rb_lifecycle_enable(BinaryChoice::On) };
        WordCell(0x0095_0104).write(ModeWord(1));
        unsafe { rb_lifecycle_mode_refresh_a(); rb_lifecycle_mode_refresh_b(); rb_lifecycle_mode_apply(ModeRequest::Select); }
        return 1;
    }
    if row(FrameClass(5), RowToken(0xd)) || class(FrameClass(0x1e7)) || class(FrameClass(0x1e8)) || class(FrameClass(299)) {
        return enter_state(ModalState(0x14));
    }
    if row(FrameClass(5), RowToken(0x25)) || class(FrameClass(0xf1)) || class(FrameClass(0xf2))
        || class(FrameClass(0xf3)) || class(FrameClass(0xf4)) || class(FrameClass(0x137)) {
        return enter_state(ModalState(0x12));
    }
    if row(FrameClass(5), RowToken(0x7b)) || class(FrameClass(0x17a)) || class(FrameClass(0x17b)) {
        return enter_state(ModalState(0x13));
    }
    if mode_d0() && row(FrameClass(5), RowToken(0x58)) {
        unsafe { write_volatile(0x0095_047c as *mut u8, 0) };
        return enter_state(ModalState(0x29));
    }
    if mode_e8() && row(FrameClass(5), RowToken(0x5a)) {
        unsafe { write_volatile(0x0095_047c as *mut u8, 1) };
        return enter_state(ModalState(0x29));
    }
    if mode_e8() && row(FrameClass(5), RowToken(0x5b)) { return enter_state(ModalState(0x2a)); }
    if row(FrameClass(5), RowToken(0x29)) { unsafe { rb_lifecycle_select_29(BinaryChoice::On) }; return 2; }
    if row(FrameClass(5), RowToken(0x30)) { unsafe { rb_lifecycle_select_30(BinaryChoice::On) }; return 2; }
    if row(FrameClass(5), RowToken(0x51)) && unsafe { rb_lifecycle_current(PropertyId(0x20d), PropertyIndex(0)) }.0 == 1 {
        unsafe { rb_lifecycle_prepare_2a(OperationId(0x2a)) };
        return transition(StageKind::Direct, StageId::Accept).word();
    }
    let result = enter();
    if result.cancelled() { notify(Notification(0x92b)); }
    result.word()
}
