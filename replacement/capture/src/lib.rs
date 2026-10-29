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
    fn event(&self) -> EventId {
        EventId(self.words[0])
    }

    fn entry_clear(&self) -> EventId {
        EventId(self.words[1])
    }

    fn cancel_clear(&self) -> EventId {
        EventId(self.words[2])
    }

    fn normal_state(&self) -> StepState {
        StepState(self.words[3])
    }

    fn cancel_state(&self) -> StepState {
        StepState(self.words[4])
    }

    fn wait_event(&self) -> EventId {
        EventId(self.words[8])
    }

    fn cancel_selector(&self) -> Option<CancelSelector> {
        CancelSelector::from_word(self.words[9])
    }
}

struct StepSnapshot {
    event: EventId,
    entry_clear: EventId,
    cancel_clear: EventId,
    normal_state: StepState,
    cancel_state: StepState,
    wait_event: EventId,
    cancel_selector: CancelSelector,
}

impl StepSnapshot {
    fn from_plan(plan: &StepPlan) -> Option<Self> {
        Some(StepSnapshot {
            event: plan.event(),
            entry_clear: plan.entry_clear(),
            cancel_clear: plan.cancel_clear(),
            normal_state: plan.normal_state(),
            cancel_state: plan.cancel_state(),
            wait_event: plan.wait_event(),
            cancel_selector: plan.cancel_selector()?,
        })
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

struct StockExecutor {
    context: *mut u8,
}

impl StockExecutor {
    fn check(&mut self, event: EventId) -> StepStatus {
        StepStatus::from_word(unsafe { rb_capture_stock_check(self.context, event.0) })
    }

    fn clear(&mut self, event: EventId) {
        unsafe { rb_capture_stock_clear(self.context, event.0) }
    }

    fn advance(&mut self, state: StepState) {
        unsafe { rb_capture_stock_advance(self.context, state.0) }
    }

    fn requested_events(&mut self) -> RequestedEvents {
        RequestedEvents(unsafe { rb_capture_stock_cancel_signals(self.context) })
    }

    fn wait(&mut self, event: EventId) {
        unsafe { rb_capture_stock_wait(event.0) }
    }

    fn release(&mut self) {
        unsafe { rb_capture_stock_release() }
    }

    fn log(&mut self, phase: LogPhase) {
        unsafe { rb_capture_stock_log(self.context, phase as u32) }
    }
}

fn run_step(snapshot: StepSnapshot, executor: &mut StockExecutor) -> StepStatus {
    let cancel = CancelMask::from_selector(snapshot.cancel_selector);

    let status = executor.check(snapshot.event);
    if !status.succeeded() {
        return status;
    }

    executor.clear(snapshot.entry_clear);

    if !cancel.requests_cancel(executor.requested_events()) {
        executor.advance(snapshot.normal_state);
        let status = executor.check(snapshot.event);
        if !status.succeeded() {
            executor.release();
            return status;
        }

        executor.log(LogPhase::Waiting);
        executor.wait(snapshot.wait_event);
        executor.log(LogPhase::CancelCheck);

        if !cancel.requests_cancel(executor.requested_events()) {
            let status = executor.check(snapshot.event);
            executor.release();
            return status;
        }
    }

    executor.advance(snapshot.cancel_state);
    executor.clear(snapshot.cancel_clear);
    let status = executor.check(snapshot.event);
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
}

/// The caller owns the descriptor lookup.
///
/// # Safety
/// `context` is non-null, aligned, and points to the stock develop context.
/// `plan` is non-null, aligned, points to a stock step descriptor, and its
/// fields are not modified during the call. Descriptor `[9]` is `0xc` or in
/// `0..=31`.
#[no_mangle]
pub unsafe extern "C" fn rb_capture_step_wait(context: *mut u8, plan: *const StepPlan) -> i32 {
    let plan = unsafe { &*plan };
    let snapshot = match StepSnapshot::from_plan(plan) {
        Some(snapshot) => snapshot,
        None => unsafe { core::hint::unreachable_unchecked() },
    };
    let mut executor = StockExecutor { context };
    run_step(snapshot, &mut executor).word()
}
