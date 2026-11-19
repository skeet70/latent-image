//! Caller-owned load timeouts in milliseconds and their conversion to the
//! loader's absolute `Deadline`.
//!
//! Timer contract (reports/xm1-timer-interrupt-laws.md §1, E-LOG-TICK-SOURCE-150):
//! the loader clock counts `0xfff91000` up through a `RELOAD` = 30000-count
//! reload period per kernel tick; one tick is nominally 10 ms (hardware 99.88 Hz,
//! so a real tick is 10.012 ms). One millisecond is therefore `RELOAD / 10` =
//! 3000 counts; at the hardware rate the converted bound elapses 0.12% sooner
//! than the stated time. No timeout is derived from instruction counts, emulator
//! scheduling or the `0x100000` DMA byte budget.

use core::num::NonZeroU32;

use crate::nand::dma_read::{Deadline, Instant};

#[allow(dead_code)]
#[path = "../../embassy_platform/src/clock.rs"]
mod timer_contract;

const NOMINAL_TICK_MILLISECONDS: u32 = 10;

/// Loader clock counts per millisecond under the timer contract.
pub const COUNTS_PER_MILLISECOND: u64 = (timer_contract::RELOAD / NOMINAL_TICK_MILLISECONDS) as u64;

/// A finite, nonzero caller timeout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Milliseconds(NonZeroU32);

impl Milliseconds {
    pub const fn new(milliseconds: u32) -> Option<Self> {
        match NonZeroU32::new(milliseconds) { Some(value) => Some(Self(value)), None => None }
    }
    pub const fn get(self) -> u32 { self.0.get() }
    pub const fn counts(self) -> u64 { self.0.get() as u64 * COUNTS_PER_MILLISECOND }
}

/// The absolute deadline `timeout` after `now`; `None` if it is not finite.
pub fn deadline_after(now: Instant, timeout: Milliseconds) -> Option<Deadline> {
    Deadline::new(Instant::new(now.count().checked_add(timeout.counts())?))
}

/// Caller policy, never applied by the loader itself: the hang-detection bound
/// for loads issued during Root startup. Generous by design (30 s) rather than
/// tuned to nominal load time; expiry yields the storage/DMA quarantine
/// outcome, never a retry or publication of an incomplete image. A camera
/// measurement must confirm substantial margin before physical acceptance; if
/// it does not, change this caller policy, not the loader.
pub const STARTUP_CLASS_HANG_BOUND: Milliseconds = match Milliseconds::new(30_000) {
    Some(timeout) => timeout,
    None => panic!("startup hang bound is nonzero"),
};
