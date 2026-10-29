//! X-M1 1.50 `FUN_00e9e038`: develop step completion, cancellation and
//! scheduler release. Descriptor `[0]`/`[8]` are the checked and waited events,
//! `[1]`/`[2]` the entry/cancel clears, `[3]`/`[4]` the normal/cancel states,
//! and `[9]` the cancel bit (`0xc` selects every requested event).

pub const STEP_PLAN_WORDS: usize = 13;

pub const ALL_CANCEL_BIT: u32 = 0xc;

/// The exported entry's null-descriptor status; stock has no such return.
pub const STATUS_INVALID_PLAN: i32 = -1;

const WAIT_LOG: u32 = 5;
const CANCEL_LOG: u32 = 6;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct StepPlan {
    words: [u32; STEP_PLAN_WORDS],
}

impl StepPlan {
    fn word(&self, index: usize) -> u32 {
        self.words[index]
    }

    fn event(&self) -> u32 {
        self.word(0)
    }

    fn entry_clear(&self) -> u32 {
        self.word(1)
    }

    fn cancel_clear(&self) -> u32 {
        self.word(2)
    }

    fn normal_state(&self) -> u32 {
        self.word(3)
    }

    fn cancel_state(&self) -> u32 {
        self.word(4)
    }

    fn wait_event(&self) -> u32 {
        self.word(8)
    }

    fn cancel_bit(&self) -> u32 {
        self.word(9)
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
struct CancelMask(u32);

impl CancelMask {
    fn from_bit(bit: u32) -> Self {
        if bit == ALL_CANCEL_BIT {
            CancelMask(u32::MAX)
        } else {
            CancelMask(1u32 << (bit & 31))
        }
    }

    fn requests_cancel(self, signals: u32) -> bool {
        self.0 & signals != 0
    }
}

/// The production adapter and the qualification consumer bind these names.
pub trait StepExecutor {
    fn check(&mut self, event: u32) -> StepStatus;
    fn clear(&mut self, event: u32);
    fn advance(&mut self, state: u32);
    fn cancel_signals(&mut self) -> u32;
    fn wait(&mut self, event: u32);
    fn release(&mut self);
    fn log(&mut self, phase: u32);
}

pub fn run_step<E: StepExecutor>(plan: &StepPlan, executor: &mut E) -> StepStatus {
    let cancel = CancelMask::from_bit(plan.cancel_bit());

    let status = executor.check(plan.event());
    if !status.succeeded() {
        return status;
    }

    executor.clear(plan.entry_clear());

    if !cancel.requests_cancel(executor.cancel_signals()) {
        executor.advance(plan.normal_state());
        let status = executor.check(plan.event());
        if !status.succeeded() {
            executor.release();
            return status;
        }

        executor.log(WAIT_LOG);
        executor.wait(plan.wait_event());
        executor.log(CANCEL_LOG);

        if !cancel.requests_cancel(executor.cancel_signals()) {
            let status = executor.check(plan.event());
            executor.release();
            return status;
        }
    }

    executor.advance(plan.cancel_state());
    executor.clear(plan.cancel_clear());
    let status = executor.check(plan.event());
    executor.release();
    status
}

struct StockExecutor {
    context: *mut u8,
}

impl StepExecutor for StockExecutor {
    fn check(&mut self, event: u32) -> StepStatus {
        StepStatus::from_word(unsafe { rb_capture_stock_check(self.context, event) })
    }

    fn clear(&mut self, event: u32) {
        unsafe { rb_capture_stock_clear(self.context, event) }
    }

    fn advance(&mut self, state: u32) {
        unsafe { rb_capture_stock_advance(self.context, state) }
    }

    fn cancel_signals(&mut self) -> u32 {
        unsafe { rb_capture_stock_cancel_signals(self.context) }
    }

    fn wait(&mut self, event: u32) {
        unsafe { rb_capture_stock_wait(event) }
    }

    fn release(&mut self) {
        unsafe { rb_capture_stock_release() }
    }

    fn log(&mut self, phase: u32) {
        unsafe { rb_capture_stock_log(self.context, phase) }
    }
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
#[no_mangle]
pub unsafe extern "C" fn rb_capture_step_wait(context: *mut u8, plan: *const StepPlan) -> i32 {
    let Some(plan) = unsafe { plan.as_ref() } else {
        return STATUS_INVALID_PLAN;
    };
    let mut executor = StockExecutor { context };
    run_step(plan, &mut executor).word()
}
