use core::cell::UnsafeCell;
use core::ptr::{read_volatile, write_volatile, NonNull};
use core::task::Poll;

use super::boot_skip::BootSkipTable;
use super::dma_read::{BlockDisposition, ByteCount, Instant, LowerStatus, ReadPorts};
use super::{Controller, DriverState, NandSemaphore, PhysicalBlock, PinClaim, BLOCK_BYTES, PAGES_PER_BLOCK};

#[path = "../../embassy_platform/src/clock.rs"]
mod qualified_clock;
pub use qualified_clock::CounterDirection;

const NAND: u32 = 0xfff5_2000;
const DMA: u32 = 0xfff5_2880;
const STATE: u32 = 0x0091_6528;
const INDEX: u32 = 0x0091_6530;
const HANDLES: u32 = 0x0091_6534;
const HANDLE: u32 = HANDLES + 4;
const TOKEN: u32 = 0x0091_6540;
const COMPLETION: u32 = 0x0091_6548;
const HANDLE_COUNTER: u32 = 0x409c_2804;
const DEFAULT_COMPLETION: u32 = 0x006e_d7b8;
const DMA_BUDGET: u32 = 0x10_0000;

extern "C" {
    // X-M1 55436: 0x724b68, 0x724c00; timeout zero returns -50 when unavailable.
    fn xm1_power_semaphore_wait(identifier: i32, timeout: i32) -> i32;
    fn xm1_power_semaphore_signal(identifier: i32) -> i32;
    // X-M1 55436: 0x42634.
    fn xm1_lower_read_clean_invalidate(start: u32, bytes: u32);
    // Core-0 loader leaves 0x42560, 0x42568.
    fn xm1_lower_read_clock_mask() -> u32;
    fn xm1_lower_read_clock_restore(saved: u32);
}

