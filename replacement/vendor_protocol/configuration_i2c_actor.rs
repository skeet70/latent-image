//! X-M1 I2C CPU ownership for configuration action 14.
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, 0x006a6124 and 0x006a6748.
//! Register transitions and routing are authenticated against image 4.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferId(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Device { Zero, One, Two, Three, Four, Five, Six }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterruptLine { Channel0, Channel1 }

impl InterruptLine {
    pub const fn number(self) -> u32 { match self { Self::Channel0 => 0x4c, Self::Channel1 => 0x4d } }
}

impl Device {
    fn routing(self) -> (u8, usize, bool) {
        match self {
            Self::Zero => (48, 1, false), Self::One => (24, 0, true),
            Self::Two => (68, 0, false), Self::Three => (114, 1, false),
            Self::Four => (25, 0, true), Self::Five => (12, 0, true),
            Self::Six => (28, 0, true),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegisterAddress(pub u32);

pub trait RegisterIo {
    fn read(&mut self, address: RegisterAddress) -> u32;
    fn write(&mut self, address: RegisterAddress, value: u32);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error { Busy, InvalidLength, Bus, Cancelled, Stale }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { Idle, Enabled, Start, AddressWrite, Write, AddressRead, Read, Stop, Disabled, Complete }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Received { pub bytes: [u8; 10], pub length: u8 }

pub struct I2cActor {
    generation: u32,
    phase: Phase,
    device: Device,
    outgoing: [u8; 11],
    write_length: u8,
    read_length: u8,
    cursor: u8,
    received: Received,
    failure: Option<Error>,
    interrupted: bool,
}

impl I2cActor {
    pub const fn new() -> Self {
        Self { generation: 0, phase: Phase::Idle, device: Device::Zero, outgoing: [0; 11], write_length: 0, read_length: 0, cursor: 0, received: Received { bytes: [0; 10], length: 0 }, failure: None, interrupted: false }
    }

    pub fn begin(&mut self, io: &mut impl RegisterIo, device: Device, outgoing: &[u8], read_length: u8) -> Result<TransferId, Error> {
        if self.phase != Phase::Idle { return Err(Error::Busy); }
        if outgoing.is_empty() || outgoing.len() > 11 || read_length > 10 { return Err(Error::InvalidLength); }
        self.generation = self.generation.wrapping_add(1);
        self.device = device;
        self.outgoing[..outgoing.len()].copy_from_slice(outgoing);
        self.write_length = outgoing.len() as u8;
        self.read_length = read_length;
        self.cursor = 0;
        self.received = Received { bytes: [0; 10], length: 0 };
        self.failure = None;
        self.interrupted = false;
        let (_, _, clock) = device.routing();
        let mode = self.address(0x10);
        let value = (io.read(mode) & !0xb) | ((clock as u32) << 3) | 1;
        io.write(mode, value);
        io.write(self.address(0), 1);
        self.phase = Phase::Enabled;
        Ok(TransferId(self.generation))
    }

    fn address(&self, offset: u32) -> RegisterAddress {
        RegisterAddress(0xfff8_5000 + self.device.routing().1 as u32 * 0x40 + offset)
    }

    fn control(&self, io: &mut impl RegisterIo, set: u32, clear: u32) {
        let address = self.address(8);
        let value = (io.read(address) & !clear) | set;
        io.write(address, value);
    }

    fn stop(&mut self, io: &mut impl RegisterIo, failure: Option<Error>) {
        self.failure = failure;
        self.interrupted = false;
        self.control(io, 1, 0);
        self.phase = Phase::Stop;
    }

    pub fn poll(&mut self, io: &mut impl RegisterIo, id: TransferId) -> Result<Option<Received>, Error> {
        if id.0 != self.generation || self.phase == Phase::Idle { return Err(Error::Stale); }
        if matches!(self.phase, Phase::Start | Phase::AddressWrite | Phase::Write | Phase::AddressRead | Phase::Read | Phase::Stop) {
            if !self.interrupted { return Ok(None); }
            self.interrupted = false;
        }
        match self.phase {
            Phase::Enabled => {
                if io.read(self.address(0)) & 1 != 0 {
                    self.control(io, 0x19, 0);
                    self.phase = Phase::Start;
                }
            }
            Phase::Start => {
                if io.read(self.address(0x18)) & 1 == 0 { self.stop(io, Some(Error::Bus)); }
                else {
                    self.control(io, 2, 0);
                    io.write(self.address(4), u32::from(self.device.routing().0) << 1);
                    self.phase = Phase::AddressWrite;
                }
            }
            Phase::AddressWrite => {
                if io.read(self.address(0x18)) & 0xc != 0xc { self.stop(io, Some(Error::Bus)); }
                else {
                    io.write(self.address(4), u32::from(self.outgoing[0]));
                    self.phase = Phase::Write;
                }
            }
            Phase::Write => {
                let acknowledged = io.read(self.address(0x18)) & 4 != 0;
                self.cursor += 1;
                if !acknowledged && self.cursor != self.write_length { self.stop(io, Some(Error::Bus)); }
                else if self.cursor != self.write_length {
                    io.write(self.address(4), u32::from(self.outgoing[self.cursor as usize]));
                } else if self.read_length == 0 { self.stop(io, None); }
                else {
                    self.control(io, 2, 0);
                    io.write(self.address(4), (u32::from(self.device.routing().0) << 1) | 1);
                    self.phase = Phase::AddressRead;
                }
            }
            Phase::AddressRead => {
                if io.read(self.address(0x18)) & 0xc != 4 { self.stop(io, Some(Error::Bus)); }
                else {
                    self.cursor = 0;
                    self.control(io, 0xc, 0);
                    self.request_byte(io);
                    self.phase = Phase::Read;
                }
            }
            Phase::Read => {
                self.received.bytes[self.cursor as usize] = io.read(self.address(4)) as u8;
                self.cursor += 1;
                if self.cursor == self.read_length {
                    self.received.length = self.read_length;
                    self.stop(io, None);
                } else { self.request_byte(io); }
            }
            Phase::Stop => {
                io.write(self.address(0), 0);
                self.phase = Phase::Disabled;
            }
            Phase::Disabled => {
                if io.read(self.address(0)) & 1 == 0 { self.phase = Phase::Complete; }
            }
            Phase::Complete => {
                self.phase = Phase::Idle;
                return match self.failure { Some(error) => Err(error), None => Ok(Some(self.received)) };
            }
            Phase::Idle => return Err(Error::Stale),
        }
        Ok(None)
    }

    fn request_byte(&self, io: &mut impl RegisterIo) {
        self.control(io, 0x20, if self.cursor + 1 == self.read_length { 4 } else { 0 });
    }

    pub fn interrupt_line(&self, id: TransferId) -> Result<InterruptLine, Error> {
        if id.0 != self.generation || self.phase == Phase::Idle { return Err(Error::Stale); }
        Ok(if self.device.routing().1 == 0 { InterruptLine::Channel0 } else { InterruptLine::Channel1 })
    }

    pub fn interrupt(&mut self, id: TransferId) -> Result<(), Error> {
        if id.0 != self.generation || !matches!(self.phase, Phase::Start | Phase::AddressWrite | Phase::Write | Phase::AddressRead | Phase::Read | Phase::Stop) { return Err(Error::Stale); }
        self.interrupted = true;
        Ok(())
    }

    pub fn cancel(&mut self, io: &mut impl RegisterIo, id: TransferId) -> Result<(), Error> {
        if id.0 != self.generation || self.phase == Phase::Idle { return Err(Error::Stale); }
        if self.phase == Phase::Enabled {
            self.failure = Some(Error::Cancelled);
            io.write(self.address(0), 0);
            self.phase = Phase::Disabled;
        } else if !matches!(self.phase, Phase::Stop | Phase::Disabled | Phase::Complete) { self.stop(io, Some(Error::Cancelled)); }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Device, Error, I2cActor, RegisterAddress, RegisterIo};

    struct Registers([u32; 8]);
    impl RegisterIo for Registers {
        fn read(&mut self, address: RegisterAddress) -> u32 { self.0[((address.0 - 0xfff8_5000) & 0x3f) as usize / 4] }
        fn write(&mut self, address: RegisterAddress, value: u32) { self.0[((address.0 - 0xfff8_5000) & 0x3f) as usize / 4] = value; }
    }

    #[test]
    fn combined_read_waits_for_interrupts_and_zero_fills_unused_bytes() {
        let mut io = Registers([0; 8]);
        let mut actor = I2cActor::new();
        let id = actor.begin(&mut io, Device::Five, &[0x24], 1).unwrap();
        assert_eq!(actor.poll(&mut io, id), Ok(None));
        assert_eq!(actor.poll(&mut io, id), Ok(None));
        io.0[6] = 1;
        actor.interrupt(id).unwrap();
        actor.poll(&mut io, id).unwrap();
        io.0[6] = 0xc;
        actor.interrupt(id).unwrap();
        actor.poll(&mut io, id).unwrap();
        actor.interrupt(id).unwrap();
        actor.poll(&mut io, id).unwrap();
        io.0[6] = 4;
        actor.interrupt(id).unwrap();
        actor.poll(&mut io, id).unwrap();
        io.0[1] = 0xa5;
        actor.interrupt(id).unwrap();
        actor.poll(&mut io, id).unwrap();
        actor.interrupt(id).unwrap();
        actor.poll(&mut io, id).unwrap();
        actor.poll(&mut io, id).unwrap();
        let received = actor.poll(&mut io, id).unwrap().unwrap();
        assert_eq!(received.length, 1);
        assert_eq!(received.bytes, [0xa5, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(actor.interrupt(id), Err(Error::Stale));
    }

    #[test]
    fn cancellation_releases_bus_before_reporting_and_rejects_late_events() {
        let mut io = Registers([0; 8]);
        let mut actor = I2cActor::new();
        let id = actor.begin(&mut io, Device::Zero, &[1, 2], 0).unwrap();
        actor.poll(&mut io, id).unwrap();
        actor.cancel(&mut io, id).unwrap();
        assert_eq!(actor.begin(&mut io, Device::Zero, &[1], 0), Err(Error::Busy));
        actor.interrupt(id).unwrap();
        actor.poll(&mut io, id).unwrap();
        actor.poll(&mut io, id).unwrap();
        assert_eq!(actor.poll(&mut io, id), Err(Error::Cancelled));
        let next = actor.begin(&mut io, Device::Zero, &[1], 0).unwrap();
        assert_ne!(next, id);
        assert_eq!(actor.interrupt(id), Err(Error::Stale));
    }
}

#[cfg(test)]
mod error_tests {
    use super::{Device, Error, I2cActor, RegisterAddress, RegisterIo};

    struct Unacknowledged { enabled: bool }
    impl RegisterIo for Unacknowledged {
        fn read(&mut self, address: RegisterAddress) -> u32 {
            if address.0 & 0x3f == 0 { u32::from(self.enabled) } else { 0 }
        }
        fn write(&mut self, address: RegisterAddress, value: u32) {
            if address.0 & 0x3f == 0 { self.enabled = value & 1 != 0; }
        }
    }

    #[test]
    fn bus_start_failure_disables_controller_before_returning_error() {
        let mut io = Unacknowledged { enabled: false };
        let mut actor = I2cActor::new();
        let id = actor.begin(&mut io, Device::Five, &[1], 0).unwrap();
        actor.poll(&mut io, id).unwrap();
        actor.interrupt(id).unwrap();
        assert_eq!(actor.poll(&mut io, id), Ok(None));
        actor.interrupt(id).unwrap();
        actor.poll(&mut io, id).unwrap();
        actor.poll(&mut io, id).unwrap();
        assert_eq!(actor.poll(&mut io, id), Err(Error::Bus));
        assert!(!io.enabled);
    }
}
