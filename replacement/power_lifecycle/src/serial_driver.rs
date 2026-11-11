#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Endpoint { AudioCodec, LcdPanel, Backup, Charge }
impl Endpoint {
    pub const fn parse(value: u32) -> Option<Self> { match value { 0 => Some(Self::AudioCodec), 1 => Some(Self::LcdPanel), 9 => Some(Self::Backup), 10 => Some(Self::Charge), _ => None } }
    pub const fn index(self) -> u32 { match self { Self::AudioCodec => 0, Self::LcdPanel => 1, Self::Backup => 9, Self::Charge => 10 } }
    pub const fn descriptor(self) -> Descriptor {
        match self {
            Self::AudioCodec => Descriptor { channel: Channel::One, pin: Pin { port: 12, bit: 3 }, phase: false, polarity: true, word_bits: 0, divider: 2, flag16: true, flag17: true },
            Self::LcdPanel => Descriptor { channel: Channel::One, pin: Pin { port: 11, bit: 3 }, phase: false, polarity: true, word_bits: 0, divider: 3, flag16: false, flag17: false },
            Self::Backup => Descriptor { channel: Channel::One, pin: Pin { port: 12, bit: 3 }, phase: false, polarity: true, word_bits: 0, divider: 3, flag16: false, flag17: false },
            Self::Charge => Descriptor { channel: Channel::Zero, pin: Pin { port: 11, bit: 2 }, phase: false, polarity: false, word_bits: 0, divider: 24, flag16: false, flag17: false },
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Channel { Zero, One }
impl Channel {
    pub const fn index(self) -> usize { match self { Self::Zero => 0, Self::One => 1 } }
    pub const fn bank(self) -> usize { 0xfff8_4000 + self.index() * 64 }
    pub const fn semaphore(self) -> i32 { 0x15 + self.index() as i32 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pin { port: u8, bit: u8 }
impl Pin {
    pub const fn address(self) -> usize { 0xfff8_9000 + self.port as usize * 64 }
    pub const fn output(self, active: bool) -> u32 { (1 << (self.bit + 16)) | ((!active as u32) << self.bit) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Descriptor { pub channel: Channel, pub pin: Pin, phase: bool, polarity: bool, word_bits: u8, divider: u16, flag16: bool, flag17: bool }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenMode { Transmit, Duplex }
impl OpenMode { pub const fn from_abi(value: u32) -> Self { if value == 1 { Self::Transmit } else { Self::Duplex } } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Handle(u32);
impl Handle { pub const fn from_abi(value: u32) -> Self { Self(value) } pub const fn raw(self) -> u32 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SerialWord(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatusResidue(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Delay(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionState { Closed, Open, Started, Other(u32) }
impl TransactionState {
    fn from_abi(value: u32) -> Self {
        match value { 0 => Self::Closed, 1 => Self::Open, 2 => Self::Started, other => Self::Other(other) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register { Control, Clock, InterruptEnable, Status, ReceiveThreshold, TransmitThreshold, Receive, Transmit }
impl Register { pub const fn offset(self) -> usize { match self { Self::Control => 0, Self::Clock => 4, Self::InterruptEnable => 8, Self::Status => 12, Self::ReceiveThreshold => 16, Self::TransmitThreshold => 20, Self::Receive => 24, Self::Transmit => 28 } } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum Error { InvalidHandle = -20, Closed = -10, AlreadyStopped = -11, AlreadyStarted = -12, InvalidEndpoint = -30, Semaphore = -255 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct Status { pub receive_flag: u32, pub transmit_flag: u32, pub error_flag: u32, pub receive_threshold: u32, pub transmit_threshold: u32, pub receive_dma: u32, pub transmit_dma: u32 }
pub trait Hardware {
    fn read(&mut self, channel: Channel, register: Register) -> u32;
    fn write(&mut self, channel: Channel, register: Register, value: u32);
    fn select(&mut self, pin: Pin, active: bool);
    fn delay(&mut self, delay: Delay);
    fn update(&mut self, channel: Channel, register: Register, clear: u32, set: u32) {
        let value = self.read(channel, register);
        self.write(channel, register, (value & !clear) | set);
    }
}
#[repr(C)]
pub struct State { phases: [u32; 4], endpoints: [u32; 4], handles: [u32; 4] }
impl State {
    pub const fn new() -> Self { Self { phases: [0; 4], endpoints: [u32::MAX; 4], handles: [0; 4] } }
    fn resolve(&self, handle: Handle) -> Result<(Endpoint, Descriptor), Error> {
        let slot = self.handles.iter().position(|value| *value == handle.0 && handle.0 != 0).ok_or(Error::InvalidHandle)?;
        let endpoint = Endpoint::parse(self.endpoints[slot]).ok_or(Error::InvalidEndpoint)?;
        Ok((endpoint, endpoint.descriptor()))
    }
    pub fn open(&mut self, hardware: &mut impl Hardware, endpoint: Endpoint, mode: OpenMode, sequence: &mut u32) -> Result<Handle, Error> {
        let handle = self.reserve(endpoint, sequence)?;
        Self::initialize(hardware, endpoint.descriptor(), mode);
        Ok(handle)
    }
    fn reserve(&mut self, endpoint: Endpoint, sequence: &mut u32) -> Result<Handle, Error> {
        let d = endpoint.descriptor();
        let slot = d.channel.index();
        if self.handles[slot] != 0 { return Err(Error::InvalidHandle); }
        loop { *sequence = sequence.wrapping_add(1); if *sequence != 0 && !self.handles.contains(sequence) { break; } }
        let handle = Handle(*sequence);
        self.handles[slot] = handle.0;
        self.endpoints[slot] = endpoint.index();
        self.phases[slot] = 1;
        Ok(handle)
    }
    fn initialize(hardware: &mut impl Hardware, d: Descriptor, mode: OpenMode) {
        for bit in [28, 16, 19, 24, 27, 0, 4, 8, 9, 12, 13] { modify(hardware, d.channel, Register::InterruptEnable, 1 << bit, 0); }
        modify(hardware, d.channel, Register::Control, 64, if mode == OpenMode::Transmit { 0 } else { 64 });
        modify(hardware, d.channel, Register::Control, 16, (d.phase as u32) << 4);
        modify(hardware, d.channel, Register::Control, 32, (d.polarity as u32) << 5);
        modify(hardware, d.channel, Register::Control, 0xf0000, (d.word_bits as u32) << 16);
        modify(hardware, d.channel, Register::Clock, 0x7ffe, (d.divider as u32) << 1);
        modify(hardware, d.channel, Register::Clock, 1 << 16, (d.flag16 as u32) << 16);
        modify(hardware, d.channel, Register::Clock, 1 << 17, (d.flag17 as u32) << 17);
        modify(hardware, d.channel, Register::Clock, 1 << 15, 0);
        modify(hardware, d.channel, Register::InterruptEnable, 0, 1 << 28);
        modify(hardware, d.channel, Register::InterruptEnable, 1 << 28, 0);
        for bit in [0, 4, 8, 9, 12, 13] { modify(hardware, d.channel, Register::Status, 0, 1 << bit); }
        modify(hardware, d.channel, Register::ReceiveThreshold, 31, 1);
        modify(hardware, d.channel, Register::TransmitThreshold, 31, 1);
    }
    pub fn configure(&mut self, hardware: &mut impl Hardware, handle: Handle, delay: Delay) -> Result<(), Error> {
        let (_, d) = self.resolve(handle)?;
        if TransactionState::from_abi(self.phases[d.channel.index()]) != TransactionState::Open { return Err(Error::AlreadyStarted); }
        self.phases[d.channel.index()] = 2;
        hardware.select(d.pin, true);
        if delay.0 != 0 { hardware.delay(delay); }
        modify(hardware, d.channel, Register::Control, 0, 128);
        Ok(())
    }
    pub fn write(&self, hardware: &mut impl Hardware, handle: Handle, word: SerialWord, residue: StatusResidue) -> Result<(), Error> {
        let (_, d) = self.resolve(handle)?;
        if TransactionState::from_abi(self.phases[d.channel.index()]) == TransactionState::Closed { return Err(Error::Closed); }
        let high = hardware.read(d.channel, Register::Status) & 0xffff0000;
        hardware.write(d.channel, Register::Status, high | residue.0 | 0x3311);
        hardware.write(d.channel, Register::Transmit, word.0 as u32);
        Ok(())
    }
    pub fn status(&self, hardware: &mut impl Hardware, handle: Handle) -> Result<Status, Error> {
        let (_, d) = self.resolve(handle)?;
        if TransactionState::from_abi(self.phases[d.channel.index()]) == TransactionState::Closed { return Err(Error::Closed); }
        let status = hardware.read(d.channel, Register::Status);
        let receive_threshold = hardware.read(d.channel, Register::ReceiveThreshold) & 31;
        let transmit_threshold = hardware.read(d.channel, Register::TransmitThreshold) & 31;
        let receive_dma = (hardware.read(d.channel, Register::InterruptEnable) >> 18) & 1;
        let transmit_dma = (hardware.read(d.channel, Register::InterruptEnable) >> 26) & 1;
        Ok(Status { receive_flag: (status >> 8) & 1, transmit_flag: (status >> 9) & 1, error_flag: (status >> 12) & 1, receive_threshold, transmit_threshold, receive_dma, transmit_dma })
    }
    pub fn read(&self, hardware: &mut impl Hardware, handle: Handle) -> Result<SerialWord, Error> {
        let (_, d) = self.resolve(handle)?;
        if TransactionState::from_abi(self.phases[d.channel.index()]) == TransactionState::Closed { return Err(Error::Closed); }
        Ok(SerialWord(hardware.read(d.channel, Register::Receive) as u16))
    }
    pub fn stop(&mut self, hardware: &mut impl Hardware, handle: Handle) -> Result<(), Error> {
        let (_, d) = self.resolve(handle)?;
        if TransactionState::from_abi(self.phases[d.channel.index()]) == TransactionState::Open { return Err(Error::AlreadyStopped); }
        self.phases[d.channel.index()] = 1;
        disable(hardware, d.channel);
        hardware.select(d.pin, false);
        Ok(())
    }
    pub fn close(&mut self, hardware: &mut impl Hardware, handle: Handle) -> Result<Channel, Error> {
        let channel = self.begin_close(hardware, handle)?;
        self.handles[channel.index()] = 0;
        Ok(channel)
    }
    fn begin_close(&mut self, hardware: &mut impl Hardware, handle: Handle) -> Result<Channel, Error> {
        let (_, d) = self.resolve(handle)?;
        disable(hardware, d.channel);
        let slot = d.channel.index();
        self.endpoints[slot] = u32::MAX;
        self.phases[slot] = 0;
        Ok(d.channel)
    }
}
fn modify(hardware: &mut impl Hardware, channel: Channel, register: Register, clear: u32, set: u32) {
    hardware.update(channel, register, clear, set);
}
fn disable(hardware: &mut impl Hardware, channel: Channel) {
    modify(hardware, channel, Register::Control, 128, 0);
    while hardware.read(channel, Register::Control) & 1 != 0 { core::hint::spin_loop(); }
}

#[cfg(target_arch = "arm")]
#[path = "serial_native.rs"]
mod native;
#[cfg(test)]
#[path = "serial_driver_tests.rs"]
mod tests;
