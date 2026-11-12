//! The stock first record's image-4 decompression engine at 0xfff43000.
//!
//! Stock 0x3b6c-0x3e9c enables the engine clock through 0xfff88010/0xfff88030,
//! resets it through 0xfff43000/0xfff4308c, submits each stream chunk through
//! 0xfff43040/0x44/0x48/0x80/0x8c and 0xfff43004 = 2, and completes on
//! 0xfff43088 bit 0 with bits 8-11 as faults. The output count is
//! 0xfff4304c and the acknowledge is 0xfff43088 = 1.

pub const ENGINE_BASE: u32 = 0xfff4_3000;
pub const CLOCK_BASE: u32 = 0xfff8_8010;
pub const PAGE_BYTES: u32 = 0x4000;
pub const CONTROL: u32 = 0x3000_3000;
pub const STATUS_CLEAR: u32 = 0xf01;
const STATUS_FAULT_MASK: u32 = 0xf00;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum EngineRegister {
    Reset = 0x00,
    Start = 0x04,
    Source = 0x40,
    Length = 0x44,
    Destination = 0x48,
    OutputCount = 0x4c,
    Control = 0x80,
    Status = 0x88,
    Enable = 0x8c,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ClockRegister {
    Gate = 0x00,
    Enable = 0x20,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register {
    Engine(EngineRegister),
    Clock(ClockRegister),
}

impl Register {
    fn address(self) -> u32 {
        match self {
            Self::Engine(register) => ENGINE_BASE + register as u32,
            Self::Clock(register) => CLOCK_BASE + register as u32,
        }
    }
}

pub trait Bus {
    fn read(&mut self, register: Register) -> u32;
    fn write(&mut self, register: Register, value: u32);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Source(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Destination(pub u32);

pub struct VolatileBus;

impl Bus for VolatileBus {
    fn read(&mut self, register: Register) -> u32 {
        unsafe { core::ptr::read_volatile(register.address() as *const u32) }
    }

    fn write(&mut self, register: Register, value: u32) {
        unsafe { core::ptr::write_volatile(register.address() as *mut u32, value); }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineError {
    Fault(u32),
    Length,
    Page,
}

/// Stock 0x3bcc waits for reset bit 0 and has no failure exit.
pub struct Session<B: Bus> {
    bus: B,
}

impl<B: Bus> Session<B> {
    pub fn open(mut bus: B) -> Self {
        let gate = bus.read(Register::Clock(ClockRegister::Gate));
        bus.write(Register::Clock(ClockRegister::Gate), gate & !7);
        let enable = bus.read(Register::Clock(ClockRegister::Enable));
        bus.write(Register::Clock(ClockRegister::Enable), enable | 4);
        bus.write(Register::Engine(EngineRegister::Enable), 0);
        bus.write(Register::Engine(EngineRegister::Reset), 0);
        while bus.read(Register::Engine(EngineRegister::Reset)) & 1 == 0 {}
        bus.write(Register::Engine(EngineRegister::Status), STATUS_CLEAR);
        Self { bus }
    }

    pub fn submit(&mut self, source: Source, destination: Destination, length: u32) -> Result<u32, EngineError> {
        if length == 0 || length > 0x7ffff {
            return Err(EngineError::Length);
        }
        self.bus.write(Register::Engine(EngineRegister::Source), source.0);
        self.bus.write(Register::Engine(EngineRegister::Length), length);
        self.bus.write(Register::Engine(EngineRegister::Destination), destination.0);
        self.bus.write(Register::Engine(EngineRegister::Control), CONTROL);
        self.bus.write(Register::Engine(EngineRegister::Enable), 1);
        self.bus.write(Register::Engine(EngineRegister::Start), 2);
        self.wait()?;
        let count = self.bus.read(Register::Engine(EngineRegister::OutputCount));
        self.bus.write(Register::Engine(EngineRegister::Status), 1);
        Ok(count)
    }

    /// Stock 0x3d08 treats fault bits 8-11 as a halt.
    fn wait(&mut self) -> Result<(), EngineError> {
        loop {
            let status = self.bus.read(Register::Engine(EngineRegister::Status));
            if status & STATUS_FAULT_MASK != 0 {
                return Err(EngineError::Fault(status));
            }
            if status & 1 != 0 {
                return Ok(());
            }
        }
    }

    /// Stock 0x3c1c then 0x3b9c.
    pub fn close(mut self) -> B {
        self.bus.write(Register::Engine(EngineRegister::Enable), 0);
        self.bus.write(Register::Engine(EngineRegister::Status), STATUS_CLEAR);
        let enable = self.bus.read(Register::Clock(ClockRegister::Enable));
        self.bus.write(Register::Clock(ClockRegister::Enable), enable & !4);
        let gate = self.bus.read(Register::Clock(ClockRegister::Gate));
        self.bus.write(Register::Clock(ClockRegister::Gate), gate | 7);
        self.bus
    }
}

/// Stock 0x3ea0's feed-end computation at 0x3f64-0x3fa0.
pub fn feed_end(available: u32, record_length: u32, page: u32) -> Result<u32, EngineError> {
    if page == 0 {
        return Err(EngineError::Page);
    }
    let end = if available >= record_length {
        record_length.checked_sub(20).ok_or(EngineError::Length)?
    } else {
        (available / page)
            .checked_mul(page)
            .and_then(|whole| whole.checked_sub(20))
            .ok_or(EngineError::Length)?
    };
    Ok(end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::images::Method4;

    const SOURCE_BASE: u32 = 0x42e4_0000;
    const DEST_BASE: u32 = 0x4061_c400;
    const OUTPUT_CAPACITY: usize = 0x4000;

    struct Device {
        source: Vec<u8>,
        output: Vec<u8>,
        stream: Method4,
        registers: [u32; 0x90 / 4],
        clock: [u32; 0x40 / 4],
        writes: Vec<(u32, u32)>,
        reset_ready: bool,
        count: u32,
        completions_after: u32,
        remaining: u32,
        status_reads: u32,
    }

    impl Device {
        fn new(record: &[u8]) -> Self {
            let mut source = vec![0; 0x20000];
            source[..record.len()].copy_from_slice(record);
            Self {
                source,
                output: vec![0; OUTPUT_CAPACITY],
                stream: Method4::default(),
                registers: [0; 0x90 / 4],
                clock: [0; 0x40 / 4],
                writes: Vec::new(),
                reset_ready: false,
                count: 0,
                completions_after: 0,
                remaining: 0,
                status_reads: 0,
            }
        }

        fn register(&mut self, register: EngineRegister) -> &mut u32 {
            &mut self.registers[register as usize / 4]
        }

        fn clock(&mut self, register: ClockRegister) -> &mut u32 {
            &mut self.clock[register as usize / 4]
        }

        fn start(&mut self) {
            let source = self.registers[EngineRegister::Source as usize / 4] as usize - SOURCE_BASE as usize;
            let length = (self.registers[EngineRegister::Length as usize / 4] & 0x7ffff) as usize;
            let before = self.stream.produced();
            self.stream
                .feed(&self.source[source..source + length], &mut self.output)
                .unwrap();
            self.count = (self.stream.produced() - before) as u32;
            self.remaining = self.completions_after;
            *self.register(EngineRegister::Status) = u32::from(self.remaining == 0);
            *self.register(EngineRegister::OutputCount) = self.count;
        }
    }

    impl Bus for Device {
        fn read(&mut self, register: Register) -> u32 {
            match register {
                Register::Engine(EngineRegister::Reset) => u32::from(self.reset_ready),
                Register::Engine(EngineRegister::Status) => {
                    self.status_reads += 1;
                    if self.remaining != 0 {
                        self.remaining -= 1;
                        if self.remaining == 0 {
                            *self.register(EngineRegister::Status) = 1;
                        }
                    }
                    *self.register(EngineRegister::Status)
                }
                Register::Engine(register) => *self.register(register),
                Register::Clock(register) => *self.clock(register),
            }
        }

        fn write(&mut self, register: Register, value: u32) {
            self.writes.push((register.address(), value));
            match register {
                Register::Engine(EngineRegister::Reset) => {
                    self.reset_ready = true;
                    self.stream = Method4::default();
                }
                Register::Engine(EngineRegister::Start) => self.start(),
                Register::Engine(EngineRegister::Status) => {
                    *self.register(EngineRegister::Status) &= !value;
                }
                Register::Engine(register) => *self.register(register) = value,
                Register::Clock(register) => *self.clock(register) = value,
            }
        }
    }

    fn record() -> Vec<u8> {
        let decoded = b"abcabcabcabcabcabcde";
        let mut record = vec![0; 20];
        record[0..4].copy_from_slice(&(decoded.len() as u32).to_le_bytes());
        record[4..8].copy_from_slice(&(29u32).to_le_bytes());
        record[8..12].copy_from_slice(&1u32.to_le_bytes());
        record[12..16].copy_from_slice(&PAGE_BYTES.to_le_bytes());
        record[16..20].copy_from_slice(&4u32.to_le_bytes());
        record.extend_from_slice(&[3, b'a', b'b', b'c', 0x80, 0x3f, 2, b'd', b'e']);
        record
    }

    #[test]
    fn driver_writes_stock_registers_in_order_and_decodes_continuously() {
        let record = record();
        let decoded = b"abcabcabcabcabcabcde";
        let mut device = Device::new(&record);
        device.clock[0] = 0x1234_567f;
        device.clock[ClockRegister::Enable as usize / 4] = 0;
        let mut session = Session::open(device);
        let end = feed_end(record.len() as u32, record.len() as u32, PAGE_BYTES).unwrap();
        assert_eq!(end, record.len() as u32 - 20);
        let count = session
            .submit(Source(SOURCE_BASE + 0x14), Destination(DEST_BASE), end)
            .unwrap();
        assert_eq!(count as usize, decoded.len());
        let device = session.close();
        assert_eq!(&device.output[..decoded.len()], decoded);
        assert_eq!(device.status_reads, 1);
        let engine = |register| ENGINE_BASE + register as u32;
        let clock = |register| CLOCK_BASE + register as u32;
        assert_eq!(
            device.writes,
            vec![
                (clock(ClockRegister::Gate), 0x1234_5678),
                (clock(ClockRegister::Enable), 4),
                (engine(EngineRegister::Enable), 0),
                (engine(EngineRegister::Reset), 0),
                (engine(EngineRegister::Status), STATUS_CLEAR),
                (engine(EngineRegister::Source), SOURCE_BASE + 0x14),
                (engine(EngineRegister::Length), record.len() as u32 - 20),
                (engine(EngineRegister::Destination), DEST_BASE),
                (engine(EngineRegister::Control), CONTROL),
                (engine(EngineRegister::Enable), 1),
                (engine(EngineRegister::Start), 2),
                (engine(EngineRegister::Status), 1),
                (engine(EngineRegister::Enable), 0),
                (engine(EngineRegister::Status), STATUS_CLEAR),
                (clock(ClockRegister::Enable), 0),
                (clock(ClockRegister::Gate), 0x1234_567f),
            ]
        );
    }

    #[test]
    fn feed_boundaries_hold_back_the_header_and_partial_pages() {
        let record_length = 0x146567;
        assert_eq!(feed_end(0x20000, record_length, PAGE_BYTES), Ok(0x1ffec));
        assert_eq!(feed_end(0x40000, record_length, PAGE_BYTES), Ok(0x3ffec));
        assert_eq!(feed_end(record_length, record_length, PAGE_BYTES), Ok(0x146553));
        assert_eq!(feed_end(0x100, record_length, PAGE_BYTES), Err(EngineError::Length));
        assert_eq!(feed_end(0x20000, record_length, 0), Err(EngineError::Page));
    }

    #[test]
    fn wait_polls_until_the_device_completes() {
        let record = record();
        let mut device = Device::new(&record);
        device.completions_after = 3;
        let mut session = Session::open(device);
        let end = feed_end(record.len() as u32, record.len() as u32, PAGE_BYTES).unwrap();
        let count = session
            .submit(Source(SOURCE_BASE + 0x14), Destination(DEST_BASE), end)
            .unwrap();
        assert_eq!(count as usize, 20);
        let device = session.close();
        assert_eq!(device.status_reads, 3);
    }

    #[test]
    fn fault_status_refuses_completion() {
        struct FaultBus;
        impl Bus for FaultBus {
            fn read(&mut self, register: Register) -> u32 {
                match register {
                    Register::Engine(EngineRegister::Reset) => 1,
                    Register::Engine(EngineRegister::Status) => 0x101,
                    _ => 0,
                }
            }
            fn write(&mut self, _: Register, _: u32) {}
        }
        let mut session = Session::open(FaultBus);
        assert_eq!(
            session.submit(Source(SOURCE_BASE), Destination(DEST_BASE), 16),
            Err(EngineError::Fault(0x101))
        );
    }
}
