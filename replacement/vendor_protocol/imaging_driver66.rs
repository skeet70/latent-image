//! X-M1 1.50 command channels at `0xfff10094` and input transfers at `0xfff10140`.
//! Source pin: x70view `55436fc85c0560dd252b0410af6f2427fe198cd3`.
//! The NONMATCHING reconstructions were checked against authenticated image4 instructions.

use core::task::Poll;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Channel { Zero = 0, One = 1 }

impl Channel {
    pub const fn from_raw(raw: u32) -> Result<Self, Error> {
        match raw { 0 => Ok(Self::Zero), 1 => Ok(Self::One), _ => Err(Error::InvalidArgument) }
    }
    const fn index(self) -> usize { self as usize }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error { InvalidArgument, InvalidHandle, LockHeld, WrongLock, AlreadyStarted, NotStarted, Timeout, HandleExhausted }

impl Error {
    pub const fn stock_status(self) -> Option<i32> {
        match self {
            Self::InvalidArgument => Some(-30),
            Self::InvalidHandle | Self::LockHeld | Self::WrongLock => Some(-20),
            Self::AlreadyStarted => Some(-12),
            Self::NotStarted => Some(-11),
            Self::Timeout | Self::HandleExhausted => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallerId(pub u16);

#[derive(Debug, PartialEq, Eq)]
pub struct LockToken { caller: CallerId, sequence: u16 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Busy { Idle, Active }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum DmaEnabled { Disabled = 0, Enabled = 1 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum InputMode { Mode0 = 0, Mode1 = 1, Mode2 = 2, Mode3 = 3 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum OutputMode { Mode0 = 0, Mode1 = 1, Mode2 = 2, Mode3 = 3, Mode4 = 4, Mode5 = 5, Mode6 = 6, Mode7 = 7 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowCount(u16);
impl RowCount {
    pub const fn new(value: u16) -> Result<Self, Error> {
        if value >= 1 && value <= 512 { Ok(Self(value)) } else { Err(Error::InvalidArgument) }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Width(u8);
impl Width {
    pub const fn new(value: u8) -> Result<Self, Error> {
        if value >= 1 && value <= 8 { Ok(Self(value)) } else { Err(Error::InvalidArgument) }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaConfiguration {
    pub enabled: DmaEnabled,
    pub input: InputMode,
    pub output: OutputMode,
    pub rows: RowCount,
    pub width: Width,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusBytes(pub [u8; 6]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaAddress(u32);
impl DmaAddress {
    pub const fn new(value: u32) -> Result<Self, Error> {
        if value & !0x3fff_fffc == 0 { Ok(Self(value)) } else { Err(Error::InvalidArgument) }
    }
    pub const fn get(self) -> u32 { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum TransferMode { Mode0 = 0, Mode1 = 1, Mode2 = 2, Mode3 = 3 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum TransferFormat { Format0 = 0, Format1 = 1, Format2 = 2, Format3 = 3, Format4 = 4 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferFlags(pub u8);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferConfiguration {
    pub mode: TransferMode,
    pub format: TransferFormat,
    pub buffer: DmaAddress,
    pub offset: DmaAddress,
    pub flags: TransferFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferHandle { channel: Channel, generation: u32 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Register {
    CommandStart(Channel), CommandConfiguration(Channel), CommandDimensions(Channel),
    CommandResultLow(Channel), CommandResultHigh(Channel), CommandBusy(Channel),
    TransferFlags(Channel), TransferControl(Channel), TransferConfiguration(Channel),
    TransferBuffer(Channel), TransferOffset(Channel), TransferInterruptClear(Channel),
    TransferBusy(Channel),
}
impl Register {
    pub const fn address(self) -> u32 {
        match self {
            Self::CommandStart(c) => 0xfff1_0094 + c as u32 * 0x18,
            Self::CommandConfiguration(c) => 0xfff1_0098 + c as u32 * 0x18,
            Self::CommandDimensions(c) => 0xfff1_009c + c as u32 * 0x18,
            Self::CommandResultLow(c) => 0xfff1_00a0 + c as u32 * 0x18,
            Self::CommandResultHigh(c) => 0xfff1_00a4 + c as u32 * 0x18,
            Self::CommandBusy(c) => 0xfff1_00a8 + c as u32 * 0x18,
            Self::TransferFlags(c) => 0xfff1_0140 + c as u32 * 0x10c,
            Self::TransferControl(c) => 0xfff1_0154 + c as u32 * 0x100,
            Self::TransferConfiguration(c) => 0xfff1_0170 + c as u32 * 0x100,
            Self::TransferBuffer(c) => 0xfff1_0174 + c as u32 * 0x100,
            Self::TransferOffset(c) => 0xfff1_0178 + c as u32 * 0x100,
            Self::TransferInterruptClear(c) => 0xfff1_015c + c as u32 * 0x100,
            Self::TransferBusy(c) => 0xfff1_0160 + c as u32 * 0x100,
        }
    }
}

pub trait RegisterIo {
    fn read(&mut self, register: Register) -> u32;
    fn write(&mut self, register: Register, value: u32);
}

#[derive(Debug, PartialEq, Eq)]
pub struct ImagingRegion(());
impl ImagingRegion {
    pub const fn new(base: u32, bytes: usize) -> Result<Self, Error> {
        if base == 0xfff1_0000 && bytes >= 0x400 { Ok(Self(())) } else { Err(Error::InvalidArgument) }
    }
}

pub struct PhysicalRegisters { _exclusive: core::marker::PhantomData<*mut ()> }
impl PhysicalRegisters {
    /// # Safety
    /// The caller owns the powered, device-mapped `0xfff10000..0xfff10400` domain exclusively.
    /// All IRQ and task access to this driver must be serialized by the caller.
    /// Programmed DMA memory must remain owned, mapped and coherent until hardware quiesces.
    pub unsafe fn admit(_region: ImagingRegion) -> Self { Self { _exclusive: core::marker::PhantomData } }
}
impl RegisterIo for PhysicalRegisters {
    fn read(&mut self, register: Register) -> u32 {
        unsafe { core::ptr::read_volatile(register.address() as *const u32) }
    }
    fn write(&mut self, register: Register, value: u32) {
        unsafe { core::ptr::write_volatile(register.address() as *mut u32, value) }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TransferState { Unconfigured, Configured, Started, Stopping }
#[derive(Clone, Copy)]
struct Transfer { generation: u32, state: TransferState }

pub struct Driver<I> {
    io: I,
    owner: Option<(CallerId, u16)>,
    next_lock: u16,
    next_handle: u32,
    transfers: [Option<Transfer>; 2],
}

impl<I: RegisterIo> Driver<I> {
    pub const fn new(io: I) -> Self {
        Self { io, owner: None, next_lock: 1, next_handle: 1, transfers: [None; 2] }
    }

    pub fn acquire(&mut self, caller: CallerId) -> Result<LockToken, Error> {
        if self.owner.is_some() { return Err(Error::LockHeld); }
        let sequence = self.next_lock;
        self.next_lock = if sequence == 0x1000 { 1 } else { sequence + 1 };
        self.owner = Some((caller, sequence));
        Ok(LockToken { caller, sequence })
    }

    pub fn release(&mut self, token: &LockToken) -> Result<(), Error> {
        if self.owner != Some((token.caller, token.sequence)) { return Err(Error::WrongLock); }
        self.owner = None;
        Ok(())
    }

    pub fn busy(&mut self, channel: Channel) -> Busy {
        if self.io.read(Register::CommandBusy(channel)) & 1 == 0 { Busy::Idle } else { Busy::Active }
    }

    pub fn configure(&mut self, channel: Channel, config: DmaConfiguration) {
        self.io.write(Register::CommandConfiguration(channel),
            ((config.enabled as u32) << 24) | ((config.input as u32) << 16) | config.output as u32);
        self.io.write(Register::CommandDimensions(channel),
            ((u32::from(config.rows.0) - 1) << 16) | (u32::from(config.width.0) - 1));
    }

    pub fn start(&mut self, channel: Channel) { self.io.write(Register::CommandStart(channel), 1); }
    pub fn cancel(&mut self, channel: Channel) { self.io.write(Register::CommandStart(channel), 0); }

    pub fn read6(&mut self, channel: Channel) -> StatusBytes {
        let low = self.io.read(Register::CommandResultLow(channel)).to_le_bytes();
        let high = self.io.read(Register::CommandResultHigh(channel)).to_le_bytes();
        StatusBytes([low[0], low[1], low[2], low[3], high[0], high[1]])
    }

    pub fn transfer_open(&mut self, channel: Channel) -> Result<TransferHandle, Error> {
        if self.transfers[channel.index()].is_some() { return Err(Error::InvalidHandle); }
        let generation = self.next_handle;
        self.next_handle = generation.checked_add(1).ok_or(Error::HandleExhausted)?;
        self.transfers[channel.index()] = Some(Transfer { generation, state: TransferState::Unconfigured });
        Ok(TransferHandle { channel, generation })
    }

    fn transfer(&self, handle: TransferHandle) -> Result<Transfer, Error> {
        self.transfers[handle.channel.index()].filter(|t| t.generation == handle.generation).ok_or(Error::InvalidHandle)
    }

    pub fn transfer_configure(&mut self, handle: TransferHandle, config: TransferConfiguration) -> Result<(), Error> {
        let transfer = self.transfer(handle)?;
        if matches!(transfer.state, TransferState::Started | TransferState::Stopping) { return Err(Error::AlreadyStarted); }
        let c = handle.channel;
        self.io.write(Register::TransferFlags(c), u32::from(config.flags.0));
        let value = self.io.read(Register::TransferConfiguration(c));
        self.io.write(Register::TransferConfiguration(c),
            (value & !0x0070_3000) | ((config.mode as u32) << 12) | ((config.format as u32) << 20));
        self.io.write(Register::TransferBuffer(c), config.buffer.0);
        self.io.write(Register::TransferOffset(c), config.offset.0);
        self.transfers[c.index()] = Some(Transfer { state: TransferState::Configured, ..transfer });
        Ok(())
    }

    pub fn transfer_start(&mut self, handle: TransferHandle) -> Result<(), Error> {
        let transfer = self.transfer(handle)?;
        match transfer.state {
            TransferState::Started | TransferState::Stopping => return Err(Error::AlreadyStarted),
            TransferState::Unconfigured => return Err(Error::InvalidArgument),
            TransferState::Configured => (),
        }
        let c = handle.channel;
        self.io.write(Register::TransferInterruptClear(c), 7);
        let config = self.io.read(Register::TransferConfiguration(c));
        self.io.write(Register::TransferConfiguration(c), config & !(1 << 28));
        self.transfers[c.index()] = Some(Transfer { state: TransferState::Started, ..transfer });
        self.io.write(Register::TransferControl(c), 0x0001_0001);
        Ok(())
    }

    /// `0x00663204` stops the transfer before waiting for hardware quiescence.
    pub fn transfer_wait(&mut self, handle: TransferHandle) -> Result<Poll<()>, Error> {
        let transfer = self.transfer(handle)?;
        let c = handle.channel;
        match transfer.state {
            TransferState::Unconfigured | TransferState::Configured => return Err(Error::NotStarted),
            TransferState::Started => {
                self.io.write(Register::TransferInterruptClear(c), 7);
                self.io.write(Register::TransferControl(c), 0);
                self.transfers[c.index()] = Some(Transfer { state: TransferState::Stopping, ..transfer });
            }
            TransferState::Stopping => (),
        }
        if self.io.read(Register::TransferBusy(c)) != 0 { return Ok(Poll::Pending); }
        let config = self.io.read(Register::TransferConfiguration(c));
        self.io.write(Register::TransferConfiguration(c), config & !(1 << 28));
        self.transfers[c.index()] = Some(Transfer { state: TransferState::Configured, ..transfer });
        Ok(Poll::Ready(()))
    }

    pub fn transfer_release(&mut self, handle: TransferHandle) -> Result<(), Error> {
        let transfer = self.transfer(handle)?;
        if matches!(transfer.state, TransferState::Started | TransferState::Stopping) { return Err(Error::AlreadyStarted); }
        self.transfers[handle.channel.index()] = None;
        Ok(())
    }

    /// A deadline never establishes hardware quiescence.
    pub fn timeout(&mut self, channel: Channel, handle: TransferHandle) -> Result<(), Error> {
        self.transfer(handle)?;
        if handle.channel != channel { return Err(Error::InvalidArgument); }
        self.cancel(channel);
        match self.transfer_wait(handle) {
            Ok(_) | Err(Error::NotStarted) => Err(Error::Timeout),
            Err(error) => Err(error),
        }
    }

    pub fn acknowledge_interrupt(&mut self, channel: Channel) -> Busy {
        self.io.write(Register::TransferInterruptClear(channel), 7);
        self.busy(channel)
    }
}


#[cfg(test)]
#[path = "imaging_driver66_tests.rs"]
mod tests;
