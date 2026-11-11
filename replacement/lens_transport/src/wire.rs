//! X-M1 1.50 serial descriptor 13 and timer descriptor 39.
//! Register policy follows 0x0064a65c, 0x0064c61c, 0x0064fd68 and their callees.

use core::num::NonZeroU32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferToken(NonZeroU32);
impl TransferToken {
    pub const fn new(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) { Some(value) => Some(Self(value)), None => None }
    }
    pub const fn get(self) -> u32 { self.0.get() }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimerToken(NonZeroU32);
impl TimerToken {
    pub const fn new(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) { Some(value) => Some(Self(value)), None => None }
    }
    pub const fn get(self) -> u32 { self.0.get() }
}

/// Milliseconds, the unit selected by the lens timer requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimerTicks(u32);
impl TimerTicks {
    pub const fn new(value: u32) -> Option<Self> {
        if value != 0 && value <= 349_519 { Some(Self(value)) } else { None }
    }
    pub const fn get(self) -> u32 { self.0 }
}

pub trait Backend {
    type Error;
    fn register(&mut self) -> Result<(), Self::Error>;
    fn start(&mut self, token: TransferToken, transmit: &[u8]) -> Result<(), Self::Error>;
    fn arm_timer(&mut self, token: TimerToken, delay: TimerTicks) -> Result<(), Self::Error>;
    fn cancel_timer(&mut self) -> Result<(), Self::Error>;
    fn abort_wire(&mut self) -> Result<(), Self::Error>;
    fn unregister(&mut self) -> Result<(), Self::Error>;
    fn set_requested_output(&mut self, active: bool) -> Result<(), Self::Error>;
    fn diagnostic_halt(&mut self) -> !;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Address(u32);
impl Address {
    pub const fn get(self) -> u32 { self.0 }
    const fn offset(self, offset: u32) -> Self { Self(self.0 + offset) }
}

pub trait Mmio {
    type SavedInterrupts;
    fn read_word(&self, address: Address) -> u32;
    fn write_word(&self, address: Address, value: u32);
    fn read_byte(&self, address: Address) -> u8;
    fn write_byte(&self, address: Address, value: u8);
    fn mask_interrupts(&self) -> Self::SavedInterrupts;
    fn restore_interrupts(&self, saved: Self::SavedInterrupts);
    fn publish_dma(&self);
}

/// Exclusive lease of serial 2, DMA 2 and 3, timer 11 and both scratch windows.
#[derive(Debug)]
pub struct LensEndpoints(());
impl LensEndpoints {
    pub const fn new() -> Self { Self(()) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestedOutput { Inactive, Active }

pub trait Platform {
    type Error;
    fn acquire(&mut self, endpoints: LensEndpoints) -> Result<(), Self::Error>;
    fn release(&mut self, endpoints: LensEndpoints) -> Result<(), Self::Error>;
    /// Requester 17 participates in the shared output-level arbitration.
    fn request_output(&mut self, level: RequestedOutput) -> Result<(), Self::Error>;
}

/// IRQ key 0x73 resolves to this controller registration in image 4.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimerInterrupt;
impl TimerInterrupt {
    pub const fn line(self) -> u32 { 31 }
    pub const fn priority(self) -> u32 { 7 }
    pub const fn mode(self) -> u32 { 3 }
}

/// The adapter must use the owned IRQ controller CPU, not a stock driver call.
pub trait InterruptController {
    type Error;
    fn register_wire(&mut self, token: TransferToken) -> Result<(), Self::Error>;
    fn unregister_wire(&mut self) -> Result<(), Self::Error>;
    fn register_timer(&mut self, interrupt: TimerInterrupt, token: TimerToken) -> Result<(), Self::Error>;
    fn unregister_timer(&mut self, interrupt: TimerInterrupt) -> Result<(), Self::Error>;
}

#[derive(Debug, Eq, PartialEq)]
pub enum Error<P, I> {
    Platform(P),
    Interrupt(I),
    NotRegistered,
    AlreadyRegistered,
    Busy,
    InvalidLength,
    StaleToken,
    DmaBusy,
}

const SERIAL: Address = Address(0xfff8_4080);
const TIMER: Address = Address(0xfffa_9c00);
const TIMER_ENABLE: Address = Address(0xfffa_9000);
const TX: Address = Address(0x4096_d000);
const RX: Address = Address(0x4096_d820);
const TX_DMA: Address = Address(0xfff0_2080);
const RX_DMA: Address = Address(0xfff0_20c0);
const DMA_REQUESTS: Address = Address(0xfff9_8500);
const TRANSFER_IRQS: u32 = 0x0909_0000;
pub const MAX_TRANSFER_BYTES: usize = 0x820;

#[derive(Clone, Copy)]
struct Transfer {
    token: TransferToken,
    length: usize,
}

pub struct CpuBackend<M, P, I> {
    mmio: M,
    platform: P,
    interrupts: I,
    registered: bool,
    transfer: Option<Transfer>,
    timer: Option<TimerToken>,
}

impl<M: Mmio, P: Platform, I: InterruptController> CpuBackend<M, P, I> {
    pub const fn new(mmio: M, platform: P, interrupts: I) -> Self {
        Self { mmio, platform, interrupts, registered: false, transfer: None, timer: None }
    }

    pub fn prepare_detection_output(&mut self) {
        self.mmio.write_word(Address(0xfff8_9004), 0x0080_0000);
    }

    pub fn initialize_detection_output(&mut self) {
        self.prepare_detection_output();
        self.mmio.write_word(Address(0xfff8_9000), 0x0080_0000);
    }

    fn require_registered(&self) -> Result<(), Error<P::Error, I::Error>> {
        if self.registered { Ok(()) } else { Err(Error::NotRegistered) }
    }

    fn update(&self, address: Address, clear: u32, set: u32) {
        let saved = self.mmio.mask_interrupts();
        self.mmio.write_word(address, (self.mmio.read_word(address) & !clear) | set);
        self.mmio.restore_interrupts(saved);
    }

    fn configure_serial(&self) {
        for bit in [0x1000_0000, 0x1_0000, 0x8_0000, 0x100_0000, 0x800_0000,
                    1, 0x10, 0x100, 0x200, 0x1000, 0x2000] {
            self.update(SERIAL.offset(8), bit, 0);
        }
        self.update(SERIAL, 0, 0x40);
        self.update(SERIAL, 0x10, 0);
        self.update(SERIAL, 0x20, 0);
        self.update(SERIAL, 0xf_0000, 0);
        self.update(SERIAL.offset(4), 0x7ffe, 8);
        self.update(SERIAL.offset(4), 0x1_0000, 0);
        self.update(SERIAL.offset(4), 0x2_0000, 0);
        self.update(SERIAL.offset(4), 0x8000, 0);
        self.update(SERIAL.offset(8), 0, 0x1000_0000);
        self.update(SERIAL.offset(8), 0x1000_0000, 0);
        for bit in [1, 0x10, 0x100, 0x200, 0x1000, 0x2000] {
            self.update(SERIAL.offset(12), 0, bit);
        }
        self.update(SERIAL.offset(16), 0x1f, 1);
        self.update(SERIAL.offset(20), 0x1f, 1);
        self.update(SERIAL, 0, 0x80);
    }

    fn configure_dma(&self, base: Address, transmit: bool, length: u32) {
        let (configuration, request, shift, source, destination) = if transmit {
            (0x0320_016a, 9, 16, TX, SERIAL.offset(28))
        } else {
            (0x0210_0163, 8, 24, SERIAL.offset(24), RX)
        };
        self.update(DMA_REQUESTS, 0x1f << shift, request << shift);
        self.mmio.write_word(base.offset(44), configuration);
        self.mmio.write_word(base, source.get());
        self.mmio.write_word(base.offset(4), destination.get());
        self.mmio.write_word(base.offset(8), length);
        for offset in [12, 16, 20, 48, 52] {
            self.mmio.write_word(base.offset(offset), 0);
        }
    }

    fn start_dma(&self, base: Address) {
        self.mmio.write_word(base.offset(40), 8);
        let software_request = if self.mmio.read_word(base.offset(44)) & 0x30 == 0 { 4 } else { 0 };
        self.mmio.publish_dma();
        self.mmio.write_word(base.offset(40), software_request | 1);
    }

    fn finish_dma(&self, base: Address) {
        while self.mmio.read_word(base.offset(36)) & 1 != 0 {
            core::hint::spin_loop();
        }
    }

    fn stop_dma(&self, base: Address) {
        self.update(base.offset(40), 0, 0x100);
        loop {
            let status = self.mmio.read_word(base.offset(36));
            if status & 1 == 0 { break; }
            if status & 8 != 0 {
                self.update(base.offset(40), 0, 2);
                while self.mmio.read_word(base.offset(36)) & 4 != 0 {
                    core::hint::spin_loop();
                }
                break;
            }
            core::hint::spin_loop();
        }
        self.update(base.offset(40), 0, 8);
    }

    /// Call only after the physical serial/DMA completion has been observed.
    pub fn complete_wire(&mut self, token: TransferToken, received: &mut [u8]) -> Result<usize, Error<P::Error, I::Error>> {
        self.require_registered()?;
        let transfer = self.transfer.ok_or(Error::StaleToken)?;
        if transfer.token != token { return Err(Error::StaleToken); }
        if received.len() < transfer.length { return Err(Error::InvalidLength); }
        for bit in [0x100_0000, 0x800_0000, 0x1_0000, 0x8_0000] {
            self.update(SERIAL.offset(8), bit, 0);
        }
        self.finish_dma(RX_DMA);
        self.finish_dma(TX_DMA);
        self.interrupts.unregister_wire().map_err(Error::Interrupt)?;
        for (index, byte) in received[..transfer.length].iter_mut().enumerate() {
            *byte = self.mmio.read_byte(RX.offset(index as u32));
        }
        self.transfer = None;
        Ok(transfer.length)
    }

    /// The interrupt trampoline supplies the token captured at registration.
    pub fn complete_timer(&mut self, token: TimerToken) -> Result<(), Error<P::Error, I::Error>> {
        self.require_registered()?;
        if self.timer != Some(token) { return Err(Error::StaleToken); }
        self.cancel_timer()
    }
}

impl<M: Mmio, P: Platform, I: InterruptController> Backend for CpuBackend<M, P, I> {
    type Error = Error<P::Error, I::Error>;

    fn register(&mut self) -> Result<(), Self::Error> {
        if self.registered { return Err(Error::AlreadyRegistered); }
        self.platform.acquire(LensEndpoints::new()).map_err(Error::Platform)?;
        self.registered = true;
        self.configure_serial();
        self.mmio.write_word(Address(0xfff8_9350), 0x0003_0001);
        self.mmio.write_word(Address(0xfff8_934c), 0x0030_0010);
        Ok(())
    }

    fn start(&mut self, token: TransferToken, transmit: &[u8]) -> Result<(), Self::Error> {
        self.require_registered()?;
        if self.transfer.is_some() { return Err(Error::Busy); }
        if transmit.is_empty() || transmit.len() > MAX_TRANSFER_BYTES { return Err(Error::InvalidLength); }
        if self.mmio.read_word(TX_DMA.offset(36)) & 5 != 0 || self.mmio.read_word(RX_DMA.offset(36)) & 5 != 0 {
            return Err(Error::DmaBusy);
        }
        for (index, byte) in transmit.iter().enumerate() {
            self.mmio.write_byte(TX.offset(index as u32), *byte);
        }
        self.update(SERIAL, 0x80, 0);
        self.update(SERIAL.offset(12), 0xffff, 0x3311);
        self.update(SERIAL.offset(32), 0x700, 0x400);
        self.update(SERIAL.offset(32), 7, 0);
        for bit in [0x100_0000, 0x800_0000, 0x1_0000, 0x8_0000] {
            self.update(SERIAL.offset(8), 0, bit);
        }
        self.configure_dma(TX_DMA, true, transmit.len() as u32);
        self.configure_dma(RX_DMA, false, transmit.len() as u32);
        self.interrupts.register_wire(token).map_err(Error::Interrupt)?;
        self.transfer = Some(Transfer { token, length: transmit.len() });
        self.start_dma(TX_DMA);
        self.start_dma(RX_DMA);
        self.update(SERIAL, 0, 0x80);
        Ok(())
    }

    fn arm_timer(&mut self, token: TimerToken, delay: TimerTicks) -> Result<(), Self::Error> {
        self.require_registered()?;
        if self.timer.is_some() { return Err(Error::Busy); }
        let scaled = u64::from(delay.get()) * 1000;
        let (prescaler, rate) = if scaled <= 2_730_624 { (0, 240_000) }
            else if scaled <= 21_844_999 { (1, 30_000) }
            else if scaled <= 87_379_999 { (2, 7_500) }
            else { (3, 1_875) };
        let count = (u64::from(delay.get()) * rate / 10_000) as u16;
        self.mmio.write_word(TIMER, 0x8000 | prescaler);
        self.update(TIMER.offset(4), 1, 0);
        self.mmio.write_word(TIMER.offset(8), u32::from(count));
        self.mmio.write_word(TIMER.offset(12), u32::from(count.wrapping_sub(1)));
        self.mmio.write_word(TIMER.offset(16), 0);
        self.interrupts.register_timer(TimerInterrupt, token).map_err(Error::Interrupt)?;
        self.timer = Some(token);
        self.mmio.write_word(TIMER_ENABLE, (1 << 27) | (1 << 11));
        Ok(())
    }

    fn cancel_timer(&mut self) -> Result<(), Self::Error> {
        self.require_registered()?;
        if self.timer.is_some() {
            self.mmio.write_word(TIMER_ENABLE, 1 << 27);
            self.interrupts.unregister_timer(TimerInterrupt).map_err(Error::Interrupt)?;
            self.timer = None;
        }
        Ok(())
    }

    fn abort_wire(&mut self) -> Result<(), Self::Error> {
        self.require_registered()?;
        self.update(SERIAL, 0x80, 0);
        while self.mmio.read_word(SERIAL) & 1 != 0 { core::hint::spin_loop(); }
        if self.transfer.is_some() {
            self.stop_dma(TX_DMA);
            self.stop_dma(RX_DMA);
            self.interrupts.unregister_wire().map_err(Error::Interrupt)?;
            self.transfer = None;
        }
        Ok(())
    }

    fn unregister(&mut self) -> Result<(), Self::Error> {
        self.require_registered()?;
        self.cancel_timer()?;
        self.abort_wire()?;
        self.update(SERIAL.offset(8), TRANSFER_IRQS, 0);
        self.platform.release(LensEndpoints::new()).map_err(Error::Platform)?;
        self.registered = false;
        Ok(())
    }

    fn set_requested_output(&mut self, active: bool) -> Result<(), Self::Error> {
        self.platform.request_output(if active { RequestedOutput::Active } else { RequestedOutput::Inactive }).map_err(Error::Platform)
    }

    fn diagnostic_halt(&mut self) -> ! {
        loop {
            for level in [0x0001_0001, 0x0001_0000] {
                self.mmio.write_word(Address(0xfff8_93c0), level);
                for _ in 0..1000 {
                    let reload = self.mmio.read_word(Address(0xfff9_1004));
                    let required = 100_000 / (10_000_000 / reload);
                    let mut previous = reload.wrapping_sub(self.mmio.read_word(Address(0xfff9_1000)));
                    let mut elapsed = 0u32;
                    loop {
                        let current = self.mmio.read_word(Address(0xfff9_1004))
                            .wrapping_sub(self.mmio.read_word(Address(0xfff9_1000)));
                        let delta = if previous >= current { previous - current }
                            else { reload.wrapping_add(previous).wrapping_add(1).wrapping_sub(current) };
                        elapsed = elapsed.wrapping_add(delta);
                        if elapsed >= required { break; }
                        previous = current;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use core::cell::RefCell;
    use self::std::collections::BTreeMap;

    #[derive(Default)]
    struct Memory(RefCell<BTreeMap<u32, u8>>);
    impl Mmio for &Memory {
        type SavedInterrupts = ();
        fn read_word(&self, address: Address) -> u32 {
            let bytes = self.0.borrow();
            u32::from_le_bytes(core::array::from_fn(|index| *bytes.get(&(address.get() + index as u32)).unwrap_or(&0)))
        }
        fn write_word(&self, address: Address, value: u32) {
            for (index, byte) in value.to_le_bytes().iter().enumerate() {
                self.0.borrow_mut().insert(address.get() + index as u32, *byte);
            }
        }
        fn read_byte(&self, address: Address) -> u8 { *self.0.borrow().get(&address.get()).unwrap_or(&0) }
        fn write_byte(&self, address: Address, value: u8) { self.0.borrow_mut().insert(address.get(), value); }
        fn mask_interrupts(&self) {}
        fn restore_interrupts(&self, _: ()) {}
        fn publish_dma(&self) {}
    }

    struct ExclusivePlatform;
    impl Platform for ExclusivePlatform {
        type Error = ();
        fn acquire(&mut self, _: LensEndpoints) -> Result<(), ()> { Ok(()) }
        fn release(&mut self, _: LensEndpoints) -> Result<(), ()> { Ok(()) }
        fn request_output(&mut self, _: RequestedOutput) -> Result<(), ()> { Ok(()) }
    }

    struct Interrupts;
    impl InterruptController for Interrupts {
        type Error = ();
        fn register_wire(&mut self, _: TransferToken) -> Result<(), ()> { Ok(()) }
        fn unregister_wire(&mut self) -> Result<(), ()> { Ok(()) }
        fn register_timer(&mut self, _: TimerInterrupt, _: TimerToken) -> Result<(), ()> { Ok(()) }
        fn unregister_timer(&mut self, _: TimerInterrupt) -> Result<(), ()> { Ok(()) }
    }

    #[test]
    fn detection_initialization_drives_the_declared_gpio_output() {
        let memory = Memory::default();
        let mut wire = CpuBackend::new(&memory, ExclusivePlatform, Interrupts);
        wire.initialize_detection_output();
        assert_eq!((&memory).read_word(Address(0xfff8_9004)), 0x0080_0000);
        assert_eq!((&memory).read_word(Address(0xfff8_9000)), 0x0080_0000);
    }

    #[test]
    fn duplex_transfer_programs_real_endpoints_and_returns_physical_reply() {
        let memory = Memory::default();
        let mut wire = CpuBackend::new(&memory, ExclusivePlatform, Interrupts);
        let token = TransferToken::new(1).unwrap();
        wire.register().unwrap();
        wire.start(token, &[0xa5, 0x5a, 0x3c, 0xc3]).unwrap();
        assert_eq!((&memory).read_word(Address(0x4096_d000)), 0xc33c_5aa5);
        assert_eq!((&memory).read_word(Address(0xfff0_2080)), 0x4096_d000);
        assert_eq!((&memory).read_word(Address(0xfff0_2084)), 0xfff8_409c);
        assert_eq!((&memory).read_word(Address(0xfff0_2088)), 4);
        assert_eq!((&memory).read_word(Address(0xfff0_20c0)), 0xfff8_4098);
        assert_eq!((&memory).read_word(Address(0xfff0_20c4)), 0x4096_d820);
        assert_eq!((&memory).read_word(Address(0xfff0_20ac)), 0x0320_016a);
        assert_eq!((&memory).read_word(Address(0xfff0_20ec)), 0x0210_0163);
        (&memory).write_word(Address(0x4096_d820), 0x4433_2211);
        let mut reply = [0; 4];
        assert_eq!(wire.complete_wire(token, &mut reply), Ok(4));
        assert_eq!(reply, [0x11, 0x22, 0x33, 0x44]);
        assert_eq!((&memory).read_word(Address(0xfff8_4088)) & 0x0909_0000, 0);
        assert_eq!(wire.complete_wire(token, &mut reply), Err(Error::StaleToken));
    }

    #[test]
    fn timer_prescaler_boundaries_and_cancellation_reach_hardware() {
        for (milliseconds, prescaler, count) in [
            (1, 0, 24), (2730, 0, 65520), (2731, 1, 8193),
            (21844, 1, 65532), (21845, 2, 16383),
            (87379, 2, 65534), (87380, 3, 16383), (349519, 3, 65534),
        ] {
            let memory = Memory::default();
            let mut wire = CpuBackend::new(&memory, ExclusivePlatform, Interrupts);
            wire.register().unwrap();
            let token = TimerToken::new(1).unwrap();
            wire.arm_timer(token, TimerTicks::new(milliseconds).unwrap()).unwrap();
            assert_eq!((&memory).read_word(Address(0xfffa_9c00)), 0x8000 | prescaler);
            assert_eq!((&memory).read_word(Address(0xfffa_9c08)), count);
            assert_eq!((&memory).read_word(Address(0xfffa_9c0c)), count - 1);
            assert_eq!((&memory).read_word(Address(0xfffa_9000)), 0x0800_0800);
            wire.complete_timer(token).unwrap();
            assert_eq!((&memory).read_word(Address(0xfffa_9000)), 0x0800_0000);
            assert_eq!(wire.complete_timer(token), Err(Error::StaleToken));
        }
        assert_eq!(TimerTicks::new(349520), None);
    }

    #[test]
    fn teardown_blocks_late_completions_and_allows_a_new_registration() {
        let memory = Memory::default();
        let mut wire = CpuBackend::new(&memory, ExclusivePlatform, Interrupts);
        wire.register().unwrap();
        let transfer = TransferToken::new(1).unwrap();
        let timer = TimerToken::new(1).unwrap();
        wire.start(transfer, &[0xa5]).unwrap();
        wire.arm_timer(timer, TimerTicks::new(10).unwrap()).unwrap();
        wire.unregister().unwrap();
        assert_eq!((&memory).read_word(Address(0xfff8_4080)) & 0x80, 0);
        assert_eq!((&memory).read_word(Address(0xfff8_4088)) & 0x0909_0000, 0);
        assert_eq!(wire.complete_wire(transfer, &mut [0]), Err(Error::NotRegistered));
        assert_eq!(wire.complete_timer(timer), Err(Error::NotRegistered));
        wire.register().unwrap();
        wire.start(TransferToken::new(2).unwrap(), &[0x5a]).unwrap();
    }
}
