//! Calls into the stock kernel through the published roots (manifest v5) and the device words the
//! time driver reads (reports/xm1-timer-interrupt-laws.md §5-§6).

use core::arch::asm;
use core::mem::transmute;
use core::ptr::read_volatile;

use crate::ids::{Domain, EncodedId, SlotStatus, TaskId};

const ROOT_CELLS: usize = 0x0096_bd20;
const TICK_CELL: usize = 0x400c_0800;
const TIMER_COUNTER: usize = 0xfff9_1000;
/// Stock loader routine `(address, bytes)` that cleans and invalidates D-cache lines on the calling core.
const CLEAN_DCACHE: usize = 0x0004_2634;
/// Image-4 critical pair that task creation 0x725dd8 holds around `+0x80`/`+0x0`. Enter masks IRQ
/// through `+0x164`, reads the cross-core lock through `+0x1624` and, unless this core already
/// holds it, retries `+0x161c` until it takes it; it returns the prior mask.
const CRITICAL_ENTER: usize = 0x0072_35d8;
const CRITICAL_LEAVE: usize = 0x0072_36b0;
/// Image-4 dynamic task allocator `(0, activate, packet)`, called the same way by its four plain-image
/// callers. It fills the stack with byte 0 and cleans it, then tries root A `+0x0` with ids from
/// cursor 0x40a0fc34 down to 105, moving to the next id when one fails, and stores the cursor
/// below the id it took. With `activate` 1 the `+0x0` flags are 2 and it activates the task
/// through the executing core's `+0xc`.
const ALLOCATE_TASK: usize = 0x0072_8804;

mod offset {
    pub const CREATE: usize = 0x0;
    pub const ACTIVATE: usize = 0xc;
    pub const SLEEP: usize = 0x28;
    pub const WAKEUP: usize = 0x2c;
    pub const WAKEUP_FROM_HANDLER: usize = 0x30;
    pub const REGISTER_AUXILIARY: usize = 0x80;
    pub const CPU_LOCK: usize = 0x130;
    pub const CPU_LOCK_FROM_HANDLER: usize = 0x134;
    pub const CPU_UNLOCK: usize = 0x138;
    pub const CPU_UNLOCK_FROM_HANDLER: usize = 0x13c;
    pub const HANDLER_CONTEXT: usize = 0x148;
    pub const INTERRUPT_ENABLE: usize = 0x158;
    pub const INTERRUPT_DISABLE: usize = 0x15c;
    pub const INTERRUPT_DEFINE: usize = 0x160;
    pub const IDENTITY: usize = 0x1618;
}

type Slot0 = extern "C" fn() -> i32;
type Slot1 = extern "C" fn(u32) -> i32;
type Slot2 = extern "C" fn(u32, u32) -> i32;
type Slot3 = extern "C" fn(u32, u32, u32) -> i32;

fn root(domain: Domain) -> usize {
    // SAFETY: the publication cells hold the two root addresses once the kernel is installed.
    unsafe { read_volatile((ROOT_CELLS + 4 * domain.index()) as *const usize) }
}

fn slot(root: usize, offset: usize) -> usize {
    // SAFETY: every offset used here is a published callable slot of a 0x162c-byte root.
    unsafe { read_volatile((root + offset) as *const usize) }
}

fn call0(root: usize, offset: usize) -> i32 {
    // SAFETY: published slots follow AAPCS.
    let f: Slot0 = unsafe { transmute(slot(root, offset)) };
    f()
}

fn call1(root: usize, offset: usize, a: u32) -> i32 {
    // SAFETY: published slots follow AAPCS.
    let f: Slot1 = unsafe { transmute(slot(root, offset)) };
    f(a)
}

fn call2(root: usize, offset: usize, a: u32, b: u32) -> i32 {
    // SAFETY: published slots follow AAPCS.
    let f: Slot2 = unsafe { transmute(slot(root, offset)) };
    f(a, b)
}

/// The domain of the executing core, from root A `+0x1618` (CP14 identity 0x4003 is core 0).
pub fn executing_domain() -> Domain {
    Domain::from_identity(call0(root(Domain::A), offset::IDENTITY) as u32)
}

fn executing_root() -> usize {
    root(executing_domain())
}

/// Root `+0x148`: the IRQ nesting word is nonzero.
pub fn handler_context() -> bool {
    call0(executing_root(), offset::HANDLER_CONTEXT) != 0
}

pub fn irq_masked() -> bool {
    let cpsr: u32;
    // SAFETY: reads the status register only.
    unsafe { asm!("mrs {0}, cpsr", out(reg) cpsr, options(nomem, nostack, preserves_flags)) };
    cpsr & 0x80 != 0
}

/// Root `+0x130`/`+0x134`: masks IRQ on this core when it is clear.
pub fn cpu_lock(handler: bool) {
    let offset = if handler { offset::CPU_LOCK_FROM_HANDLER } else { offset::CPU_LOCK };
    call0(executing_root(), offset);
}

/// Root `+0x138`/`+0x13c`: unmasks IRQ on this core; it never dispatches.
pub fn cpu_unlock(handler: bool) {
    let offset = if handler { offset::CPU_UNLOCK_FROM_HANDLER } else { offset::CPU_UNLOCK };
    call0(executing_root(), offset);
}

/// The `+0x0` packet: flags 2 as the stock task-submission wrapper 0x725dd8 passes, external
/// priority 1..16, stack `[base, base + bytes)`.
#[repr(C)]
pub struct CreatePacket {
    pub flags: u32,
    pub argument: u32,
    pub entry: u32,
    pub priority: u32,
    pub stack_bytes: u32,
    pub stack_base: u32,
}

