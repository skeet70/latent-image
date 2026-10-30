//! Owned replacement for `FUN_00bfb028`. `rb_modal_dispatch(r0 = id)` returns
//! the action result, or zero for a null cell base or null action entry.
//!
//! Retained stock services: prehook `0x00d0cc68`; pointer tables `0x00cf58b8`,
//! `0x00cfe2a8`, `0x00d0013c`; event/width/state cells `0x014676e4`,
//! `0x00cd5e78`, `0x01467588`; prehook shadow `0x0095037e`.

#[cfg(target_arch = "arm")]
core::arch::global_asm!(include_str!("dispatch.S"));

const CELL_POINTERS: u32 = 0x00cf_58b8;
const ACTION_POINTERS: u32 = 0x00cf_e2a8;
const TRANSITION_POINTERS: u32 = 0x00d0_013c;
const EVENT_CELLS: u32 = 0x0146_76e4;
const WIDTH_CELLS: u32 = 0x00cd_5e78;
const STATE_CELLS: u32 = 0x0146_7588;

/// The full word indexes the pointer tables; both table and halfword offsets
/// wrap at 32 bits.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct ControllerId(u32);

impl ControllerId {
    const fn new(word: u32) -> Self {
        Self(word)
    }

    const fn word(self) -> u32 {
        self.0
    }

    #[inline(always)]
    fn pointer_offset(self) -> u32 {
        self.0.wrapping_mul(4)
    }

