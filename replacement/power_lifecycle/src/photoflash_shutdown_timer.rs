use super::{Address, Runtime, TimerHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SavedInterrupts(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TimerIndex(u32);

pub trait TimerRuntime: Runtime {
    fn release_irq_handle(&mut self, handle: TimerHandle) -> i32;
    fn lock_interrupts(&mut self) -> SavedInterrupts;
    fn unlock_interrupts(&mut self, saved: SavedInterrupts);
    fn timer_diagnostic(&mut self, category: u32, code: u32);
}
const STATE: u32 = 0x0095_5a00;
const HANDLES: u32 = 0x0095_5a60;
fn index(runtime: &impl TimerRuntime, handle: TimerHandle) -> Option<TimerIndex> {
    if handle.0 == 0 { return None; }
    (0..12).find(|index| runtime.read32(Address(HANDLES + index * 4)) == handle.0).map(TimerIndex)
}

pub fn stop(runtime: &mut impl TimerRuntime, handle: TimerHandle) -> i32 {
    let Some(index) = index(runtime, handle) else { return -20; };
    if runtime.read32(Address(STATE + index.0 * 4)) == 1 { return -11; }
    runtime.write32(Address(0xfffa_9000), 0x1_0000 << index.0);
    let irq_slot = Address(STATE + (index.0 + 36) * 4);
    let irq_handle = TimerHandle(runtime.read32(irq_slot));
    let status = if irq_handle.0 != 0 {
        let result = runtime.release_irq_handle(irq_handle);
        runtime.write32(irq_slot, 0);
        result
    } else { 0 };
    let saved = runtime.lock_interrupts();
    let mask = runtime.read32(Address(STATE + 0xc0));
    if mask != 0 { runtime.write32(Address(STATE + 0xc0), mask & !(1 << index.0)); }
    runtime.unlock_interrupts(saved);
    runtime.write32(Address(STATE + index.0 * 4), 1);
    status
}

pub fn delete(runtime: &mut impl TimerRuntime, handle: TimerHandle) -> i32 {
    let Some(index) = index(runtime, handle) else {
        runtime.timer_diagnostic(2, 18);
        return -20;
    };
    runtime.write32(Address(STATE + (index.0 + 12) * 4), u32::MAX);
    runtime.write32(Address(STATE + index.0 * 4), 0);
    runtime.write32(Address(HANDLES + index.0 * 4), 0);
    0
}
