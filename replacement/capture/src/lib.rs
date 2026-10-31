//! X-M1 1.50 `FUN_00e9e038`: develop step completion, cancellation and
//! scheduler release. Descriptor `[0]`/`[8]` are the checked and waited events,
//! `[1]`/`[2]` the entry/cancel clears, `[3]`/`[4]` the normal/cancel states,
//! and `[9]` the cancel bit (`0xc` selects every requested event).

pub const STEP_PLAN_WORDS: usize = 13;

pub const ALL_CANCEL_BIT: u32 = 0xc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EventId(u32);

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StepState(u32);

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RequestedEvents(u32);

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LogPhase {
    Waiting = 5,
    CancelCheck = 6,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CancelBit(u8);

#[derive(Clone, Copy, PartialEq, Eq)]
enum CancelSelector {
    All,
    Bit(CancelBit),
}

impl CancelSelector {
    fn from_word(word: u32) -> Option<Self> {
        match word {
            ALL_CANCEL_BIT => Some(CancelSelector::All),
            0..=31 => Some(CancelSelector::Bit(CancelBit(word as u8))),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
struct CancelMask(u32);

impl CancelMask {
    fn from_selector(selector: CancelSelector) -> Self {
        match selector {
            CancelSelector::All => CancelMask(u32::MAX),
            CancelSelector::Bit(bit) => CancelMask(1u32 << bit.0),
        }
    }

    fn requests_cancel(self, events: RequestedEvents) -> bool {
        self.0 & events.0 != 0
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct StepPlan {
    words: [u32; STEP_PLAN_WORDS],
}

impl StepPlan {
    unsafe fn word(plan: *const StepPlan, index: usize) -> u32 {
        let words = core::ptr::addr_of!((*plan).words) as *const u32;
        unsafe { core::ptr::read_volatile(words.add(index)) }
    }

    unsafe fn event(plan: *const StepPlan) -> EventId {
        EventId(unsafe { Self::word(plan, 0) })
    }

    unsafe fn entry_clear(plan: *const StepPlan) -> EventId {
        EventId(unsafe { Self::word(plan, 1) })
    }

    unsafe fn cancel_clear(plan: *const StepPlan) -> EventId {
        EventId(unsafe { Self::word(plan, 2) })
    }

    unsafe fn normal_state(plan: *const StepPlan) -> StepState {
        StepState(unsafe { Self::word(plan, 3) })
    }

    unsafe fn cancel_state(plan: *const StepPlan) -> StepState {
        StepState(unsafe { Self::word(plan, 4) })
    }

    unsafe fn wait_event(plan: *const StepPlan) -> EventId {
        EventId(unsafe { Self::word(plan, 8) })
    }

    unsafe fn cancel_selector(plan: *const StepPlan) -> Option<CancelSelector> {
        CancelSelector::from_word(unsafe { Self::word(plan, 9) })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StepStatus(i32);

impl StepStatus {
    fn from_word(word: i32) -> Self {
        StepStatus(word)
    }

    pub fn word(self) -> i32 {
        self.0
    }

    fn succeeded(self) -> bool {
        self.0 == 0
    }
}

#[derive(Clone, Copy)]
struct ContextHandle(*mut u8);

struct StockExecutor {
    context: ContextHandle,
}

impl StockExecutor {
    fn check(&mut self, event: EventId) -> StepStatus {
        StepStatus::from_word(unsafe { rb_capture_stock_check(self.context.0, event.0) })
    }

    fn clear(&mut self, event: EventId) {
        unsafe { rb_capture_stock_clear(self.context.0, event.0) }
    }

    fn advance(&mut self, state: StepState) {
        unsafe { rb_capture_stock_advance(self.context.0, state.0) }
    }

    fn requested_events(&mut self) -> RequestedEvents {
        RequestedEvents(unsafe { rb_capture_stock_cancel_signals(self.context.0) })
    }

    fn wait(&mut self, event: EventId) {
        unsafe { rb_capture_stock_wait(event.0) }
    }

    fn release(&mut self) {
        unsafe { rb_capture_stock_release() }
    }

    fn log(&mut self, phase: LogPhase) {
        unsafe { rb_capture_stock_log(self.context.0, phase as u32) }
    }
}

unsafe fn run_step(plan: *const StepPlan, executor: &mut StockExecutor) -> StepStatus {
    let cancel = match unsafe { StepPlan::cancel_selector(plan) } {
        Some(selector) => CancelMask::from_selector(selector),
        None => unsafe { rb_capture_invalid_descriptor() },
    };

    let status = executor.check(unsafe { StepPlan::event(plan) });
    if !status.succeeded() {
        return status;
    }

    executor.clear(unsafe { StepPlan::entry_clear(plan) });

    if !cancel.requests_cancel(executor.requested_events()) {
        executor.advance(unsafe { StepPlan::normal_state(plan) });
        let status = executor.check(unsafe { StepPlan::event(plan) });
        if !status.succeeded() {
            executor.release();
            return status;
        }

        executor.log(LogPhase::Waiting);
        executor.wait(unsafe { StepPlan::wait_event(plan) });
        executor.log(LogPhase::CancelCheck);

        if !cancel.requests_cancel(executor.requested_events()) {
            let status = executor.check(unsafe { StepPlan::event(plan) });
            executor.release();
            return status;
        }
    }

    executor.advance(unsafe { StepPlan::cancel_state(plan) });
    executor.clear(unsafe { StepPlan::cancel_clear(plan) });
    let status = executor.check(unsafe { StepPlan::event(plan) });
    executor.release();
    status
}

unsafe extern "C" {
    fn rb_capture_stock_check(context: *mut u8, event: u32) -> i32;
    fn rb_capture_stock_clear(context: *mut u8, event: u32);
    fn rb_capture_stock_advance(context: *mut u8, state: u32);
    fn rb_capture_stock_cancel_signals(context: *const u8) -> u32;
    fn rb_capture_stock_wait(event: u32);
    fn rb_capture_stock_release();
    fn rb_capture_stock_log(context: *mut u8, phase: u32);
    fn rb_capture_invalid_descriptor() -> !;
}

/// The caller owns the descriptor lookup.
///
/// # Safety
/// `context` is non-null, aligned, and points to the stock develop context.
/// `plan` is non-null, aligned, and points to a stock step descriptor. The
/// descriptor fields are read with volatile reads at the stock use points;
/// descriptor `[9]` is `0xc` or in `0..=31`.
#[no_mangle]
pub unsafe extern "C" fn rb_capture_step_wait(context: *mut u8, plan: *const StepPlan) -> i32 {
    let mut executor = StockExecutor {
        context: ContextHandle(context),
    };
    unsafe { run_step(plan, &mut executor) }.word()
}
