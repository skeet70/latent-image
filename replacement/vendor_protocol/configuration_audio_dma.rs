use super::packets::{PcmAddress, PcmByteLength};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegisterAddress(u32);
impl RegisterAddress { pub const fn get(self) -> u32 { self.0 } }

pub trait AudioRegisters {
    type InterruptMask;
    fn read(&mut self, address: RegisterAddress) -> u32;
    fn write(&mut self, address: RegisterAddress, value: u32);
    fn mask_interrupts(&mut self) -> Self::InterruptMask;
    fn restore_interrupts(&mut self, mask: Self::InterruptMask);
    fn clean_data_cache(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DmaError { InvalidHandle, Busy, NotActive, InvalidParameter, Verification }
impl DmaError {
    pub const fn return_code(self) -> i32 {
        match self { Self::InvalidHandle => -20, Self::Busy => -12, Self::NotActive => -11, Self::InvalidParameter => -30, Self::Verification => -255 }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferLease(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerBank { First, Second }
impl ControllerBank {
    const fn base(self) -> u32 { match self { Self::First => 0xfffa0000, Self::Second => 0xfffa1000 } }
    pub const fn receive_fifo(self) -> PcmAddress { PcmAddress::from_abi(self.base() + 0x20) }
    pub const fn transmit_fifo(self) -> PcmAddress { PcmAddress::from_abi(self.base() + 0x10) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferDirection { Receive, Transmit }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplePacking { Packed8, Packed16, Packed32 }
impl SamplePacking {
    const fn burst(self) -> u32 { match self { Self::Packed8 => 0, Self::Packed16 => 1, Self::Packed32 => 2 } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferProgram {
    control: u32,
    source: PcmAddress,
    destination: PcmAddress,
    length: PcmByteLength,
    alternate_source: PcmAddress,
    alternate_destination: PcmAddress,
    alternate_length: PcmByteLength,
}
impl TransferProgram {
    pub const fn receive(bank: ControllerBank, destination: PcmAddress, length: PcmByteLength, packing: SamplePacking) -> Self {
        Self { control: 0x40120011 | packing.burst() << 12, source: bank.receive_fifo(), destination, length,
            alternate_source: bank.receive_fifo(), alternate_destination: PcmAddress::from_abi(0x93adf8), alternate_length: PcmByteLength::from_abi(2048) }
    }
    pub const fn transmit(bank: ControllerBank, source: PcmAddress, length: PcmByteLength, packing: SamplePacking, padding: PcmAddress, padding_length: PcmByteLength) -> Self {
        Self { control: 0x40220011 | packing.burst() << 12, source, destination: bank.transmit_fifo(), length,
            alternate_source: padding, alternate_destination: bank.transmit_fifo(), alternate_length: padding_length }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransferState { Closed, Open(TransferLease), Active(TransferLease) }

// Audio kind2 maps to FFF020 channel1 at the pinned X-M1 table0x7c86bc.
pub struct AudioDma {
    generation: u32,
    state: TransferState,
}
impl AudioDma {
    pub const fn new() -> Self { Self { generation: 0, state: TransferState::Closed } }
    const fn register(offset: u32) -> RegisterAddress { RegisterAddress(0xfff02040 + offset) }
    fn owns(&self, lease: TransferLease) -> bool {
        matches!(self.state, TransferState::Open(current) | TransferState::Active(current) if current == lease)
    }
    pub fn acquire(&mut self) -> Result<TransferLease, DmaError> {
        if self.state != TransferState::Closed { return Err(DmaError::Busy); }
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 { self.generation = 1; }
        let lease = TransferLease(self.generation);
        self.state = TransferState::Open(lease);
        Ok(lease)
    }
    pub fn program(&self, registers: &mut impl AudioRegisters, lease: TransferLease, program: TransferProgram, continuation: bool) -> Result<(), DmaError> {
        if !self.owns(lease) { return Err(DmaError::InvalidHandle); }
        let mask = registers.mask_interrupts();
        let priority = RegisterAddress(0xfff98500);
        let previous = registers.read(priority);
        registers.write(priority, previous & !(0x1f << 8) | (13 << 8));
        registers.restore_interrupts(mask);
        registers.write(Self::register(0x2c), program.control | if continuation { 1 << 28 } else { 0 });
        registers.write(Self::register(0), program.source.get());
        registers.write(Self::register(4), program.destination.get());
        registers.write(Self::register(8), program.length.get());
        registers.write(Self::register(0xc), program.alternate_source.get());
        registers.write(Self::register(0x10), program.alternate_destination.get());
        registers.write(Self::register(0x14), program.alternate_length.get());
        registers.write(Self::register(0x30), 0);
        registers.write(Self::register(0x34), 0);
        Ok(())
    }
    pub fn start(&mut self, registers: &mut impl AudioRegisters, lease: TransferLease) -> Result<(), DmaError> {
        if !self.owns(lease) { return Err(DmaError::InvalidHandle); }
        if !matches!(self.state, TransferState::Open(_)) { return Err(DmaError::Busy); }
        if registers.read(Self::register(0x24)) & 5 != 0 { return Err(DmaError::Busy); }
        registers.write(Self::register(0x28), 8);
        let interrupt = if registers.read(Self::register(0x2c)) & 0x30 == 0 { 4 } else { 0 };
        registers.clean_data_cache();
        registers.write(Self::register(0x28), 1 | interrupt);
        self.state = TransferState::Active(lease);
        Ok(())
    }
    pub fn pending(&self, registers: &mut impl AudioRegisters, lease: TransferLease) -> Result<bool, DmaError> {
        if !self.owns(lease) { return Err(DmaError::InvalidHandle); }
        Ok(registers.read(Self::register(0x24)) & 4 != 0)
    }
    pub fn finish(&mut self, registers: &mut impl AudioRegisters, lease: TransferLease) -> Result<bool, DmaError> {
        if !self.owns(lease) { return Err(DmaError::InvalidHandle); }
        if !matches!(self.state, TransferState::Active(_)) { return Err(DmaError::NotActive); }
        if registers.read(Self::register(0x24)) & 1 != 0 { return Ok(false); }
        self.state = TransferState::Open(lease);
        Ok(true)
    }
    pub fn cancel(&mut self, registers: &mut impl AudioRegisters, lease: TransferLease) -> Result<(), DmaError> {
        if !self.owns(lease) { return Err(DmaError::InvalidHandle); }
        if !matches!(self.state, TransferState::Active(_)) { return Err(DmaError::NotActive); }
        let control = registers.read(Self::register(0x28));
        registers.write(Self::register(0x28), control | 0x100);
        loop {
            let status = registers.read(Self::register(0x24));
            if status & 1 == 0 { break; }
            if status & 8 != 0 {
                let control = registers.read(Self::register(0x28));
                registers.write(Self::register(0x28), control | 2);
                while registers.read(Self::register(0x24)) & 4 != 0 {}
                break;
            }
        }
        let control = registers.read(Self::register(0x28));
        registers.write(Self::register(0x28), control | 8);
        self.state = TransferState::Open(lease);
        Ok(())
    }
    pub fn release(&mut self, lease: TransferLease) -> Result<(), DmaError> {
        if !self.owns(lease) { return Err(DmaError::InvalidHandle); }
        self.state = TransferState::Closed;
        Ok(())
    }
}
impl Default for AudioDma { fn default() -> Self { Self::new() } }

pub fn configure_controller(registers: &mut impl AudioRegisters, bank: ControllerBank, direction: TransferDirection) -> Result<(), DmaError> {
    let control = RegisterAddress(bank.base() + 0xc);
    let mode = RegisterAddress(bank.base() + 8);
    let packing_register = RegisterAddress(bank.base() + 0x100);
    let previous = registers.read(control);
    registers.write(control, previous & !0x10001);
    let previous = registers.read(mode);
    registers.write(mode, previous & !0x8003 | 2);
    registers.write(packing_register, 0);
    if registers.read(control) & 0x10001 != 0 || registers.read(mode) & 0x8003 != 2 || registers.read(packing_register) & 3 != 0 { return Err(DmaError::Verification); }
    let previous = registers.read(mode);
    let (enabled, opposite) = match direction { TransferDirection::Receive => (1 << 16, 1 << 17), TransferDirection::Transmit => (1 << 17, 1 << 16) };
    if previous & opposite != 0 { return Err(DmaError::Busy); }
    registers.write(mode, previous | enabled);
    Ok(())
}

pub fn stop_controller(registers: &mut impl AudioRegisters, bank: ControllerBank) {
    let mode = RegisterAddress(bank.base() + 8);
    let previous = registers.read(mode);
    if previous & 0x20000 != 0 { registers.write(mode, previous & !0x20000); }
    let previous = registers.read(mode);
    if previous & 0x10000 != 0 { registers.write(mode, previous & !0x10000); }
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::{AudioRegisters, RegisterAddress};
    pub struct NativeAudioRegisters;
    extern "C" { fn xm1_power_dcache_clean_range(address: u32, length: u32); }
    impl AudioRegisters for NativeAudioRegisters {
        type InterruptMask = u32;
        fn read(&mut self, address: RegisterAddress) -> u32 { unsafe { core::ptr::read_volatile(address.get() as *const u32) } }
        fn write(&mut self, address: RegisterAddress, value: u32) { unsafe { core::ptr::write_volatile(address.get() as *mut u32, value); } }
        fn mask_interrupts(&mut self) -> u32 {
            let saved: u32;
            unsafe { core::arch::asm!("mrs {saved}, cpsr", "cpsid i", saved = out(reg) saved, options(nostack)); }
            saved
        }
        fn restore_interrupts(&mut self, mask: u32) { unsafe { core::arch::asm!("msr cpsr_c, {mask}", mask = in(reg) mask, options(nostack)); } }
        fn clean_data_cache(&mut self) { unsafe { xm1_power_dcache_clean_range(0, 0); } }
    }
}

#[cfg(test)]
mod tests {
    use super::{AudioDma, AudioRegisters, ControllerBank, DmaError, PcmAddress, PcmByteLength, RegisterAddress, SamplePacking, TransferDirection, TransferProgram, configure_controller};

    struct Registers { dma: [u32; 16], controller: [u32; 65], priority: u32 }
    impl Registers { fn new() -> Self { Self { dma: [0; 16], controller: [0; 65], priority: 0 } } }
    impl AudioRegisters for Registers {
        type InterruptMask = ();
        fn read(&mut self, address: RegisterAddress) -> u32 {
            match address.get() {
                0xfff98500 => self.priority,
                value if (0xfff02040..0xfff02080).contains(&value) => self.dma[((value - 0xfff02040) / 4) as usize],
                value if (0xfffa0000..=0xfffa0100).contains(&value) => self.controller[((value - 0xfffa0000) / 4) as usize],
                _ => panic!("unexpected register"),
            }
        }
        fn write(&mut self, address: RegisterAddress, value: u32) {
            match address.get() {
                0xfff98500 => self.priority = value,
                at if (0xfff02040..0xfff02080).contains(&at) => self.dma[((at - 0xfff02040) / 4) as usize] = value,
                at if (0xfffa0000..=0xfffa0100).contains(&at) => self.controller[((at - 0xfffa0000) / 4) as usize] = value,
                _ => panic!("unexpected register"),
            }
        }
        fn mask_interrupts(&mut self) {}
        fn restore_interrupts(&mut self, _: ()) {}
        fn clean_data_cache(&mut self) {}
    }

    #[test]
    fn receive_program_reaches_the_actual_dma_registers() {
        let mut registers = Registers::new();
        let mut dma = AudioDma::new();
        let lease = dma.acquire().unwrap();
        let program = TransferProgram::receive(ControllerBank::First, PcmAddress::from_abi(0x42e40010), PcmByteLength::from_abi(9600), SamplePacking::Packed32);
        dma.program(&mut registers, lease, program, false).unwrap();
        dma.start(&mut registers, lease).unwrap();
        assert_eq!(registers.dma[0], 0xfffa0020);
        assert_eq!(registers.dma[1], 0x42e40010);
        assert_eq!(registers.dma[2], 9600);
        assert_eq!(registers.dma[0x28 / 4], 1);
        assert_eq!(registers.priority, 13 << 8);
    }

    #[test]
    fn receive_and_transmit_cannot_enable_opposite_directions() {
        let mut registers = Registers::new();
        configure_controller(&mut registers, ControllerBank::First, TransferDirection::Receive).unwrap();
        assert_eq!(registers.controller[2] & 0x30000, 0x10000);
        assert_eq!(configure_controller(&mut registers, ControllerBank::First, TransferDirection::Transmit), Err(DmaError::Busy));
    }

    #[test]
    fn released_dma_lease_cannot_cancel_the_next_owner() {
        let mut registers = Registers::new();
        let mut dma = AudioDma::new();
        let previous = dma.acquire().unwrap();
        dma.release(previous).unwrap();
        let current = dma.acquire().unwrap();
        dma.start(&mut registers, current).unwrap();
        let before = registers.dma;
        assert_eq!(dma.cancel(&mut registers, previous), Err(DmaError::InvalidHandle));
        assert_eq!(registers.dma, before);
        dma.cancel(&mut registers, current).unwrap();
        assert_ne!(registers.dma[0x28 / 4] & 0x100, 0);
    }
}