unsafe fn load(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn store(address: u32, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }

fn lower(code: i32) -> LowerStatus {
    match LowerStatus::new(code) {
        Some(status) => status,
        None => unreachable!(),
    }
}

fn status(code: i32) -> Result<(), LowerStatus> {
    if code == 0 { Ok(()) } else { Err(lower(code)) }
}

fn pin_failure(error: super::Error) -> LowerStatus {
    match error {
        super::Error::PinRoute(failure) => lower(failure.status.code()),
        other => lower(other.request_status()),
    }
}

#[derive(Clone, Copy)]
pub enum DrainContract {
    /// FujiMod revision 3d90035d synchronously copies one 128 KiB block before
    /// clearing the modeled NAND and DMA activity bits.
    FujiMod3d90035d,
    /// A device-specific leaf established independently of this owner.
    External {
        context: NonNull<()>,
        quiescent: unsafe extern "C" fn(*mut ()) -> u32,
    },
    UnqualifiedPhysical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelLeafContract { UncontendedRunnableTask }

#[derive(Clone, Copy)]
pub struct PrefixContext(NonNull<()>);

impl PrefixContext {
    pub const fn new(context: NonNull<()>) -> Self { Self(context) }
    pub const fn as_ptr(self) -> *mut () { self.0.as_ptr() }
}

pub type PrefixConsumer = unsafe fn(PrefixContext, ByteCount) -> Result<(), LowerStatus>;

#[derive(Clone, Copy)]
pub enum ProgressSink {
    None,
    Consumer { context: PrefixContext, consume: PrefixConsumer },
}

#[derive(Clone, Copy)]
pub struct ClockDevice {
    pub tick: unsafe extern "C" fn() -> u32,
    pub counter: unsafe extern "C" fn() -> u32,
}

#[derive(Clone, Copy)]
pub enum ClockSource {
    NativeCore0,
    ExternalDevice(ClockDevice),
}

pub struct ClockBinding {
    clock: UnsafeCell<qualified_clock::Clock>,
    direction: CounterDirection,
    source: ClockSource,
}

unsafe impl Sync for ClockBinding {}

impl ClockBinding {
    /// # Safety
    /// Every accessor runs on core 0. The counter direction matches the selected device; external accessors are nonblocking and do not reenter this binding.
    pub const unsafe fn new(direction: CounterDirection, source: ClockSource) -> Self {
        Self { clock: UnsafeCell::new(qualified_clock::Clock::new()), direction, source }
    }

    pub fn now(&self) -> Instant {
        let saved = unsafe { xm1_lower_read_clock_mask() };
        let tick = || match self.source {
            ClockSource::NativeCore0 => unsafe { load(0x400c_0800) },
            ClockSource::ExternalDevice(device) => unsafe { (device.tick)() },
        };
        let before = tick();
        let count = match self.source {
            ClockSource::NativeCore0 => unsafe { load(0xfff9_1000) },
            ClockSource::ExternalDevice(device) => unsafe { (device.counter)() },
        };
        let after = tick();
        let count = if before == after { count } else {
            match self.direction {
                CounterDirection::Up => 0,
                CounterDirection::Down => qualified_clock::RELOAD,
            }
        };
        let now = unsafe { (&mut *self.clock.get()).now(after, count, self.direction).now };
        unsafe { xm1_lower_read_clock_restore(saved); }
        Instant::new(now)
    }
}

struct CriticalLease { saved: u32, root: u32, counted: bool }

impl CriticalLease {
    unsafe fn try_enter() -> Option<Self> {
        let saved = unsafe { xm1_lower_read_clock_mask() };
        let root = unsafe { load(0x0096_bd20) };
        let owner = unsafe { load(0x400c_0804) };
        if owner == 17 { return Some(Self { saved, root, counted: false }); }
        if owner != 33 {
            let acquire: unsafe extern "C" fn(u32, u32) -> i32 =
                unsafe { core::mem::transmute(load(root + 0x161c) as usize) };
            if unsafe { acquire(0, 33) } != 0 {
                unsafe { xm1_lower_read_clock_restore(saved); }
                return None;
            }
        }
        unsafe { store(0x40a0_f824, load(0x40a0_f824).wrapping_add(1)); }
        Some(Self { saved, root, counted: true })
    }

    unsafe fn release(self) {
        if self.counted {
            let remaining = unsafe { load(0x40a0_f824).wrapping_sub(1) };
            unsafe { store(0x40a0_f824, remaining); }
            if remaining == 0 {
                let release: unsafe extern "C" fn(u32) =
                    unsafe { core::mem::transmute(load(self.root + 0x1620) as usize) };
                unsafe { release(0); }
            }
        }
        unsafe { xm1_lower_read_clock_restore(self.saved); }
    }
}

struct IrqToken(u32);

fn detach_irq(token: IrqToken) -> Poll<Result<(), LowerStatus>> {
    let Some(critical) = (unsafe { CriticalLease::try_enter() }) else { return Poll::Pending; };
    let detached = (|| {
        // 0x64eb20 validates against this 128-entry IRQ handle table.
        let index = (0..128u32).find(|index| unsafe { load(0x0094_0e9c + index * 4) == token.0 })
            .ok_or_else(|| lower(-20))?;
        let disable: unsafe extern "C" fn(u32) -> i32 =
            unsafe { core::mem::transmute(load(critical.root + 0x15c) as usize) };
        let reset: unsafe extern "C" fn(u32, u32) -> i32 =
            unsafe { core::mem::transmute(load(critical.root + 0x160) as usize) };
        status(unsafe { disable(index) })?;
        status(unsafe { reset(index, 0) })?;
        unsafe {
            store(0x0094_109c + index * 4, u32::MAX);
            store(0x0094_0e9c + index * 4, 0);
        }
        Ok(())
    })();
    unsafe { critical.release(); }
    Poll::Ready(detached)
}

#[derive(Clone, Copy)]
struct ChannelHandle(u32);

#[derive(Clone, Copy, PartialEq, Eq)]
struct DmaAddress(u32);

#[repr(u32)]
#[derive(Clone, Copy)]
enum SoftwareState { Acquired = 1, Active = 2 }

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChannelPhase { Idle, Active, StopRequested, IdleStopRequested, Drained, Released }

#[derive(Clone, Copy, PartialEq, Eq)]
enum PinLease { Unclaimed, Attempted, Held, Released }

pub struct DriverLease {
    held: bool,
    state_held: bool,
    pins: PinLease,
    controller: Controller,
    pending_dma: bool,
}

pub struct ChannelLease {
    handle: ChannelHandle,
    phase: ChannelPhase,
    destination_end: Option<DmaAddress>,
}

pub struct NandReadPorts {
    clock: &'static ClockBinding,
    drain_contract: DrainContract,
    driver_held: bool,
    table: Option<BootSkipTable<'static>>,
    progress: ProgressSink,
}

impl NandReadPorts {
    /// # Safety
    /// Stock semaphore and pin-route services are initialized. Controller users honor Lifecycle 36, State 52 and DMA resource 10.
    /// The boot skip table remains immutable and mapped. Destinations have valid uncached aliases.
    /// FujiMod qualification requires device revision 3d90035d.
    /// All calls run on core 0. Kernel semaphore locks are uncontended; the caller remains runnable in task context.
    /// Consumer context stays valid while the owner retains the operation; its callback is nonblocking.
    pub const unsafe fn new(clock: &'static ClockBinding, drain_contract: DrainContract, _kernel_contract: KernelLeafContract, progress: ProgressSink) -> Self {
        Self {
            clock,
            drain_contract,
            driver_held: false,
            table: None,
            progress,
        }
    }

    fn check_channel(&self, channel: &ChannelLease, state: SoftwareState) -> Result<(), LowerStatus> {
        if !self.driver_held || channel.phase == ChannelPhase::Released {
            return Err(lower(-20));
        }
        unsafe {
            if load(HANDLE) != channel.handle.0 || load(INDEX) != 1 { return Err(lower(-20)); }
            if load(STATE) != state as u32 { return Err(lower(-12)); }
        }
        Ok(())
    }

    fn modeled_drain(&self) -> bool {
        match self.drain_contract {
            DrainContract::FujiMod3d90035d => {
                // FujiMod 3d90035d clears receive activity after its synchronous destination copy.
                unsafe { load(DMA + 0x10) & 1 == 0 && load(NAND + 4) & 4 == 0 }
            }
            DrainContract::External { context, quiescent } => unsafe { quiescent(context.as_ptr()) != 0 },
            DrainContract::UnqualifiedPhysical => false,
        }
    }
}

// SAFETY: FujiMod drain is restricted to the pinned synchronous-copy revision; External drain
// is the constructor caller's device-specific no-further-writes proof; UnqualifiedPhysical never drains.
unsafe impl ReadPorts for NandReadPorts {
    type Driver = DriverLease;
    type Channel = ChannelLease;

    fn now(&mut self) -> Instant { self.clock.now() }

    fn try_driver(&mut self) -> Poll<Result<Self::Driver, LowerStatus>> {
        if self.driver_held { return Poll::Ready(Err(lower(-11))); }
        match unsafe { xm1_power_semaphore_wait(NandSemaphore::Lifecycle as i32, 0) } {
            0 => {
                self.driver_held = true;
                Poll::Ready(Ok(DriverLease {
                    held: true,
                    state_held: false,
                    pins: PinLease::Unclaimed,
                    controller: Controller::new(),
                    pending_dma: false,
                }))
            }
            -50 => Poll::Pending,
            code => Poll::Ready(Err(lower(code))),
        }
    }

    fn try_channel(&mut self, driver: &mut Self::Driver) -> Poll<Result<Self::Channel, LowerStatus>> {
        if !driver.held || !self.driver_held { return Poll::Ready(Err(lower(-11))); }
        if !driver.state_held {
            return match unsafe { xm1_power_semaphore_wait(NandSemaphore::State as i32, 0) } {
                0 => {
                    driver.state_held = true;
                    self.table = unsafe { BootSkipTable::from_boot() }.ok();
                    Poll::Pending
                }
                -50 => Poll::Pending,
                code => Poll::Ready(Err(lower(code))),
            };
        }
        unsafe {
            if load(STATE) != 0 || load(HANDLE) != 0 || load(TOKEN) != 0 {
                return Poll::Pending;
            }
        }
        if driver.pins == PinLease::Unclaimed {
            driver.pins = PinLease::Attempted;
            if let Err(error) = unsafe { super::claim_pins(PinClaim::Held) } {
                return Poll::Ready(Err(pin_failure(error)));
            }
            driver.pins = PinLease::Held;
            return Poll::Pending;
        }
        if driver.controller.state == DriverState::Stopped {
            unsafe { driver.controller.configure(); }
            return Poll::Pending;
        }
        if !driver.pending_dma {
            match unsafe { xm1_power_semaphore_wait(10, 0) } {
                -50 => return Poll::Pending,
                0 => driver.pending_dma = true,
                code => return Poll::Ready(Err(lower(code))),
            }
        }
        let Some(critical) = (unsafe { CriticalLease::try_enter() }) else { return Poll::Pending; };
        let handle = unsafe {
            // 0x65722c allocates from this counter and the same two-entry handle table.
            let other = load(HANDLES);
            let mut next = load(HANDLE_COUNTER).wrapping_add(1);
            if next == 0 { next = 1; }
            if next == other {
                next = next.wrapping_add(1);
                if next == 0 { next = 1; }
            }
            store(HANDLE_COUNTER, next);
            store(HANDLE, next);
            store(STATE, 1);
            store(INDEX, 1);
            store(COMPLETION, DEFAULT_COMPLETION);
            store(DMA + 4, load(DMA + 4) & !1);
            critical.release();
            ChannelHandle(next)
        };
        driver.pending_dma = false;
        Poll::Ready(Ok(ChannelLease { handle, phase: ChannelPhase::Idle, destination_end: None }))
    }

    fn classify(&mut self, block: PhysicalBlock) -> Result<BlockDisposition, LowerStatus> {
        if !self.driver_held { return Err(lower(-11)); }
        let table = self.table.as_ref().ok_or_else(|| lower(-10))?;
        Ok(if table.iter().any(|entry| entry.index() == block.index()) {
            BlockDisposition::Bad
        } else {
            BlockDisposition::Usable
        })
    }

    fn start(&mut self, channel: &mut Self::Channel, block: PhysicalBlock, destination: &mut [u8]) -> Result<(), LowerStatus> {
        self.check_channel(channel, SoftwareState::Acquired)?;
        if channel.phase != ChannelPhase::Idle || destination.len() != BLOCK_BYTES as usize {
            return Err(lower(-30));
        }
        let pointer = destination.as_mut_ptr() as usize;
        let pointer = u32::try_from(pointer).map_err(|_| lower(-30))?;
        if pointer & 31 != 0 { return Err(lower(-30)); }
        let alias = pointer | 0x4000_0000;
        let end = alias.checked_add(BLOCK_BYTES).ok_or_else(|| lower(-30))?;
        unsafe {
            if load(TOKEN) != 0 { return Err(lower(-11)); }
            store(NAND, 1);
            store(NAND + 0x98, 0x8000_0000);
            let tag = load(0x0081_2b9c) & 7;
            let control = (load(DMA + 0x20) & !0x1070_3000) | 0x8000_1000 | (tag << 20);
            store(DMA + 0x20, control);
            store(DMA + 4, 0x10000);
            store(DMA + 0x24, alias);
            store(DMA + 0x28, DMA_BUDGET);
            store(STATE, 2);
            channel.phase = ChannelPhase::Active;
            channel.destination_end = Some(DmaAddress(end));
            store(DMA + 0x0c, 1);
            store(DMA + 0x44, 0);
            store(COMPLETION, 0);
            store(DMA + 0x20, load(DMA + 0x20) & !0x1000_0000);
            xm1_lower_read_clean_invalidate(0, 0);
            store(DMA + 4, load(DMA + 4) | 1);
            // 0x6f1600 encodes 256 sectors as a zero low byte in application receive mode.
            store(NAND + 0x1c, 0x0030_0000);
            store(NAND + 0x18, 0x8003);
            let page = block.index() * PAGES_PER_BLOCK;
            store(NAND + 0x60, page);
            store(NAND + 0x20, 0);
            store(NAND + 0x24, page & 0x00ff_ffff);
            store(NAND + 4, 0x108);
            // 0x6f1a40 and 0x6f167c produce the table-loader's polling DMA command.
            store(NAND + 8, 0x0080_23f3);
        }
        Ok(())
    }

    fn poll_controller(&mut self, channel: &mut Self::Channel) -> Poll<Result<(), LowerStatus>> {
        if let Err(error) = self.check_channel(channel, SoftwareState::Active) { return Poll::Ready(Err(error)); }
        unsafe {
            let timing = load(NAND + 4);
            if timing & 4 != 0 { return Poll::Pending; }
            if matches!(self.drain_contract, DrainContract::FujiMod3d90035d) {
                if load(NAND + 0x54) & 1 == 0 || load(DMA + 0x10) & 1 != 0
                    || Some(DmaAddress(load(DMA + 0x24))) != channel.destination_end
                    || load(DMA + 0x28) != DMA_BUDGET - BLOCK_BYTES {
                    return Poll::Pending;
                }
            }
            Poll::Ready(if timing & 0x100 != 0 { Err(lower(10)) } else { Ok(()) })
        }
    }

    fn request_stop(&mut self, channel: &mut Self::Channel) -> Result<(), LowerStatus> {
        if channel.phase == ChannelPhase::Idle {
            self.check_channel(channel, SoftwareState::Acquired)?;
            channel.phase = ChannelPhase::IdleStopRequested;
            return Ok(());
        }
        self.check_channel(channel, SoftwareState::Active)?;
        if channel.phase != ChannelPhase::Active { return Err(lower(-11)); }
        unsafe {
            store(DMA + 0x0c, 1);
            store(DMA + 4, 0x10000);
            store(DMA + 4, load(DMA + 4) & !1);
        }
        channel.phase = ChannelPhase::StopRequested;
        Ok(())
    }

    fn poll_quiescent(&mut self, channel: &mut Self::Channel) -> Poll<Result<(), LowerStatus>> {
        if channel.phase == ChannelPhase::IdleStopRequested {
            if let Err(error) = self.check_channel(channel, SoftwareState::Acquired) { return Poll::Ready(Err(error)); }
            channel.phase = ChannelPhase::Drained;
            return Poll::Ready(Ok(()));
        }
        if let Err(error) = self.check_channel(channel, SoftwareState::Active) { return Poll::Ready(Err(error)); }
        if channel.phase != ChannelPhase::StopRequested { return Poll::Ready(Err(lower(-11))); }
        if !self.modeled_drain() { return Poll::Pending; }
        unsafe {
            store(DMA + 0x20, load(DMA + 0x20) & !0x1000_0000);
            let token = load(TOKEN);
            if token != 0 {
                match detach_irq(IrqToken(token)) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                    Poll::Ready(Ok(())) => (),
                }
                store(TOKEN, 0);
            }
            store(COMPLETION, DEFAULT_COMPLETION);
            store(STATE, 1);
        }
        channel.phase = ChannelPhase::Drained;
        Poll::Ready(Ok(()))
    }

    fn finish_transfer(&mut self, channel: &mut Self::Channel) -> Result<(), LowerStatus> {
        self.check_channel(channel, SoftwareState::Acquired)?;
        if channel.phase != ChannelPhase::Drained { return Err(lower(-11)); }
        unsafe {
            store(NAND + 0x10, 0);
            store(NAND + 0x18, 0x80a3);
            store(NAND + 8, 0);
        }
        channel.phase = ChannelPhase::Idle;
        Ok(())
    }

    fn close_channel(&mut self, channel: &mut Self::Channel) -> Result<(), LowerStatus> {
        if channel.phase == ChannelPhase::Drained { self.finish_transfer(channel)?; }
        self.check_channel(channel, SoftwareState::Acquired)?;
        if channel.phase != ChannelPhase::Idle {
            return Err(lower(-11));
        }
        unsafe {
            if load(TOKEN) != 0 { return Err(lower(-11)); }
            let released = status(xm1_power_semaphore_signal(10));
            if released.is_ok() {
                store(DMA + 4, load(DMA + 4) & !1);
                store(INDEX, u32::MAX);
                store(STATE, 0);
                store(HANDLE, 0);
                store(COMPLETION, 0);
                channel.phase = ChannelPhase::Released;
            }
            released
        }
    }

    fn unlock_driver(&mut self, driver: &mut Self::Driver) -> Result<(), LowerStatus> {
        if !driver.held || !self.driver_held { return Err(lower(-11)); }
        if driver.pending_dma {
            status(unsafe { xm1_power_semaphore_signal(10) })?;
            driver.pending_dma = false;
        }
        unsafe { driver.controller.shutdown(); }
        if matches!(driver.pins, PinLease::Attempted | PinLease::Held) {
            unsafe { super::claim_pins(PinClaim::Released) }.map_err(pin_failure)?;
            driver.pins = PinLease::Released;
        }
        if driver.state_held {
            status(unsafe { xm1_power_semaphore_signal(NandSemaphore::State as i32) })?;
            driver.state_held = false;
        }
        status(unsafe { xm1_power_semaphore_signal(NandSemaphore::Lifecycle as i32) })?;
        driver.held = false;
        self.driver_held = false;
        self.table = None;
        Ok(())
    }

    fn progress(&mut self, completed: ByteCount) -> Result<(), LowerStatus> {
        match self.progress {
            ProgressSink::None => Ok(()),
            ProgressSink::Consumer { context, consume } => unsafe { consume(context, completed) },
        }
    }
}
