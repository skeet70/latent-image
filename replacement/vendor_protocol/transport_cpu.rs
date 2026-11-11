use core::convert::TryFrom;
use super::{Channel, TransportError};
use core::num::NonZeroU32;

#[cfg(test)]
#[path = "transport_cpu_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverHandle(NonZeroU32);
impl DriverHandle {
    pub const fn from_raw(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) { Some(value) => Some(Self(value)), None => None }
    }
    pub const fn get(self) -> u32 { self.0.get() }
}

pub struct HandleSequence(u32);
impl HandleSequence {
    pub const fn new(last: u32) -> Self { Self(last) }
    fn next(&mut self) -> DriverHandle {
        self.0 = self.0.wrapping_add(1);
        if self.0 == 0 { self.0 = 1; }
        DriverHandle(NonZeroU32::new(self.0).unwrap())
    }
}

pub struct HandleTable<'a> { entries: &'a mut [Option<DriverHandle>] }
impl<'a> HandleTable<'a> {
    pub fn new(entries: &'a mut [Option<DriverHandle>]) -> Self { Self { entries } }
    pub fn allocate(&mut self, index: usize, sequence: &mut HandleSequence) -> Result<DriverHandle, TransportError> {
        if index >= self.entries.len() { return Err(TransportError::InvalidChannel); }
        if self.entries[index].is_some() { return Err(TransportError::ChannelBusy); }
        loop {
            let candidate = sequence.next();
            if !self.entries.contains(&Some(candidate)) {
                self.entries[index] = Some(candidate);
                return Ok(candidate);
            }
        }
    }
    pub fn find(&self, handle: DriverHandle) -> Result<usize, TransportError> {
        self.entries.iter().position(|entry| *entry == Some(handle)).ok_or(TransportError::InvalidChannel)
    }
    pub fn release(&mut self, handle: DriverHandle) -> Result<(), TransportError> {
        let index = self.find(handle)?;
        self.entries[index] = None;
        Ok(())
    }
    pub fn channel(&self, handle: DriverHandle) -> Result<Channel, TransportError> {
        match self.find(handle)? { 0 => Ok(Channel::Zero), 1 => Ok(Channel::One), _ => Err(TransportError::InvalidChannel) }
    }
}

pub trait CacheInstructions {
    fn level_id(&mut self) -> u32;
    fn select_level(&mut self, selector: u32);
    fn instruction_barrier(&mut self);
    fn size_id(&mut self) -> u32;
    fn clean_invalidate_set_way(&mut self, operand: u32);
    fn clean_invalidate_address(&mut self, address: u32);
}

pub fn clean_invalidate_all(cache: &mut impl CacheInstructions) {
    let level_id = cache.level_id();
    let end = (level_id & 0x0700_0000) >> 23;
    let mut selector = 0;
    while selector < end {
        let cache_type = (level_id >> (selector + (selector >> 1))) & 7;
        if cache_type >= 2 {
            cache.select_level(selector);
            cache.instruction_barrier();
            let size = cache.size_id();
            let line_shift = (size & 7) + 4;
            let max_way = (size >> 3) & 0x3ff;
            let way_shift = max_way.leading_zeros();
            let max_set = (size >> 13) & 0x7fff;
            for set in (0..=max_set).rev() {
                for way in (0..=max_way).rev() {
                    let way_bits = if way_shift == 32 { 0 } else { way << way_shift };
                    cache.clean_invalidate_set_way(selector | way_bits | (set << line_shift));
                }
            }
        }
        selector += 2;
    }
}

pub fn clean_invalidate_range(cache: &mut impl CacheInstructions, address: u32, length: u32) -> Result<(), TransportError> {
    if address == 0 || length == 0 { clean_invalidate_all(cache); return Ok(()); }
    let end = address.checked_add(length).ok_or(TransportError::AddressOverflow)?;
    let mut line = address & !31;
    loop {
        cache.clean_invalidate_address(line);
        line = line.wrapping_add(32);
        if (line as i32) >= (end as i32) { break; }
    }
    Ok(())
}

pub trait DelayCounter {
    fn reload(&mut self) -> u32;
    fn current(&mut self) -> u32;
}

pub fn spin_delay(counter: &mut impl DelayCounter, milliseconds: u32) -> Result<(), TransportError> {
    let reload = counter.reload();
    if reload == 0 { return Err(TransportError::InvalidTimer); }
    let divisor = 10_000_000 / reload;
    if divisor == 0 { return Err(TransportError::InvalidTimer); }
    let target = milliseconds.wrapping_mul(1000) / divisor;
    let mut previous = counter.reload().wrapping_sub(counter.current());
    let mut elapsed = 0u32;
    loop {
        let now = counter.reload().wrapping_sub(counter.current());
        let delta = if previous >= now { previous - now } else { reload.wrapping_add(previous).wrapping_add(1).wrapping_sub(now) };
        elapsed = elapsed.wrapping_add(delta);
        if elapsed >= target { return Ok(()); }
        previous = now;
    }
}

#[cfg(target_arch = "arm")]
pub struct ArmCacheInstructions;
#[cfg(target_arch = "arm")]
impl CacheInstructions for ArmCacheInstructions {
    fn level_id(&mut self) -> u32 {
        let value: u32;
        unsafe { core::arch::asm!("mrc p15, 1, {value}, c0, c0, 1", value = out(reg) value, options(nostack)); }
        value
    }
    fn select_level(&mut self, selector: u32) {
        unsafe { core::arch::asm!("mcr p15, 2, {value}, c0, c0, 0", value = in(reg) selector, options(nostack)); }
    }
    fn instruction_barrier(&mut self) { unsafe { core::arch::asm!("isb sy", options(nostack)); } }
    fn size_id(&mut self) -> u32 {
        let value: u32;
        unsafe { core::arch::asm!("mrc p15, 1, {value}, c0, c0, 0", value = out(reg) value, options(nostack)); }
        value
    }
    fn clean_invalidate_set_way(&mut self, operand: u32) {
        unsafe { core::arch::asm!("mcr p15, 0, {value}, c7, c14, 2", value = in(reg) operand, options(nostack)); }
    }
    fn clean_invalidate_address(&mut self, address: u32) {
        unsafe { core::arch::asm!("mcr p15, 0, {value}, c7, c14, 1", value = in(reg) address, options(nostack)); }
    }
}

pub struct MmioDelayCounter;
impl DelayCounter for MmioDelayCounter {
    fn reload(&mut self) -> u32 { unsafe { core::ptr::read_volatile(0xfff9_1004usize as *const u32) } }
    fn current(&mut self) -> u32 { unsafe { core::ptr::read_volatile(0xfff9_1000usize as *const u32) } }
}

pub struct PublishedInterruptHandlers {
    zero: super::irq_control::Handler,
    one: super::irq_control::Handler,
}

impl PublishedInterruptHandlers {
    pub const fn from_handlers(zero: super::irq_control::Handler, one: super::irq_control::Handler) -> Self { Self { zero, one } }
    pub fn new(zero: extern "C" fn(), one: extern "C" fn()) -> Result<Self, TransportError> {
        fn handler(callback: extern "C" fn()) -> Result<super::irq_control::Handler, TransportError> {
            let address = u32::try_from(callback as usize).map_err(|_| TransportError::AddressOverflow)?;
            super::irq_control::Handler::new(address).ok_or(TransportError::AddressOverflow)
        }
        Ok(Self { zero: handler(zero)?, one: handler(one)? })
    }
}

pub fn register_dma_irq<B: super::irq_control::Backend>(
    backend: &B,
    kernel_table: super::irq_control::Table,
    handles: &mut HandleTable<'_>,
    sequence: &mut HandleSequence,
    channel: Channel,
    handlers: &PublishedInterruptHandlers,
) -> Result<DriverHandle, TransportError> {
    use super::irq_control::{self, InterruptLine, Registration};
    let handler = match channel { Channel::Zero => handlers.zero, Channel::One => handlers.one };
    let line = InterruptLine::from_static(85);
    let handle = handles.allocate(line.get() as usize, sequence)?;
    let registration = Registration::new(handler.get(), 13, 0, 1).ok_or(TransportError::InvalidChannel)?;
    irq_control::configure(backend, kernel_table, line, registration);
    irq_control::enable(backend, kernel_table, line).map_err(|_| TransportError::InvalidChannel)?;
    Ok(handle)
}

pub fn open_channel(
    channel: Channel,
    handles: &mut HandleTable<'_>,
    sequence: &mut HandleSequence,
    io: &mut impl super::ControllerIo,
) -> Result<(super::DmaChannel, DriverHandle), TransportError> {
    let handle = handles.allocate(channel as usize, sequence)?;
    let control = io.read32(super::Register::ChannelControl(channel));
    io.write32(super::Register::ChannelControl(channel), control & !1);
    Ok((super::DmaChannel::open(channel), handle))
}

pub struct IrqContext<'a, B> {
    pub backend: &'a B,
    pub table: super::irq_control::Table,
    pub handles: HandleTable<'a>,
    pub sequence: &'a mut HandleSequence,
    pub handlers: PublishedInterruptHandlers,
    pub sources: &'a mut [u32; 128],
    pub channel_handles: [Option<DriverHandle>; 2],
}