    #[inline(always)]
    fn halfword_offset(self) -> u32 {
        self.0.wrapping_mul(2)
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Event(u16);

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Width(u16);

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct State(u16);

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct CellIndex(u16);

impl CellIndex {
    #[inline(always)]
    fn compute(event: Event, width: Width, state: State) -> Self {
        let index = u32::from(event.0)
            .wrapping_mul(u32::from(width.0))
            .wrapping_add(u32::from(state.0));
        Self(index as u16)
    }

    #[inline(always)]
    fn byte_offset(self) -> u32 {
        u32::from(self.0).wrapping_mul(4)
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct ActionIndex(u16);

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct CellHigh(i16);

impl CellHigh {
    #[inline(always)]
    fn from_word(word: u16) -> Self {
        Self(word as i16)
    }

    #[inline(always)]
    fn positive_index(self) -> Option<u16> {
        if self.0 > 0 { Some(self.0 as u16) } else { None }
    }

    #[inline(always)]
    fn state_value(self) -> StateValue {
        StateValue(self.0.wrapping_neg() as u16)
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct StateValue(u16);

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct ActionTarget(u32);

impl ActionTarget {
    #[inline(always)]
    fn from_word(word: u32) -> Option<Self> {
        if word == 0 { None } else { Some(Self(word)) }
    }

    #[inline(always)]
    fn word(self) -> u32 {
        self.0
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct TransitionTarget(u32);

impl TransitionTarget {
    #[inline(always)]
    fn from_word(word: u32) -> Option<Self> {
        if word == 0 { None } else { Some(Self(word)) }
    }

    #[inline(always)]
    fn word(self) -> u32 {
        self.0
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct CellTable(u32);

impl CellTable {
    #[inline(always)]
    fn present(self) -> Option<CellAddress> {
        if self.0 == 0 { None } else { Some(CellAddress(self.0)) }
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct ActionTable(u32);

impl ActionTable {
    #[inline(always)]
    fn entry(self, index: ActionIndex) -> Option<ActionTarget> {
        let address = self.0.wrapping_add(u32::from(index.0).wrapping_mul(4));
        ActionTarget::from_word(unsafe { read_word(address) })
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct TransitionTable(u32);

impl TransitionTable {
    #[inline(always)]
    fn base(self) -> u32 {
        self.0
    }

    #[inline(always)]
    fn entry(self, index: u16) -> Option<TransitionTarget> {
        let address = self.0.wrapping_add(u32::from(index).wrapping_mul(4));
        TransitionTarget::from_word(unsafe { read_word(address) })
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct CellAddress(u32);

impl CellAddress {
    #[inline(always)]
    fn action_index(self) -> ActionIndex {
        ActionIndex(unsafe { read_half(self.0) })
    }

    #[inline(always)]
    fn high(self) -> CellHigh {
        CellHigh::from_word(unsafe { read_half(self.0.wrapping_add(2)) })
    }

    #[inline(always)]
    fn offset(address: u32, index: CellIndex) -> u32 {
        address.wrapping_add(index.byte_offset())
    }
}

#[derive(Clone, Copy)]
struct Captured {
    cell: CellTable,
    action: ActionTable,
    transition: TransitionTable,
    event: Event,
    width: Width,
    state: State,
}

impl Captured {
    #[inline(always)]
    unsafe fn capture(controller: ControllerId) -> Self {
        let cell = CellTable(unsafe { read_word(CELL_POINTERS.wrapping_add(controller.pointer_offset())) });
        let action = ActionTable(unsafe { read_word(ACTION_POINTERS.wrapping_add(controller.pointer_offset())) });
        let transition = TransitionTable(unsafe { read_word(TRANSITION_POINTERS.wrapping_add(controller.pointer_offset())) });
        let event = Event(unsafe { read_half(EVENT_CELLS.wrapping_add(controller.halfword_offset())) });
        let width = Width(unsafe { read_half(WIDTH_CELLS.wrapping_add(controller.halfword_offset())) });
        let state = State(unsafe { read_half(STATE_CELLS.wrapping_add(controller.halfword_offset())) });
        Self { cell, action, transition, event, width, state }
    }
}

#[inline(always)]
unsafe fn read_word(address: u32) -> u32 {
    unsafe { core::ptr::read_volatile(address as *const u32) }
}

#[inline(always)]
unsafe fn read_half(address: u32) -> u16 {
    unsafe { core::ptr::read_volatile(address as *const u16) }
}

#[inline(always)]
fn state_cell(controller: ControllerId) -> *mut u16 {
    STATE_CELLS.wrapping_add(controller.halfword_offset()) as *mut u16
}

#[repr(C)]
struct Residue {
    first: u32,
    second: u32,
}

#[inline(always)]
fn residue(packed: u64) -> Residue {
    Residue { first: packed as u32, second: (packed >> 32) as u32 }
}

unsafe extern "C" {
    fn rb_modal_call_prehook(controller: u32, state: u32, event: u32, doubled: u32) -> u64;
    fn rb_modal_call(target: u32, first: u32, second: u32, third: u32) -> u64;
}

/// # Safety
///
/// Every action and transition entry the captured tables can select must be
/// an admitted target of the `//replacement/modal_runtime:modal-closure` proof.
#[no_mangle]
pub unsafe extern "C" fn rb_modal_dispatch(controller: u32) -> u32 {
    let controller = ControllerId::new(controller);
    let captured = unsafe { Captured::capture(controller) };
    let Some(cell_base) = captured.cell.present() else {
        return 0;
    };

    // The stock event, width and state loads at 0x00bfb06c/0x00bfb078/0x00bfb084
    // are LDRH (e19270b3/e19280b3/e19240b3).
    let prehook = residue(unsafe {
        rb_modal_call_prehook(
            controller.word(),
            u32::from(captured.state.0),
            u32::from(captured.event.0),
            controller.word().wrapping_mul(2),
        )
    });

    let index = CellIndex::compute(captured.event, captured.width, captured.state);
    let cell = CellAddress(CellAddress::offset(cell_base.0, index));
    let action = captured.action.entry(cell.action_index());

    let (carried_first, carried_second, result) = match action {
        Some(target) => {
            // The retained prehook returns with r2 still holding the event.
            let action_residue = residue(unsafe {
                rb_modal_call(
                    target.word(),
                    prehook.first,
                    prehook.second,
                    u32::from(captured.event.0),
                )
            });
            (action_residue.first, action_residue.second, action_residue.first)
        }
        None => (prehook.first, prehook.second, 0),
    };

    let high = cell.high();
    if let Some(index) = high.positive_index() {
        if let Some(target) = captured.transition.entry(index) {
            let _ = unsafe {
                rb_modal_call(
                    target.word(),
                    carried_first,
                    carried_second,
                    captured.transition.base(),
                )
            };
        }
    } else {
        unsafe { core::ptr::write_volatile(state_cell(controller), high.state_value().0) };
    }

    result
}