/// Root `+0x80`, eventflag create for the task's own id, with `{attribute 0, pattern 0}`; task create 0x725dd8
/// passes attribute 0 when descriptor word +0x18 is 0xa5f0a5f0 and 4 otherwise.
pub fn register_auxiliary(domain: Domain, id: TaskId) -> SlotStatus {
    let packet = [0u32; 2];
    SlotStatus::from_raw(call2(root(domain), offset::REGISTER_AUXILIARY, EncodedId::new(domain, id).raw(), packet.as_ptr() as u32))
}

/// The allocator's packet; it builds the `+0x0` packet from these fields.
#[repr(C)]
pub struct AllocatePacket {
    pub entry: u32,
    pub priority: u32,
    pub stack_bytes: u32,
    pub stack_base: u32,
    pub argument: u32,
}

/// Creates and activates a class-1 task through the allocator.
pub fn allocate_and_activate(packet: &AllocatePacket) -> i32 {
    // SAFETY: the stock routine follows AAPCS and reads the packet only during the call.
    let f: Slot3 = unsafe { transmute(ALLOCATE_TASK) };
    f(0, 1, packet as *const AllocatePacket as u32)
}

/// Root `+0x0` of the owning domain; it writes that domain's records whatever core runs it.
pub fn create(domain: Domain, id: TaskId, packet: &CreatePacket) -> SlotStatus {
    SlotStatus::from_raw(call2(root(domain), offset::CREATE, EncodedId::new(domain, id).raw(), packet as *const CreatePacket as u32))
}

/// Root `+0xc` of the executing core; the class in the id selects the target domain.
pub fn activate(id: EncodedId) -> SlotStatus {
    SlotStatus::from_raw(call1(executing_root(), offset::ACTIVATE, id.raw()))
}

/// Root `+0x28`: -1 sleeps without a timer, a positive value is kernel clock units.
pub fn sleep(timeout: i32) -> SlotStatus {
    SlotStatus::from_raw(call1(executing_root(), offset::SLEEP, timeout as u32))
}

pub fn wakeup(id: EncodedId) -> SlotStatus {
    SlotStatus::from_raw(call1(executing_root(), offset::WAKEUP, id.raw()))
}

pub fn wakeup_from_handler(id: EncodedId) -> SlotStatus {
    SlotStatus::from_raw(call1(executing_root(), offset::WAKEUP_FROM_HANDLER, id.raw()))
}

/// An interrupt line of the 128-entry handler table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterruptLine(u8);

impl InterruptLine {
    pub const fn new(line: u8) -> Option<InterruptLine> {
        if line < 128 {
            Some(InterruptLine(line))
        } else {
            None
        }
    }
}

/// Handler-table priority field, 0..15.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterruptPriority(u8);

impl InterruptPriority {
    pub const fn new(priority: u8) -> Option<InterruptPriority> {
        if priority < 16 {
            Some(InterruptPriority(priority))
        } else {
            None
        }
    }
}

/// Root `+0x160` with `{ignored, handler, priority, 0, 1}`: `+0x100` bit 0 and mode 1, as stock
/// line 120. The handler runs under the stock IRQ entry, acknowledge and EOI with CPSR.I clear.
pub fn define_interrupt(line: InterruptLine, handler: extern "C" fn(), priority: InterruptPriority) -> SlotStatus {
    let packet = [0u32, handler as usize as u32, priority.0 as u32, 0, 1];
    SlotStatus::from_raw(call2(executing_root(), offset::INTERRUPT_DEFINE, line.0 as u32, packet.as_ptr() as u32))
}

/// Root `+0x158`; it programs the executing core's controller bank.
pub fn enable_interrupt(line: InterruptLine) -> SlotStatus {
    SlotStatus::from_raw(call1(executing_root(), offset::INTERRUPT_ENABLE, line.0 as u32))
}

/// Root `+0x15c`; it programs the executing core's controller bank.
pub fn disable_interrupt(line: InterruptLine) -> SlotStatus {
    SlotStatus::from_raw(call1(executing_root(), offset::INTERRUPT_DISABLE, line.0 as u32))
}

/// The global tick cell, advanced once per kernel tick by core 0's line-32 handler.
pub fn tick() -> u32 {
    // SAFETY: an always-mapped kernel word.
    unsafe { read_volatile(TICK_CELL as *const u32) }
}

pub fn timer_count() -> u32 {
    // SAFETY: the kernel timer counter register; reading has no side effect.
    unsafe { read_volatile(TIMER_COUNTER as *const u32) }
}

pub fn clean_dcache(start: usize, bytes: usize) {
    // SAFETY: the stock routine takes an address and a byte count and touches only cache state.
    let f: Slot2 = unsafe { transmute(CLEAN_DCACHE) };
    f(start as u32, bytes as u32);
}

/// Holds the stock cross-core critical pair until dropped.
pub struct CriticalPair(u32);

impl CriticalPair {
    pub fn enter() -> CriticalPair {
        // SAFETY: the stock routine takes no arguments and returns the prior mask.
        let f: Slot0 = unsafe { transmute(CRITICAL_ENTER) };
        CriticalPair(f() as u32)
    }
}

impl Drop for CriticalPair {
    fn drop(&mut self) {
        // SAFETY: the stock routine takes the mask its pair returned.
        let f: Slot1 = unsafe { transmute(CRITICAL_LEAVE) };
        f(self.0);
    }
}