pub struct ChannelRuntime<'a, B, I, C, T> {
    pub io: I,
    pub irq: IrqContext<'a, B>,
    pub cache: C,
    pub timer: T,
}

impl<B, I: super::ControllerIo, C, T> super::ControllerIo for ChannelRuntime<'_, B, I, C, T> {
    fn read32(&mut self, register: super::Register) -> u32 { self.io.read32(register) }
    fn read16(&mut self, register: super::Register) -> u16 { self.io.read16(register) }
    fn write32(&mut self, register: super::Register, value: u32) { self.io.write32(register, value); }
    fn write16(&mut self, register: super::Register, value: u16) { self.io.write16(register, value); }
}

impl<B: super::irq_control::Backend, I: super::ControllerIo, C: CacheInstructions, T: DelayCounter>
    super::ChannelPlatform for ChannelRuntime<'_, B, I, C, T> {
    fn register_interrupt(&mut self, channel: Channel, _: super::ChannelCallback) -> Result<(), TransportError> {
        let handle = register_dma_irq(self.irq.backend, self.irq.table, &mut self.irq.handles, self.irq.sequence, channel, &self.irq.handlers)?;
        self.irq.channel_handles[channel as usize] = Some(handle);
        self.irq.sources[85] = 0x46;
        Ok(())
    }
    fn synchronize(&mut self) { clean_invalidate_all(&mut self.cache); }
    fn delay_start(&mut self) -> Result<(), TransportError> { spin_delay(&mut self.timer, 1) }
}

