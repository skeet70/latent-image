//! Read-only classification of the recipe bank's menu pages and row tokens,
//! and the owned modal-75 editor-cancel action.

pub const VERBS_PER_SLOT: u32 = 4;
pub const BANK_SLOTS: u32 = 7;
pub const SELECT_ROWS: u32 = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MenuClass(u32);

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RowToken(u32);

impl RowToken {
    const VERB_BASE: Self = Self(0x40);
    const SELECT_BASE: Self = Self(0x60);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot(u8);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selection(u8);

pub const CLASS_SLOT: MenuClass = MenuClass(0x281);
pub const CLASS_SUB0: MenuClass = MenuClass(0x270);
pub const CLASS_SELECT: MenuClass = MenuClass(0x285);
pub const CLASS_EDITOR: MenuClass = MenuClass(0x131);
pub const CLASS_EDIT: MenuClass = MenuClass(0x277);
pub const CLASS_DIALOG: MenuClass = MenuClass(0x27d);
pub const CLASS_FIELD0: MenuClass = MenuClass(0x267);
pub const CLASS_FIELD_LAST: MenuClass = MenuClass(0x26f);

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BankPage {
    None = 0,
    Slot = 1,
    Sub = 2,
    Select = 3,
    Editor = 4,
    Edit = 5,
    Dialog = 6,
    Field = 7,
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BankVerb {
    SaveCurrent = 0,
    EditName = 1,
    Edit = 2,
    Reset = 3,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BankItem {
    Verb { slot: Slot, verb: BankVerb },
    Select { selection: Selection },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PageRoute {
    pub page: BankPage,
    pub slot: Option<Slot>,
}

impl Slot {
    pub fn index(self) -> u8 {
        self.0
    }
}

impl Selection {
    pub fn code(self) -> u8 {
        self.0
    }
}

pub fn route_page(cls: MenuClass) -> PageRoute {
    if cls == CLASS_SLOT {
        return PageRoute { page: BankPage::Slot, slot: None };
    }
    if cls.0 >= CLASS_SUB0.0 && cls.0 < CLASS_SUB0.0 + BANK_SLOTS {
        return PageRoute {
            page: BankPage::Sub,
            slot: Some(Slot((cls.0 - CLASS_SUB0.0) as u8)),
        };
    }
    let page = if cls == CLASS_SELECT {
        BankPage::Select
    } else if cls == CLASS_EDITOR {
        BankPage::Editor
    } else if cls == CLASS_EDIT {
        BankPage::Edit
    } else if cls == CLASS_DIALOG {
        BankPage::Dialog
    } else if cls.0 >= CLASS_FIELD0.0 && cls.0 <= CLASS_FIELD_LAST.0 {
        BankPage::Field
    } else {
        BankPage::None
    };
    PageRoute { page, slot: None }
}

pub fn decode_token(token: RowToken) -> Option<BankItem> {
    if token.0 >= RowToken::VERB_BASE.0
        && token.0 < RowToken::VERB_BASE.0 + BANK_SLOTS * VERBS_PER_SLOT
    {
        let offset = token.0 - RowToken::VERB_BASE.0;
        return Some(BankItem::Verb {
            slot: Slot((offset / VERBS_PER_SLOT) as u8),
            verb: match offset % VERBS_PER_SLOT {
                0 => BankVerb::SaveCurrent,
                1 => BankVerb::EditName,
                2 => BankVerb::Edit,
                _ => BankVerb::Reset,
            },
        });
    }
    if token.0 >= RowToken::SELECT_BASE.0 && token.0 < RowToken::SELECT_BASE.0 + SELECT_ROWS {
        return Some(BankItem::Select {
            selection: Selection((token.0 - RowToken::SELECT_BASE.0) as u8),
        });
    }
    None
}

#[no_mangle]
pub extern "C" fn rb_ui_route_page(cls: u32) -> u32 {
    route_page(MenuClass(cls)).page as u32
}

#[no_mangle]
pub extern "C" fn rb_ui_route_slot(cls: u32) -> u32 {
    route_page(MenuClass(cls)).slot.map_or(0, |slot| slot.index() as u32)
}

/// C ABI: `kind | slot << 8 | verb << 16 | selection << 24`.
#[no_mangle]
pub extern "C" fn rb_ui_route_item(token: u32) -> u32 {
    match decode_token(RowToken(token)) {
        None => 0,
        Some(BankItem::Verb { slot, verb }) => {
            1 | ((slot.index() as u32) << 8) | ((verb as u32) << 16)
        }
        Some(BankItem::Select { selection }) => 2 | ((selection.code() as u32) << 24),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(cls: u32) -> (u32, u32) {
        (rb_ui_route_page(cls), rb_ui_route_slot(cls))
    }

    #[test]
    fn exported_page_and_slot_boundaries() {
        assert_eq!(page(0x281), (1, 0));
        assert_eq!(page(0x285), (3, 0));
        assert_eq!(page(0x131), (4, 0));
        assert_eq!(page(0x277), (5, 0));
        assert_eq!(page(0x27d), (6, 0));
        assert_eq!(page(0x26f), (7, 0));
        assert_eq!(page(0x270), (2, 0));
        assert_eq!(page(0x276), (2, 6));
        assert_eq!(page(0x278), (0, 0));
        assert_eq!(page(0x266), (0, 0));
        assert_eq!(page(0), (0, 0));
    }

    #[test]
    fn exported_verb_tokens() {
        assert_eq!(rb_ui_route_item(0x40), 1);
        assert_eq!(rb_ui_route_item(0x41), 0x0001_0001);
        assert_eq!(rb_ui_route_item(0x42), 0x0002_0001);
        assert_eq!(rb_ui_route_item(0x43), 0x0003_0001);
        assert_eq!(rb_ui_route_item(0x44), 0x0000_0101);
        assert_eq!(rb_ui_route_item(0x58), 0x0000_0601);
        assert_eq!(rb_ui_route_item(0x5b), 0x0003_0601);
    }

    #[test]
    fn exported_select_tokens() {
        assert_eq!(rb_ui_route_item(0x60), 2);
        assert_eq!(rb_ui_route_item(0x61), 0x0100_0002);
        assert_eq!(rb_ui_route_item(0x67), 0x0700_0002);
    }

    #[test]
    fn exported_unknown_tokens_are_no_item() {
        for token in [
            0u32, 0x3f, 0x5c, 0x5f, 0x68, 0x70, 0x76, 0x7d, 0x7e, 0x80, 0x82, 0xffff_ffff,
        ] {
            assert_eq!(rb_ui_route_item(token), 0, "{token:#x}");
        }
    }
}

const MODAL_QUERY_EDITOR: ModalQuery = ModalQuery(0x53);
const EDITOR_SESSION_CELL: usize = 0x0095_4d08;
const MODAL_STATE_CELL: usize = 0x0146_761e;
const EDITOR_CANCEL_STATE: u16 = 0x1d;
const EDITOR_CANCEL_STAGE: StageOpen = StageOpen { kind: 0x0f, id: 0x1a };
const SUPPRESSED: TransitionResult = TransitionResult(0);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct ModalQuery(u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct QueryAnswer(u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct EditorSession(u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct TransitionResult(u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct StageOpen {
    kind: u32,
    id: u32,
}

impl QueryAnswer {
    fn is_active(self) -> bool {
        self.0 != 0
    }
}

impl EditorSession {
    fn is_open(self) -> bool {
        self.0 == 1
    }
}

impl TransitionResult {
    fn ends_editor(self) -> bool {
        matches!(self.0, 0xffff_ffff | 2)
    }
}

pub(crate) trait CancelServices {
    fn answer(&mut self, query: ModalQuery) -> QueryAnswer;
    fn editor_session(&mut self) -> EditorSession;
    fn open_stage(&mut self, stage: StageOpen);
    fn transition(&mut self) -> TransitionResult;
    fn store_state(&mut self, state: u16);
}

// FUN_00c77794: query 0x53 nonzero admits only while 0x00954d08 is 1; results
// -1 and 2 write state 0x1d after stage 0xf/0x1a.
pub(crate) fn run_editor_cancel(services: &mut impl CancelServices) -> TransitionResult {
    if services.answer(MODAL_QUERY_EDITOR).is_active() && !services.editor_session().is_open() {
        return SUPPRESSED;
    }
    services.open_stage(EDITOR_CANCEL_STAGE);
    let result = services.transition();
    if result.ends_editor() {
        services.store_state(EDITOR_CANCEL_STATE);
    }
    result
}

unsafe extern "C" {
    fn rb_call_modal_query(query: u32) -> u32;
    fn rb_call_stage_open(kind: u32, id: u32);
    fn rb_call_transition() -> u32;
}

struct RetainedServices;

impl CancelServices for RetainedServices {
    fn answer(&mut self, query: ModalQuery) -> QueryAnswer {
        QueryAnswer(unsafe { rb_call_modal_query(query.0) })
    }

    fn editor_session(&mut self) -> EditorSession {
        EditorSession(unsafe { core::ptr::read_volatile(EDITOR_SESSION_CELL as *const u32) })
    }

    fn open_stage(&mut self, stage: StageOpen) {
        unsafe { rb_call_stage_open(stage.kind, stage.id) }
    }

    fn transition(&mut self) -> TransitionResult {
        TransitionResult(unsafe { rb_call_transition() })
    }

    fn store_state(&mut self, state: u16) {
        unsafe { core::ptr::write_volatile(MODAL_STATE_CELL as *mut u16, state) }
    }
}

#[no_mangle]
pub unsafe extern "C" fn rb_ui_editor_cancel() -> u32 {
    run_editor_cancel(&mut RetainedServices).0
}

#[cfg(test)]
mod cancel_tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    enum Effect {
        Query(u32),
        EditorSession,
        StageOpen { kind: u32, id: u32 },
        Transition,
        StoreState(u16),
    }

    struct Fake {
        answer: u32,
        session: u32,
        transition: u32,
        effects: Vec<Effect>,
    }

    impl Fake {
        fn new(answer: u32, session: u32, transition: u32) -> Self {
            Self { answer, session, transition, effects: Vec::new() }
        }
    }

    impl CancelServices for Fake {
        fn answer(&mut self, query: ModalQuery) -> QueryAnswer {
            self.effects.push(Effect::Query(query.0));
            QueryAnswer(self.answer)
        }

        fn editor_session(&mut self) -> EditorSession {
            self.effects.push(Effect::EditorSession);
            EditorSession(self.session)
        }

        fn open_stage(&mut self, stage: StageOpen) {
            self.effects.push(Effect::StageOpen { kind: stage.kind, id: stage.id });
        }

        fn transition(&mut self) -> TransitionResult {
            self.effects.push(Effect::Transition);
            TransitionResult(self.transition)
        }

        fn store_state(&mut self, state: u16) {
            self.effects.push(Effect::StoreState(state));
        }
    }

    #[test]
    fn active_query_without_session_suppresses_before_stage_and_transition() {
        let mut fake = Fake::new(1, 0, 0xffff_ffff);
        assert_eq!(run_editor_cancel(&mut fake).0, 0);
        assert_eq!(fake.effects, vec![Effect::Query(0x53), Effect::EditorSession]);
    }

    #[test]
    fn open_editor_runs_cancel_and_marks_state_for_minus_one() {
        let mut fake = Fake::new(1, 1, 0xffff_ffff);
        assert_eq!(run_editor_cancel(&mut fake).0, 0xffff_ffff);
        assert_eq!(
            fake.effects,
            vec![
                Effect::Query(0x53),
                Effect::EditorSession,
                Effect::StageOpen { kind: 0x0f, id: 0x1a },
                Effect::Transition,
                Effect::StoreState(0x1d),
            ]
        );
    }

    #[test]
    fn quiet_query_skips_session_and_marks_state_for_two() {
        let mut fake = Fake::new(0, 0, 2);
        assert_eq!(run_editor_cancel(&mut fake).0, 2);
        assert_eq!(
            fake.effects,
            vec![
                Effect::Query(0x53),
                Effect::StageOpen { kind: 0x0f, id: 0x1a },
                Effect::Transition,
                Effect::StoreState(0x1d),
            ]
        );
    }

    #[test]
    fn other_transition_word_is_returned_without_state_write() {
        let mut fake = Fake::new(0, 0, 0x5d);
        assert_eq!(run_editor_cancel(&mut fake).0, 0x5d);
        assert_eq!(fake.effects.last(), Some(&Effect::Transition));
    }

    #[test]
    fn session_word_must_equal_one() {
        let mut fake = Fake::new(2, 2, 0);
        assert_eq!(run_editor_cancel(&mut fake).0, 0);
        assert_eq!(fake.effects, vec![Effect::Query(0x53), Effect::EditorSession]);
    }

    #[test]
    fn any_nonzero_query_answer_is_active() {
        let mut fake = Fake::new(2, 0, 0x5d);
        assert_eq!(run_editor_cancel(&mut fake).0, 0);
        assert_eq!(fake.effects, vec![Effect::Query(0x53), Effect::EditorSession]);
    }

    #[no_mangle]
    extern "C" fn rb_call_modal_query(_: u32) -> u32 {
        0
    }

    #[no_mangle]
    extern "C" fn rb_call_stage_open(_: u32, _: u32) {}

    #[no_mangle]
    extern "C" fn rb_call_transition() -> u32 {
        0
    }
}
