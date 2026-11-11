use super::transport::{self, cpu, irq_control, ControllerIo, Register, TransportError};

#[cfg(test)]
#[path = "transport_start_cpu_tests.rs"]
mod tests;

const CONTROLLER: usize = 0x0c;
const CONTROL: usize = 0x54;
const SESSION: usize = 0xb4;
pub const USB_MEMORY_BYTES: usize = 0xcc;

/// Owns the USB RAM aperture at 0x0146975c..0x01469828.
pub struct UsbMemory<'a> { bytes: &'a mut [u8; USB_MEMORY_BYTES] }
impl<'a> UsbMemory<'a> {
    pub fn owned(bytes: &'a mut [u8; USB_MEMORY_BYTES]) -> Self { Self { bytes } }
    /// # Safety
    /// The aperture must be mapped, exclusively owned, and inaccessible to IRQ handlers during mutation.
    pub unsafe fn native() -> Self {
        Self { bytes: unsafe { &mut *(0x0146_975cusize as *mut [u8; USB_MEMORY_BYTES]) } }
    }
    fn byte(&self, offset: usize) -> u8 { unsafe { core::ptr::read_volatile(self.bytes.as_ptr().add(offset)) } }
    fn put_byte(&mut self, offset: usize, value: u8) { unsafe { core::ptr::write_volatile(self.bytes.as_mut_ptr().add(offset), value); } }
    fn word(&self, offset: usize) -> u32 {
        u32::from_le_bytes([self.byte(offset), self.byte(offset + 1), self.byte(offset + 2), self.byte(offset + 3)])
    }
    fn put(&mut self, offset: usize, value: u32) {
        for (index, byte) in value.to_le_bytes().iter().enumerate() { self.put_byte(offset + index, *byte); }
    }
    fn half(&self, offset: usize) -> u16 { u16::from_le_bytes([self.byte(offset), self.byte(offset + 1)]) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionMode(u32);
impl SessionMode {
    pub const fn from_abi(value: u32) -> Self { Self(value) }
    pub const fn as_abi(self) -> u32 { self.0 }
}

/// The callback table contains eight guest-address words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackTable(transport::RegionSpan);
impl CallbackTable {
    pub fn published(region: transport::PublishedRegion, offset: u32) -> Result<Self, TransportError> {
        let span = region.span(offset, 32)?;
        if span.address() & 3 != 0 { return Err(TransportError::OutsideRegion); }
        Ok(Self(span))
    }
    pub const fn address(self) -> u32 { self.0.address() }
}

pub struct TransferBuffer<'a> { aperture: transport::PublishedAperture<'a> }
impl<'a> TransferBuffer<'a> {
    pub fn owned(region: transport::PublishedRegion, bytes: &'a mut [u8]) -> Result<Self, TransportError> {
        Ok(Self { aperture: transport::PublishedAperture::publish(region, bytes)? })
    }
    /// # Safety
    /// The region must name exclusively owned, writable mapped RAM for the returned lifetime.
    pub unsafe fn native(region: transport::PublishedRegion) -> Result<Self, TransportError> {
        let bytes = unsafe { core::slice::from_raw_parts_mut(region.base() as usize as *mut u8, region.length() as usize) };
        Self::owned(region, bytes)
    }
    pub fn aperture(&self) -> &transport::PublishedAperture<'a> { &self.aperture }
    pub fn aperture_mut(&mut self) -> &mut transport::PublishedAperture<'a> { &mut self.aperture }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartRegister { ResetCommand, LinkStatus, InitialMaskLow, InitialMaskHigh, InitialClear, EndpointControl, PendingLow, PendingMiddle, PendingHigh }
impl StartRegister {
    pub const fn address(self) -> usize {
        match self {
            Self::ResetCommand => 0xfff55100, Self::LinkStatus => 0xfff55104,
            Self::InitialMaskLow => 0xfff54094, Self::InitialMaskHigh => 0xfff54098,
            Self::InitialClear => 0xfff540ac, Self::EndpointControl => 0xfff54004,
            Self::PendingLow => 0xfff5402c, Self::PendingMiddle => 0xfff54030, Self::PendingHigh => 0xfff54034,
        }
    }
}
/// Stock route selector 0x70b, slot 30: level 11 or 0.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockLevel { Active, Idle }
pub trait StartIo: ControllerIo {
    fn start_read32(&mut self, register: StartRegister) -> u32;
    fn start_read16(&mut self, register: StartRegister) -> u16;
    fn start_write32(&mut self, register: StartRegister, value: u32);
    fn start_write16(&mut self, register: StartRegister, value: u16);
    fn request_clock(&mut self, level: ClockLevel) -> transport::ServiceStatus;
}

pub struct MappedUsbIo { controller: transport::MmioController }
impl MappedUsbIo {
    /// # Safety
    /// The USB MMIO registers must be mapped and exclusively owned. The canonical Power route constructor must have completed; its RAM, source table, and clock MMIO must be mapped.
    pub unsafe fn new() -> Self { Self { controller: unsafe { transport::MmioController::claim() } } }
}
impl ControllerIo for MappedUsbIo {
    fn read32(&mut self, register: Register) -> u32 { self.controller.read32(register) }
    fn read16(&mut self, register: Register) -> u16 { self.controller.read16(register) }
    fn write32(&mut self, register: Register, value: u32) { self.controller.write32(register, value); }
    fn write16(&mut self, register: Register, value: u16) { self.controller.write16(register, value); }
}
impl StartIo for MappedUsbIo {
    fn start_read32(&mut self, register: StartRegister) -> u32 { unsafe { core::ptr::read_volatile(register.address() as *const u32) } }
    fn start_read16(&mut self, register: StartRegister) -> u16 { unsafe { core::ptr::read_volatile(register.address() as *const u16) } }
    fn start_write32(&mut self, register: StartRegister, value: u32) { unsafe { core::ptr::write_volatile(register.address() as *mut u32, value); } }
    fn start_write16(&mut self, register: StartRegister, value: u16) { unsafe { core::ptr::write_volatile(register.address() as *mut u16, value); } }
    fn request_clock(&mut self, level: ClockLevel) -> transport::ServiceStatus {
        extern "C" { fn xm1_power_resource_route_request(selector: u32, slot: u32, level: u32) -> i32; }
        let level = match level { ClockLevel::Active => 11, ClockLevel::Idle => 0 };
        transport::ServiceStatus(unsafe { xm1_power_resource_route_request(0x70b, 30, level) })
    }
}

pub struct StartRuntime<'a, 'irq, B, I, T> {
    pub io: I,
    pub irq: &'a mut cpu::IrqContext<'irq, B>,
    pub timer: T,
    pub handler: irq_control::Handler,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartError { Transport(TransportError), NotStarting }
impl From<TransportError> for StartError { fn from(value: TransportError) -> Self { Self::Transport(value) } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchedulerTicks(u32);
impl SchedulerTicks { pub const fn get(self) -> u32 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartProgress { WaitTicks(SchedulerTicks), Disconnected, Ready }
#[derive(Clone, Copy)]
struct Debounce { previous: bool, equal_samples: u32 }

pub struct UsbStart<'a> { memory: UsbMemory<'a>, debounce: Option<Debounce> }
impl<'a> UsbStart<'a> {
    pub fn new(memory: UsbMemory<'a>) -> Self { Self { memory, debounce: None } }

    fn clear_transfer_state(&mut self) {
        for offset in [0, 4, 8, 0x3c, 0x40, 0x44, 0x48, 0x4c, 0xa0, 0xa4, 0xa8, 0xac, 0xb0] { self.memory.put(offset, 0); }
        for offset in [0, 4, 12, 16, 20, 32] { self.memory.put(CONTROL + offset, 0); }
        self.memory.put_byte(CONTROL + 8, 0);
        self.memory.put_byte(CONTROL + 9, 0);
        self.memory.put(CONTROLLER + 0x10, 0x8181_0303);
        for (index, byte) in [0xe8, 0xdf, 0xfb, 0x7f, 0xbd, 0xaf, 0, 0].iter().enumerate() {
            self.memory.put_byte(CONTROLLER + 0x18 + index, *byte);
        }
    }
    pub fn reset_session(&mut self) {
        for offset in [4, 5, 20, 21] { self.memory.put_byte(SESSION + offset, 0); }
        for offset in [8, 12, 16] { self.memory.put(SESSION + offset, 0); }
    }
    pub fn set_session(&mut self, mode: SessionMode) { self.memory.put(SESSION, mode.as_abi()); }
    pub fn session(&self) -> SessionMode { SessionMode(self.memory.word(SESSION)) }
    pub fn clear_active(&mut self) { self.memory.put(CONTROLLER + 0x20, 0); }

    /// The pending word 0xfff5402c/30/34 minus the shadow mask at 0x01469768+0x18.
    pub fn pending_status(&self, io: &mut impl StartIo) -> crate::transport_pump::UsbStatus {
        let pending = u64::from(io.start_read16(StartRegister::PendingLow))
            | (u64::from(io.start_read16(StartRegister::PendingMiddle)) << 16)
            | (u64::from(io.start_read16(StartRegister::PendingHigh)) << 32);
        let mask = u64::from(self.memory.half(CONTROLLER + 0x18))
            | (u64::from(self.memory.half(CONTROLLER + 0x1a)) << 16)
            | (u64::from(self.memory.half(CONTROLLER + 0x1c)) << 32);
        crate::transport_pump::UsbStatus::new(pending, mask)
    }
    /// 0x00d7d160: the pump returns through 0x00d7d174 when this word is one.
    pub fn pump_stopping(&self) -> bool { self.memory.word(CONTROLLER + 0x24) == 1 }
    /// 0x00d7dc58: a zero bit in these halfwords clears its pending bit.
    pub fn acknowledge(&mut self, io: &mut impl ControllerIo, acknowledge: crate::transport_pump::Acknowledge) {
        io.write16(Register::OutMaskLow, acknowledge.low);
        io.write16(Register::OutMaskMiddle, acknowledge.middle);
        io.write16(Register::OutMaskHigh, acknowledge.high);
    }
    /// 0x00d7dc90 restores the interrupt block and the shadow masks only while active.
    pub fn restore_event_masks(&self, io: &mut impl ControllerIo) {
        if self.memory.word(CONTROLLER + 0x20) != 1 { return; }
        io.write32(Register::InterruptReset, self.memory.word(CONTROLLER + 0x10));
        self.masks(io);
    }
    pub fn set_event_mask(&mut self, mask: [u16; 3]) {
        for (index, value) in mask.iter().enumerate() {
            self.memory.put_byte(CONTROLLER + 0x18 + index * 2, *value as u8);
            self.memory.put_byte(CONTROLLER + 0x19 + index * 2, (*value >> 8) as u8);
        }
    }
    /// 0x00d7ae94: clears the control and channel blocks, the interrupt block and the mask.
    pub fn reset_transfer_state(&mut self) {
        for offset in [0, 4, 8, 0x3c, 0x40, 0x44, 0x48, 0x4c, 0xa0, 0xa4, 0xa8, 0xac, 0xb0] { self.memory.put(offset, 0); }
        for offset in [0, 4, 12, 16, 20, 32] { self.memory.put(CONTROL + offset, 0); }
        self.memory.put_byte(CONTROL + 8, 0);
        self.memory.put_byte(CONTROL + 9, 0);
        self.memory.put(CONTROLLER + 0x10, 0x8181_0303);
        let mut mask = [0xffffu16; 3];
        for event in crate::transport_pump::UsbEvent::ORDER {
            let bit = event.bit();
            if bit == 3 { continue; }
            let word = match bit { 0..=15 => &mut mask[0], 16..=31 => &mut mask[1], _ => &mut mask[2] };
            *word &= !(1u16 << (bit % 16));
        }
        self.set_event_mask(mask);
    }

    fn masks(&self, io: &mut impl ControllerIo) {
        io.write16(Register::EventClearLow, self.memory.half(CONTROLLER + 0x18));
        io.write16(Register::EventClearMiddle, self.memory.half(CONTROLLER + 0x1a));
        io.write16(Register::EventClearHigh, self.memory.half(CONTROLLER + 0x1c));
    }
    pub fn bind_callbacks(&mut self, table: CallbackTable, io: &mut impl ControllerIo) {
        self.memory.put(CONTROLLER + 12, table.address());
        self.memory.put_byte(CONTROLLER + 0x18, self.memory.byte(CONTROLLER + 0x18) & !4);
        self.memory.put_byte(CONTROLLER + 0x19, self.memory.byte(CONTROLLER + 0x19) & !0x20);
        self.masks(io);
    }

    pub fn initialize<B: irq_control::Backend, I: StartIo, T: cpu::DelayCounter>(
        &mut self, runtime: &mut StartRuntime<'_, '_, B, I, T>,
    ) -> Result<(), StartError> {
        let _ = runtime.io.request_clock(ClockLevel::Active);
        cpu::spin_delay(&mut runtime.timer, 150)?;
        self.clear_transfer_state();
        self.memory.put(CONTROLLER + 0x20, 1);
        self.memory.put(CONTROLLER + 0x24, 0);
        self.memory.put(CONTROLLER + 0x28, 0);
        self.memory.put(CONTROL + 0x18, 0);
        self.memory.put(CONTROL + 0x1c, 0);
        if self.memory.word(0x50) != 0 {
            let control = runtime.io.read32(Register::OutControl) | 0x10000;
            runtime.io.write32(Register::OutControl, control);
            cpu::spin_delay(&mut runtime.timer, 150)?;
            runtime.io.write32(Register::OutControl, control & !0x10000);
        }
        runtime.io.start_write32(StartRegister::ResetCommand, 1);
        cpu::spin_delay(&mut runtime.timer, 1)?;
        runtime.io.start_write32(StartRegister::LinkStatus, 0x1ecbc7);
        cpu::spin_delay(&mut runtime.timer, 150)?;
        runtime.io.write16(Register::ControllerEnable, 1);
        cpu::spin_delay(&mut runtime.timer, 1)?;
        runtime.io.start_write16(StartRegister::InitialClear, 0x3fff);
        let low = runtime.io.start_read16(StartRegister::InitialMaskLow) | 0xc0;
        let high = runtime.io.start_read16(StartRegister::InitialMaskHigh) | 1;
        runtime.io.start_write16(StartRegister::InitialMaskLow, low);
        runtime.io.start_write16(StartRegister::InitialMaskHigh, high);
        if runtime.io.start_read32(StartRegister::LinkStatus) & 0x8000_0000 == 0 {
            if self.memory.word(CONTROLLER + 0x28) != 1 {
                let disconnect = runtime.io.read16(Register::ControllerDisconnect);
                runtime.io.write16(Register::ControllerDisconnect, disconnect | 2);
                self.memory.put(CONTROLLER + 0x28, 1);
            }
            let _ = runtime.io.request_clock(ClockLevel::Idle);
        }
        self.memory.put(0x50, self.memory.word(0x50).wrapping_add(1));
        Ok(())
    }

    pub fn begin_start(&mut self) {
        self.clear_transfer_state();
        self.memory.put(CONTROLLER + 0x20, 1);
        self.memory.put(CONTROLLER + 0x24, 0);
        self.debounce = Some(Debounce { previous: false, equal_samples: 0 });
    }
    pub fn start<B: irq_control::Backend, I: StartIo, T: cpu::DelayCounter, M: transport::scheduler::Machine>(
        &mut self,
        runtime: &mut StartRuntime<'_, '_, B, I, T>,
        machine: &M,
        domain: transport::scheduler::layout::Domain,
    ) -> Result<StartProgress, StartError> {
        self.begin_start();
        loop {
            match self.poll_start(runtime)? {
                StartProgress::WaitTicks(ticks) => {
                    let _ = transport::scheduler::delay(machine, domain, transport::scheduler::Ticks(ticks.get() as i32));
                }
                result => return Ok(result),
            }
        }
    }

    /// Call again only after the returned scheduler delay has completed.
    pub fn poll_start<B: irq_control::Backend, I: StartIo, T: cpu::DelayCounter>(
        &mut self, runtime: &mut StartRuntime<'_, '_, B, I, T>,
    ) -> Result<StartProgress, StartError> {
        let debounce = self.debounce.as_mut().ok_or(StartError::NotStarting)?;
        let connected = runtime.io.start_read32(StartRegister::LinkStatus) & 0x8000_0000 != 0;
        if connected == debounce.previous {
            debounce.equal_samples += 1;
            if debounce.equal_samples <= 99 { return Ok(StartProgress::WaitTicks(SchedulerTicks(10))); }
        } else if !debounce.previous {
            debounce.previous = true;
            debounce.equal_samples = 0;
            return Ok(StartProgress::WaitTicks(SchedulerTicks(10)));
        } else {
            self.debounce = None;
            return Ok(StartProgress::Disconnected);
        }
        self.debounce = None;
        if !connected { return Ok(StartProgress::Disconnected); }
        #[cfg(target_arch = "arm")]
        let saved = unsafe {
            extern "C" { fn xm1_power_interrupt_lock() -> u32; }
            xm1_power_interrupt_lock()
        };
        #[cfg(not(target_arch = "arm"))]
        let saved = runtime.irq.backend.mask_irq_fiq();
        let pulse = if self.memory.word(CONTROLLER + 0x28) == 1 {
            let control = runtime.io.read32(Register::OutControl) | 0x10000;
            runtime.io.write32(Register::OutControl, control);
            let result = (0..20).try_for_each(|_| cpu::spin_delay(&mut runtime.timer, 250));
            runtime.io.write32(Register::OutControl, control & !0x10000);
            result
        } else { Ok(()) };
        self.memory.put(CONTROLLER + 0x28, 0);
        #[cfg(target_arch = "arm")]
        unsafe {
            extern "C" { fn xm1_power_interrupt_unlock(saved: u32); }
            xm1_power_interrupt_unlock(saved);
        }
        #[cfg(not(target_arch = "arm"))]
        runtime.irq.backend.restore_irq_fiq(saved);
        pulse?;
        if runtime.io.start_read32(StartRegister::LinkStatus) & 0x8000_0000 == 0 { return Ok(StartProgress::Disconnected); }
        let low = runtime.io.start_read16(StartRegister::PendingLow);
        let middle = runtime.io.start_read16(StartRegister::PendingMiddle);
        let high = runtime.io.start_read16(StartRegister::PendingHigh);
        runtime.io.write16(Register::OutMaskLow, !low);
        runtime.io.write16(Register::OutMaskMiddle, !middle);
        runtime.io.write16(Register::OutMaskHigh, !high);
        if runtime.io.start_read32(StartRegister::LinkStatus) & 0x8000_0000 == 0 { return Ok(StartProgress::Disconnected); }
        self.masks(&mut runtime.io);
        runtime.io.write32(Register::InterruptReset, self.memory.word(CONTROLLER + 0x10));
        runtime.io.write32(Register::OutInitialize, 0x101);
        cpu::spin_delay(&mut runtime.timer, 1)?;
        let control = runtime.io.start_read16(StartRegister::EndpointControl);
        runtime.io.start_write16(StartRegister::EndpointControl, control | 4);
        let control = runtime.io.start_read16(StartRegister::EndpointControl);
        runtime.io.start_write16(StartRegister::EndpointControl, control | 8);
        runtime.io.write16(Register::InMaskLow, 0x10d);
        runtime.io.write16(Register::InMaskMiddle, 0x2083);
        runtime.io.write16(Register::InMaskHigh, 0);
        runtime.io.write16(Register::EndpointPulse, 0xfff);
        self.register_irq(runtime)?;
        let enable = runtime.io.read16(Register::ControllerEnable);
        runtime.io.write16(Register::ControllerEnable, enable | 6);
        Ok(StartProgress::Ready)
    }

    fn register_irq<B: irq_control::Backend, I, T>(&mut self, runtime: &mut StartRuntime<'_, '_, B, I, T>) -> Result<(), StartError> {
        let line = irq_control::InterruptLine::from_static(84);
        let handle = runtime.irq.handles.allocate(84, runtime.irq.sequence)?;
        let registration = irq_control::Registration::new(runtime.handler.get(), 13, 0, 1).ok_or(TransportError::InvalidChannel)?;
        irq_control::configure(runtime.irq.backend, runtime.irq.table, line, registration);
        irq_control::enable(runtime.irq.backend, runtime.irq.table, line).map_err(|_| TransportError::InvalidChannel)?;
        runtime.irq.sources[84] = 15;
        self.memory.put(CONTROLLER, handle.get());
        Ok(())
    }

    pub fn controller_irq(&self) -> Option<cpu::DriverHandle> { cpu::DriverHandle::from_raw(self.memory.word(CONTROLLER)) }

    /// Stock 0x00d7c790 first releases the interrupt handle at 0x01469768 and zeroes it.
    pub fn release_irq<B: irq_control::Backend, I, T>(&mut self, runtime: &mut StartRuntime<'_, '_, B, I, T>) {
        if let Some(handle) = self.controller_irq() { let _ = cpu::release_interrupt_handle(runtime.irq, handle); }
        self.memory.put(CONTROLLER, 0);
    }
    /// The channel and reset part of the session teardown; the reset flag is the word at 0x01469790.
    pub fn teardown(&self, channels: [transport::ChannelLease; 2]) -> transport::ControllerTeardown {
        transport::ControllerTeardown::new(None, channels, self.memory.word(CONTROLLER + 0x28) == 1)
    }
    pub fn finish_teardown(&mut self) { self.memory.put(CONTROLLER + 0x28, 1); }

    pub fn finalize<B: irq_control::Backend, I: ControllerIo, T>(&mut self, runtime: &mut StartRuntime<'_, '_, B, I, T>) -> Result<(), StartError> {
        if self.memory.word(CONTROLLER + 0x20) == 1 { return Ok(()); }
        if let Some(handle) = self.controller_irq() {
            // Stock rearm ignores the release status.
            let _ = cpu::release_interrupt_handle(runtime.irq, handle);
        }
        runtime.io.write32(Register::InterruptReset, self.memory.word(CONTROLLER + 0x10));
        self.masks(&mut runtime.io);
        self.memory.put(CONTROLLER + 0x20, 1);
        self.register_irq(runtime)
    }
}

/// Stock 0x00d814f4 route vote for a connection-bit change.
pub fn vote_connection(io: &mut impl StartIo, connected: bool) -> transport::ServiceStatus {
    io.request_clock(if connected { ClockLevel::Active } else { ClockLevel::Idle })
}

pub fn link_connected(io: &mut impl StartIo) -> bool {
    io.start_read32(StartRegister::LinkStatus) & 0x8000_0000 != 0
}

pub fn bus_status_bit14(io: &mut impl StartIo) -> bool {
    link_connected(io) && io.read16(Register::BusSpeed) & 0x4000 != 0
}

/// Stock 0x00d7b8e0 packet size: 512 with bus-speed bit 14, else 64.
pub fn bulk_packet_size(io: &mut impl ControllerIo) -> u16 {
    if io.read16(Register::BusSpeed) & 0x4000 != 0 { 512 } else { 64 }
}