pub fn release_interrupt_handle<B: super::irq_control::Backend>(
    irq: &mut IrqContext<'_, B>,
    handle: DriverHandle,
) -> Result<(), TransportError> {
    use super::irq_control::{self, InterruptLine};
    let index = irq.handles.find(handle)?;
    let line = InterruptLine::new(index as u32).ok_or(TransportError::InvalidChannel)?;
    #[cfg(target_arch = "arm")]
    let saved = unsafe {
        extern "C" { fn xm1_power_interrupt_lock() -> u32; }
        xm1_power_interrupt_lock()
    };
    let result = irq_control::disable(irq.backend, irq.table, line)
        .map_err(|_| TransportError::InvalidChannel);
    let result = if result.is_ok() {
        irq_control::define(irq.backend, irq.table, line.get(), None);
        irq.sources[index] = u32::MAX;
        irq.handles.release(handle)
    } else { result };
    #[cfg(target_arch = "arm")]
    unsafe {
        extern "C" { fn xm1_power_interrupt_unlock(saved: u32); }
        xm1_power_interrupt_unlock(saved);
    }
    result
}

pub trait ChannelSemaphore {
    fn release(&mut self, channel: Channel) -> super::ServiceStatus;
}

#[cfg(target_arch = "arm")]
pub struct NativeChannelSemaphore;
#[cfg(target_arch = "arm")]
impl ChannelSemaphore for NativeChannelSemaphore {
    fn release(&mut self, channel: Channel) -> super::ServiceStatus {
        extern "C" { fn xm1_power_semaphore_signal(id: i32) -> i32; }
        let id = match channel { Channel::Zero => 19, Channel::One => 20 };
        super::ServiceStatus(unsafe { xm1_power_semaphore_signal(id) })
    }
}

pub struct TeardownRuntime<'a, 'irq, B, I, C, T, S> {
    pub runtime: &'a mut ChannelRuntime<'irq, B, I, C, T>,
    pub channel_handles: HandleTable<'a>,
    pub semaphores: S,
    pub pending_semaphores: [Option<DriverHandle>; 2],
}

impl<B, I: super::ControllerIo, C, T, S> super::ControllerIo for TeardownRuntime<'_, '_, B, I, C, T, S> {
    fn read32(&mut self, register: super::Register) -> u32 { self.runtime.io.read32(register) }
    fn read16(&mut self, register: super::Register) -> u16 { self.runtime.io.read16(register) }
    fn write32(&mut self, register: super::Register, value: u32) { self.runtime.io.write32(register, value); }
    fn write16(&mut self, register: super::Register, value: u16) { self.runtime.io.write16(register, value); }
}

impl<B: super::irq_control::Backend, I: super::ControllerIo, C, T, S: ChannelSemaphore>
    super::TeardownPlatform for TeardownRuntime<'_, '_, B, I, C, T, S> {
    fn release_interrupt(&mut self, handle: DriverHandle) -> Result<(), TransportError> {
        release_interrupt_handle(&mut self.runtime.irq, handle)
    }
    fn release_channel_interrupt(&mut self, channel: Channel) -> Result<(), TransportError> {
        if let Some(handle) = self.runtime.irq.channel_handles[channel as usize] {
            release_interrupt_handle(&mut self.runtime.irq, handle)?;
            self.runtime.irq.channel_handles[channel as usize] = None;
        }
        Ok(())
    }
    fn release_channel(&mut self, channel: Channel, handle: DriverHandle) -> Result<(), TransportError> {
        if self.pending_semaphores[channel as usize] != Some(handle) {
            let actual = self.channel_handles.channel(handle)?;
            if actual != channel { return Err(TransportError::InvalidChannel); }
            let control = self.runtime.io.read32(super::Register::ChannelControl(channel));
            self.runtime.io.write32(super::Register::ChannelControl(channel), control & !1);
            self.channel_handles.release(handle)?;
            self.pending_semaphores[channel as usize] = Some(handle);
        }
        let status = self.semaphores.release(channel);
        if status.0 != 0 { return Err(TransportError::Release(status)); }
        self.pending_semaphores[channel as usize] = None;
        Ok(())
    }
}

#[cfg(target_arch = "arm")]
pub fn cancel_native_transfer_timer(timer: super::TransferTimer) -> super::ServiceStatus {
    use core::ptr::{read_volatile, write_volatile};
    let address = timer.address() as *mut u32;
    let raw = unsafe { read_volatile(address) };
    let mut handle = Some(super::RegistryHandle(raw));
    let status = super::cancel_transfer_timer(
        &mut handle,
        &mut xm1_power_services::shutdown_task7_8_transport::NativeRegistrationRegistry,
    );
    unsafe { write_volatile(address, 0); }
    status
}
