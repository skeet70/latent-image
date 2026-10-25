//! The retained engine's SD0 host and declared blank SDHC card
//! (`emulator/sd_card.py`, `SDCardController`). Card identity, capacity and
//! transfer latency are declared, unmeasured inputs; MMC, UHS/tuning and
//! erase refuse. The `0xfff5002c` clock word keeps the law of the plan's
//! former `sd_clocks` node, FujiMod's `sdhci::Clocks`: stable after
//! `clock_ticks`, bits 3-5 refused.
//!
//! Not ported: the retained host's check of the interrupt controller's
//! line-78 programming, and its media-failure controls.

use crate::run::NodeName;
use fujimod_emulator_devices::exr2::gpio::Pads;
use fujimod_emulator_devices::graph::Mapping;
use fujimod_emulator_devices::parts::pins::{InputPins, Pin};
use fujimod_emulator_devices::parts::sd_card::{self, Card, SECTOR};
use fujimod_emulator_devices::{Cx, Device};
use fujimod_emulator_exr2_plan::{Adapter, Resources};
use fujimod_emulator_ip::sdhci::Clocks;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

pub const IMPLEMENTATION: &str = "xm1-retained-sd-host";
const BASE: u64 = 0xfff5_0000;
/// The three catalog windows: registers, status/enables, vendor words.
const WINDOWS: [(u64, u64); 3] = [(BASE, 0x30), (BASE + 0x30, 0x10), (BASE + 0x100, 8)];
const CLOCK: u64 = 0x2c;
/// Every register address the host models, at any admitted width.
pub const REGISTERS: [u32; 29] = {
    const OFFSETS: [u32; 29] = [
        0, 4, 6, 8, 0xa, 0xc, 0xe, 0x10, 0x12, 0x14, 0x16, 0x18, 0x1a, 0x1c, 0x1e, 0x20, 0x24, 0x28, 0x2c, 0x2e,
        0x30, 0x32, 0x34, 0x36, 0x38, 0x3a, 0x3e, 0x100, 0x104,
    ];
    let mut registers = [0; 29];
    let mut index = 0;
    while index < OFFSETS.len() {
        registers[index] = BASE as u32 + OFFSETS[index];
        index += 1;
    }
    registers
};
const HISTORY: usize = 32;

fn halfword(offset: u64) -> bool {
    matches!(offset, 4 | 6 | 8 | 10 | 12 | 14 | 0x28 | 0x2e | 0x30 | 0x32 | 0x34 | 0x36 | 0x38 | 0x3a | 0x3e | 0x100 | 0x104)
        || (16..32).contains(&offset) && offset % 2 == 0
}

fn word(offset: u64) -> bool {
    matches!(offset, 0 | 12 | 0x20 | 0x24)
}

fn command_word(command: u8) -> Option<u32> {
    Some(match command {
        0 => 0,
        2 => 0x0209_0000,
        3 => 0x031a_0000,
        6 => 0x063a_0011,
        7 => 0x071b_0000,
        8 => 0x081a_0000,
        9 => 0x0909_0000,
        12 => 0x0cdb_0000,
        13 => 0x0d1a_0000,
        16 => 0x101a_0000,
        18 => 0x123a_0037,
        25 => 0x193a_0027,
        55 => 0x371a_0000,
        _ => return None,
    })
}

fn application_word(command: u8) -> Option<u32> {
    Some(match command {
        6 => 0x061a_0000,
        13 => 0x0d3a_0011,
        41 => 0x2902_0000,
        42 => 0x2a1a_0000,
        51 => 0x333a_0011,
        _ => return None,
    })
}

/// The interrupt the host raises on its owner core.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Irq {
    core: usize,
    line: u32,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    irq: Irq,
    /// Card detect, low when a card is present.
    detect: Pin,
    /// Write protect, high when the card is protected.
    protect: Pin,
    ram_bytes: u64,
    transfer_ticks: u64,
    clock_ticks: u64,
    sectors: u32,
    max_dirty_sectors: usize,
    rca: u16,
}

/// Where a retained card's sectors came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaImage {
    pub path: String,
    pub sha256: String,
    pub sectors: u64,
    pub nonzero_sectors: u64,
}

#[derive(Clone)]
pub struct RetainedCard {
    pub card: Card,
    pub image: MediaImage,
}

#[derive(Clone, Debug)]
pub struct Contacts {
    pub gpio: NodeName,
    pub detect: Pin,
    pub protect: Pin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum CardState {
    Idle,
    Ready,
    Identification,
    Standby,
    Transfer,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
enum Response {
    Short(u32),
    Long([u8; 16]),
}

impl Response {
    fn bytes(&self) -> [u8; 16] {
        match self {
            Response::Short(value) => {
                let mut bytes = [0; 16];
                bytes[..4].copy_from_slice(&value.to_le_bytes());
                bytes
            }
            Response::Long(bytes) => *bytes,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
enum Data {
    None,
    Metadata(Vec<u8>),
    Sectors { first: u32, count: u32, write: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum PioState {
    CommandPending,
    BufferReady,
    SectorPending,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Pio {
    state: PioState,
    /// Words fed for a write sector, or the words of a read sector not yet taken.
    buffer: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Completion {
    Transfer,
    PioWriteSector,
    PioReadSector,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Request {
    word: u32,
    command: u8,
    application: bool,
    argument: u32,
    response: Response,
    next_state: CardState,
    next_app: bool,
    block_length: u32,
    bus_width: u8,
    data: Data,
    pio: Option<Pio>,
    clock: u16,
    quantity: u32,
    cursor: u32,
    dma_start: u32,
    boundary_wait: bool,
    due: Option<(u64, Completion)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Finished {
    command: u8,
    application: bool,
    argument: u32,
    word: u32,
    quantity: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Counters {
    started: u64,
    completed: u64,
    irqs: u64,
    dma_boundaries: u64,
}

/// Halfword register storage indexed by offset / 2, through `+0x106`.
#[derive(Clone, Serialize, Deserialize)]
struct Registers(#[serde(with = "fujimod_emulator_devices::checkpoint::array")] [u16; 132]);

impl Registers {
    fn get(&self, offset: u64) -> u16 {
        self.0[(offset / 2) as usize]
    }

    fn set(&mut self, offset: u64, value: u16) {
        self.0[(offset / 2) as usize] = value;
    }

    fn word(&self, offset: u64) -> u32 {
        u32::from(self.get(offset)) | u32::from(self.get(offset + 2)) << 16
    }

    fn put_word(&mut self, offset: u64, value: u32) {
        self.set(offset, value as u16);
        self.set(offset + 2, (value >> 16) as u16);
    }

    fn or(&mut self, offset: u64, bits: u16) {
        self.set(offset, self.get(offset) | bits);
    }
}

pub struct SdHost {
    options: Options,
    contacts: Box<dyn InputPins>,
    gpio: NodeName,
    card: Card,
    image: Option<MediaImage>,
    regs: Registers,
    /// The guest-written `+0x2c` control, without the stable bit.
    clock: u16,
    clocks: Clocks,
    state: CardState,
    block_length: u32,
    bus_width: u8,
    application_pending: bool,
    request: Option<Request>,
    response: [u8; 16],
    counters: Counters,
    history: Vec<Finished>,
}

impl SdHost {
    fn new(options: Options, contacts: Box<dyn InputPins>, gpio: NodeName) -> Result<Self, String> {
        if options.transfer_ticks == 0 || options.rca == 0 || options.ram_bytes == 0 || options.max_dirty_sectors > 65536 {
            return Err("SD host requires a positive latency, RCA and DRAM extent and at most 65536 dirty sectors".into());
        }
        options.detect.validate()?;
        options.protect.validate()?;
        if options.detect == options.protect {
            return Err("SD detect and protect must use distinct pads".into());
        }
        let card = Card::new(sd_card::Options { sectors: options.sectors, max_dirty_sectors: options.max_dirty_sectors })?;
        let clocks = Clocks::new(&[(BASE + CLOCK, options.clock_ticks)])?;
        Ok(Self {
            options,
            contacts,
            gpio,
            card,
            image: None,
            regs: Registers([0; 132]),
            clock: 0,
            clocks,
            state: CardState::Idle,
            block_length: 512,
            bus_width: 1,
            application_pending: false,
            request: None,
            response: [0; 16],
            counters: Counters::default(),
            history: Vec::new(),
        })
    }

    /// Replaces the medium of an unexecuted host with a retained card.
    pub fn insert(&mut self, retained: RetainedCard) -> Result<(), String> {
        if self.request.is_some() || retained.card.sectors() != self.options.sectors {
            return Err("retained SD medium differs from the declared card or the host is busy".into());
        }
        self.card = retained.card;
        self.image = Some(retained.image);
        Ok(())
    }

    pub fn contacts(&self) -> Contacts {
        Contacts { gpio: self.gpio.clone(), detect: self.options.detect, protect: self.options.protect }
    }

    pub fn card(&self) -> &Card {
        &self.card
    }

    pub fn present(&self) -> Result<bool, String> {
        Ok(!self.contacts.level(self.options.detect)?)
    }

    fn protected(&self) -> Result<bool, String> {
        self.contacts.level(self.options.protect)
    }

    fn physical(&self, address: u32, length: u32) -> Result<u64, String> {
        let address = u64::from(address);
        let physical = if (0x4000_0000..0x6000_0000).contains(&address) { address & 0x3fff_ffff } else { address };
        if address & 3 != 0 || !(1..=0x200_0000).contains(&length) || physical + u64::from(length) > self.options.ram_bytes {
            return Err("SD DMA address/quantity exceeds aligned modeled DRAM".into());
        }
        Ok(physical)
    }

    fn r1(&self, application: bool) -> u32 {
        let state = self.state as u32;
        state << 9 | if self.state == CardState::Transfer { 0x100 } else { 0 } | if application { 0x20 } else { 0 }
    }

    fn csd(&self) -> [u8; 16] {
        let value = 1u128 << 126
            | 0x0e << 112
            | 0x32 << 96
            | 0x515 << 84
            | 9 << 80
            | u128::from(self.card.sectors() / 1024 - 1) << 48
            | 1 << 46
            | 0x7f << 39
            | 9 << 22
            | 1;
        (value >> 8).to_le_bytes()
    }

    fn signal(&mut self, cx: &mut Cx<'_>) -> Result<(), String> {
        let normal = self.regs.get(0x30) & self.regs.get(0x34) & self.regs.get(0x38);
        let error = self.regs.get(0x32) & self.regs.get(0x36) & self.regs.get(0x3a);
        if normal != 0 || error != 0 {
            cx.raise_irq(self.options.irq.core, self.options.irq.line)?;
            self.counters.irqs += 1;
        }
        Ok(())
    }

    fn plan(&self, word: u32, argument: u32) -> Result<Request, String> {
        let command = (word >> 24 & 0x3f) as u8;
        let application = self.application_pending && command != 55;
        let expected = if application { application_word(command) } else { command_word(command) };
        let pio = !application && matches!((command, word), (25, 0x193a_0026) | (18, 0x123a_0036));
        if expected != Some(word) && !pio {
            let kind = if application { "ACMD" } else { "CMD" };
            return Err(format!("Unsupported SD {kind}{command} word {word:#x}"));
        }
        let rca = u32::from(self.options.rca) << 16;
        let mut response = Response::Short(self.r1(false));
        let mut next_state = self.state;
        let mut next_app = false;
        let mut block_length = self.block_length;
        let mut bus_width = self.bus_width;
        let mut data = Data::None;
        let refuse = |reason: &str| Err::<Request, String>(reason.to_owned());
        match command {
            0 => {
                if argument != 0 {
                    return refuse("SD CMD0 requires argument0");
                }
                response = Response::Short(0);
                next_state = CardState::Idle;
                block_length = 512;
                bus_width = 1;
            }
            8 => {
                if self.state != CardState::Idle || argument != 0x1aa {
                    return refuse("SDHC CMD8 requires idle/voltage-pattern1AA");
                }
                response = Response::Short(0x1aa);
            }
            55 => {
                if ![0, rca].contains(&argument)
                    || !matches!(self.state, CardState::Idle | CardState::Standby | CardState::Transfer)
                {
                    return refuse("SD CMD55 has unsupported RCA/card state");
                }
                response = Response::Short(self.r1(true));
                next_app = true;
            }
            41 if application => {
                if self.state != CardState::Idle || argument != 0x4110_0000 {
                    return refuse("SDHC ACMD41 requires recovered HCS/OCR request");
                }
                response = Response::Short(0xc010_0000);
                next_state = CardState::Ready;
            }
            2 => {
                if self.state != CardState::Ready || argument != 0 {
                    return refuse("SD CMD2 requires ready/argument0");
                }
                let cid = u128::from_be_bytes(*b"\xfeMCVIRT0\x10\x12\x34\x56\x78\x01\xaa\xff");
                response = Response::Long((cid >> 8).to_le_bytes());
                next_state = CardState::Identification;
            }
            3 => {
                if self.state != CardState::Identification || argument != 0 {
                    return refuse("SD CMD3 requires identification/argument0");
                }
                response = Response::Short(rca | 3 << 9);
                next_state = CardState::Standby;
            }
            9 => {
                if self.state != CardState::Standby || argument != rca {
                    return refuse("SD CMD9 requires standby/assigned RCA");
                }
                response = Response::Long(self.csd());
            }
            7 => {
                if !matches!(self.state, CardState::Standby | CardState::Transfer) || ![0, rca].contains(&argument) {
                    return refuse("SD CMD7 requires assigned RCA or deselection");
                }
                next_state = if argument != 0 { CardState::Transfer } else { CardState::Standby };
                response = Response::Short(if argument != 0 { 0x900 } else { 0x600 });
            }
            _ if self.state != CardState::Transfer => return refuse("SD selected-card command before CMD7"),
            16 => {
                if ![8, 64, 512].contains(&argument) {
                    return refuse("SD recovered block lengths are8/64/512");
                }
                block_length = argument;
            }
            42 if application => {
                if argument != 0 {
                    return refuse("SD ACMD42 only disconnects card-detect pull-up");
                }
            }
            6 if application => {
                if argument != 2 {
                    return refuse("SD ACMD6 only selects recovered four-bit mode");
                }
                bus_width = 4;
            }
            13 if application => {
                if argument != 0 || self.block_length != 64 || self.bus_width != 4 {
                    return refuse("SD ACMD13 requires four-bit/64-byte status request");
                }
                let mut status = vec![0; 64];
                status[0] = 0x80;
                status[10] = 0x70;
                data = Data::Metadata(status);
            }
            51 if application => {
                if argument != 0 || self.block_length != 8 {
                    return refuse("SD ACMD51 requires eight-byte SCR request");
                }
                data = Data::Metadata(vec![2, 5, 0, 0, 0, 0, 0, 0]);
            }
            6 => {
                if argument != 1 || self.block_length != 64 {
                    return refuse("SD CMD6 only supports default-speed capability check");
                }
                let mut switch = vec![0; 64];
                switch[13] = 1;
                data = Data::Metadata(switch);
            }
            18 | 25 => {
                if self.block_length != 512 {
                    return refuse("SD sector transaction before restored512-byte block length");
                }
                if command == 25 && self.protected()? {
                    return refuse("SD sector write requires an unprotected declared card");
                }
                let count = match self.regs.get(6) {
                    0 => 65536,
                    count => u32::from(count),
                };
                self.card.range(argument, count)?;
                data = Data::Sectors { first: argument, count, write: command == 25 };
            }
            12 => {
                if argument != 0 {
                    return refuse("SD CMD12 requires argument0");
                }
            }
            13 => {
                if argument != rca {
                    return refuse("SD CMD13 requires assigned RCA");
                }
            }
            _ => return refuse("SD command class not implemented"),
        }
        Ok(Request {
            word,
            command,
            application,
            argument,
            response,
            next_state,
            next_app,
            block_length,
            bus_width,
            data,
            pio: pio.then(|| Pio { state: PioState::CommandPending, buffer: Vec::new() }),
            clock: 0,
            quantity: 0,
            cursor: 0,
            dma_start: 0,
            boundary_wait: false,
            due: None,
        })
    }

    fn schedule(&self, cx: &mut Cx<'_>, request: &mut Request, completion: Completion) -> Result<(), String> {
        let at = cx.now().checked_add(self.options.transfer_ticks).ok_or("SD deadline overflow")?;
        request.due = Some((at, completion));
        cx.schedule(at, 0);
        Ok(())
    }

    fn start(&mut self, cx: &mut Cx<'_>, word: u32) -> Result<(), String> {
        if !self.present()? || self.request.is_some() {
            return Err("SD command while absent or prior transaction pending".into());
        }
        if self.clock & 5 != 5 || self.regs.get(0x28) & 0x100 == 0 {
            return Err("SD command before stock clock and host-power setup".into());
        }
        let argument = self.regs.word(8);
        let mut request = self.plan(word, argument)?;
        let data = !matches!(request.data, Data::None);
        let expected_status = if data { 2 } else { 1 };
        if data && self.regs.get(0x34) & self.regs.get(0x38) & expected_status == 0
            || self.regs.get(0x30) != 0
            || self.regs.get(0x32) != 0
        {
            return Err("SD command requires cleared status and enabled completion signal".into());
        }
        let quantity = match &request.data {
            Data::None => 0,
            Data::Metadata(bytes) => bytes.len() as u32,
            Data::Sectors { count, .. } => count * SECTOR as u32,
        };
        if data {
            let sectors = matches!(request.data, Data::Sectors { .. });
            if u32::from(self.regs.get(4) & 0xfff) != if sectors { SECTOR as u32 } else { quantity } {
                return Err("SD block-size differs from the command payload".into());
            }
            if self.regs.get(0x28) & 0x18 != 0 {
                return Err("SD DMA mode is not the recovered SDMA path".into());
            }
            let address = self.regs.word(0);
            if request.pio.is_some() {
                let signals = if request.command == 25 { 0x13 } else { 0x23 };
                if self.regs.get(0x34) & self.regs.get(0x38) & signals != signals {
                    return Err("SD PIO requires command/data/directional buffer-ready signals".into());
                }
                if let Data::Sectors { first, count, write: true } = request.data {
                    self.card
                        .reserve_write(first, count)
                        .map_err(|_| "SD PIO write exceeds reserved sparse dirty-sector bound".to_owned())?;
                }
            } else {
                self.physical(address, quantity)?;
                let boundary = 4096u32 << (self.regs.get(4) >> 12 & 7);
                if quantity > boundary - address % boundary && self.regs.get(0x34) & self.regs.get(0x38) & 8 == 0 {
                    return Err("SDMA boundary crossing requires enabled DMA interrupt".into());
                }
            }
            if let (Data::Sectors { first, write: true, .. }, None) = (&request.data, &request.pio) {
                let alignment = if address & 0x4000_0000 != 0 { 7 } else { 31 };
                if address & alignment != 0 {
                    return Err("SD sector-write address is outside stock SDMA alignment".into());
                }
                let source = cx.ram_bytes(self.physical(address, quantity)?, quantity as usize)?;
                self.preflight_write(*first, &source)?;
            }
        }
        request.clock = self.clock;
        request.quantity = quantity;
        request.dma_start = self.regs.word(0);
        self.schedule(cx, &mut request, Completion::Transfer)?;
        self.request = Some(request);
        self.regs.set(12, word as u16);
        self.regs.set(14, (word >> 16) as u16);
        self.counters.started += 1;
        Ok(())
    }

    /// Refuses a whole SDMA write whose sectors would exceed the dirty-sector bound.
    fn preflight_write(&self, first: u32, source: &[u8]) -> Result<(), String> {
        let count = (source.len() / SECTOR) as u32;
        let inside = self.card.nonzero_sectors().filter(|(lba, _)| (first..first + count).contains(lba)).count();
        let written = source.chunks(SECTOR).filter(|block| block.iter().any(|b| *b != 0)).count();
        if self.card.dirty_sectors() - inside + written > self.options.max_dirty_sectors {
            return Err("SD sparse dirty-sector bound exceeded".into());
        }
        Ok(())
    }

    fn check(&self, request: &Request) -> Result<(), String> {
        if !self.present()? {
            return Err("SD card/profile changed during an active transaction".into());
        }
        if self.clock != request.clock {
            return Err("SD clock changed during an active transaction".into());
        }
        if request.command == 25 && self.protected()? {
            return Err("SD write became protected during an active transaction".into());
        }
        Ok(())
    }

    fn complete(&mut self, cx: &mut Cx<'_>) -> Result<(), String> {
        let Some(mut request) = self.request.take() else {
            return Ok(());
        };
        let Some((_, completion)) = request.due.filter(|(at, _)| *at <= cx.now()) else {
            self.request = Some(request);
            return Ok(());
        };
        request.due = None;
        let checked = self.check(&request);
        if let Err(error) = checked {
            self.request = Some(request);
            return Err(error);
        }
        match completion {
            Completion::Transfer => self.transfer(cx, request),
            Completion::PioWriteSector => self.pio_write_sector(cx, request),
            Completion::PioReadSector => self.pio_read_sector(cx, request),
        }
    }

    fn sectors_first(request: &Request) -> u32 {
        match request.data {
            Data::Sectors { first, .. } => first,
            _ => request.argument,
        }
    }

    fn transfer(&mut self, cx: &mut Cx<'_>, mut request: Request) -> Result<(), String> {
        if let Some(pio) = request.pio.as_mut() {
            self.response = request.response.bytes();
            pio.state = PioState::BufferReady;
            if request.command == 18 {
                let mut sector = vec![0; SECTOR];
                self.card.read_bytes(u64::from(request.argument) * SECTOR as u64, &mut sector)?;
                pio.buffer = sector;
            }
            self.regs.or(0x30, if request.command == 25 { 0x11 } else { 0x21 });
            self.request = Some(request);
            return self.signal(cx);
        }
        let (quantity, cursor) = (request.quantity, request.cursor);
        let mut end = quantity;
        if quantity != 0 {
            let boundary = 4096u32 << (self.regs.get(4) >> 12 & 7);
            let address = request.dma_start.wrapping_add(cursor);
            end = quantity.min(cursor + boundary - address % boundary);
            let length = end - cursor;
            let physical = self.physical(address, length)?;
            let mut bytes = vec![0; length as usize];
            let offset = u64::from(Self::sectors_first(&request)) * SECTOR as u64 + u64::from(cursor);
            match &request.data {
                Data::Sectors { write: true, .. } => {
                    cx.ram_read(physical, &mut bytes)?;
                    self.card.write_bytes(offset, &bytes)?;
                }
                Data::Sectors { write: false, .. } => {
                    self.card.read_bytes(offset, &mut bytes)?;
                    cx.ram_write(physical, &bytes)?;
                }
                Data::Metadata(payload) => {
                    bytes.copy_from_slice(&payload[cursor as usize..end as usize]);
                    cx.ram_write(physical, &bytes)?;
                }
                Data::None => unreachable!("a quantity implies data"),
            }
            request.cursor = end;
            self.regs.put_word(0, request.dma_start.wrapping_add(end));
            if matches!(request.command, 18 | 25) {
                self.regs.set(6, (quantity - end).div_ceil(SECTOR as u32) as u16);
            }
        }
        self.response = request.response.bytes();
        if end < quantity {
            self.regs.or(0x30, 9);
            request.boundary_wait = true;
            self.counters.dma_boundaries += 1;
            self.request = Some(request);
            return self.signal(cx);
        }
        let busy_response = request.word >> 16 & 3 == 3;
        self.finish(cx, request, if quantity != 0 || busy_response { 3 } else { 1 })
    }

    fn finish(&mut self, cx: &mut Cx<'_>, request: Request, status: u16) -> Result<(), String> {
        self.state = request.next_state;
        self.application_pending = request.next_app;
        self.block_length = request.block_length;
        self.bus_width = request.bus_width;
        if matches!(request.command, 18 | 25) {
            self.regs.set(6, 0);
        }
        self.regs.or(0x30, status);
        self.counters.completed += 1;
        if self.history.len() == HISTORY {
            self.history.remove(0);
        }
        self.history.push(Finished {
            command: request.command,
            application: request.application,
            argument: request.argument,
            word: request.word,
            quantity: request.quantity,
        });
        self.signal(cx)
    }

    fn pio_write_sector(&mut self, cx: &mut Cx<'_>, mut request: Request) -> Result<(), String> {
        let first = u64::from(request.argument + request.cursor / SECTOR as u32);
        let pio = request.pio.as_mut().ok_or("SD PIO sector without a PIO request")?;
        self.card.write_bytes(first * SECTOR as u64, &pio.buffer)?;
        pio.buffer.clear();
        request.cursor += SECTOR as u32;
        self.regs.set(6, ((request.quantity - request.cursor) / SECTOR as u32) as u16);
        if request.cursor == request.quantity {
            return self.finish(cx, request, 2);
        }
        pio.state = PioState::BufferReady;
        self.regs.or(0x30, 0x10);
        self.request = Some(request);
        self.signal(cx)
    }

    fn pio_read_sector(&mut self, cx: &mut Cx<'_>, mut request: Request) -> Result<(), String> {
        self.regs.set(6, ((request.quantity - request.cursor) / SECTOR as u32) as u16);
        if request.cursor == request.quantity {
            return self.finish(cx, request, 2);
        }
        let mut sector = vec![0; SECTOR];
        let first = u64::from(request.argument + request.cursor / SECTOR as u32);
        self.card.read_bytes(first * SECTOR as u64, &mut sector)?;
        let pio = request.pio.as_mut().ok_or("SD PIO sector without a PIO request")?;
        pio.buffer = sector;
        pio.state = PioState::BufferReady;
        self.regs.or(0x30, 0x20);
        self.request = Some(request);
        self.signal(cx)
    }

    /// The PIO request of `command` with a ready buffer the owner core has acknowledged.
    fn ready_pio(&self, cx: &Cx<'_>, command: u8, ready: u16) -> Result<(), String> {
        let ready_request = self.request.as_ref().is_some_and(|request| {
            request.command == command && request.pio.as_ref().is_some_and(|pio| pio.state == PioState::BufferReady)
        });
        if !ready_request || cx.cpu != self.options.irq.core || self.regs.get(0x30) & ready != 0 {
            let direction = if command == 25 { "word" } else { "read" };
            return Err(format!("SD PIO {direction} requires its ready owner and real buffer-status ACK"));
        }
        Ok(())
    }

    fn pio_word(&mut self, cx: &mut Cx<'_>, value: u32) -> Result<(), String> {
        self.ready_pio(cx, 25, 0x10)?;
        let mut request = self.request.take().ok_or("SD PIO without request")?;
        let result = (|| {
            self.check(&request)?;
            let pio = request.pio.as_mut().ok_or("SD PIO without a PIO request")?;
            if pio.buffer.len() >= SECTOR || request.cursor >= request.quantity {
                return Err("SD PIO word exceeds its finite sector count".to_owned());
            }
            pio.buffer.extend_from_slice(&value.to_le_bytes());
            if pio.buffer.len() == SECTOR {
                pio.state = PioState::SectorPending;
                self.schedule(cx, &mut request, Completion::PioWriteSector)?;
            }
            Ok(())
        })();
        self.request = Some(request);
        result
    }

    fn pio_read_word(&mut self, cx: &mut Cx<'_>) -> Result<u32, String> {
        self.ready_pio(cx, 18, 0x20)?;
        let mut request = self.request.take().ok_or("SD PIO without request")?;
        let result = (|| {
            self.check(&request)?;
            let pio = request.pio.as_mut().ok_or("SD PIO without a PIO request")?;
            if pio.buffer.len() < 4 || request.cursor >= request.quantity {
                return Err("SD PIO read exceeds its finite sector count".to_owned());
            }
            let value = u32::from_le_bytes([pio.buffer[0], pio.buffer[1], pio.buffer[2], pio.buffer[3]]);
            pio.buffer.drain(..4);
            request.cursor += 4;
            if pio.buffer.is_empty() {
                pio.state = PioState::SectorPending;
                self.schedule(cx, &mut request, Completion::PioReadSector)?;
            }
            Ok(value)
        })();
        self.request = Some(request);
        result
    }

    fn present_state(&self) -> Result<u32, String> {
        let (inhibit, ready) = match &self.request {
            None => (0, 0),
            Some(request) => {
                let inhibit = 1 | if request.quantity != 0 || matches!(request.command, 7 | 12) { 2 } else { 0 };
                let ready = match &request.pio {
                    Some(pio) if pio.state == PioState::BufferReady => 1 << if request.command == 25 { 10 } else { 11 },
                    _ => 0,
                };
                (inhibit, ready)
            }
        };
        Ok(1 << 17 | if self.present()? { 1 << 16 } else { 0 } | inhibit | ready)
    }

    fn load(&mut self, cx: &mut Cx<'_>, offset: u64, size: u8) -> Result<u32, String> {
        Ok(match offset {
            0x24 => self.present_state()?,
            16..32 => u32::from(u16::from_le_bytes([self.response[(offset - 16) as usize], self.response[(offset - 15) as usize]])),
            0x20 => self.pio_read_word(cx)?,
            0 => self.regs.word(0),
            12 if size == 4 => self.regs.word(12),
            _ => u32::from(self.regs.get(offset)),
        })
    }

    fn store(&mut self, cx: &mut Cx<'_>, offset: u64, size: u8, value: u32) -> Result<(), String> {
        if offset == 0x24 || (16..32).contains(&offset) {
            return Err("SD present-state/response registers are read-only".into());
        }
        if offset == 0x20 {
            return self.pio_word(cx, value);
        }
        if matches!(offset, 0x30 | 0x32) {
            self.regs.set(offset, self.regs.get(offset) & !(value as u16));
            return Ok(());
        }
        if offset == 0x2e {
            if value & !0x70f != 0 {
                return Err("SD timeout/reset contains unsupported bits".into());
            }
            if value & 0x700 != 0 {
                self.request = None;
                self.regs.set(0x30, 0);
                self.regs.set(0x32, 0);
            }
            self.regs.set(0x2e, (value & 0xf) as u16);
            return Ok(());
        }
        if offset == 12 && size == 2 {
            if self.request.is_some() {
                return Err("SD transfer mode changed during a command".into());
            }
            self.regs.set(12, value as u16);
            return Ok(());
        }
        if offset == 12 || offset == 14 {
            let word = if size == 4 { value } else { u32::from(self.regs.get(12)) | value << 16 };
            return self.start(cx, word);
        }
        if let (0, Some(request)) = (offset, self.request.as_ref()) {
            if !request.boundary_wait || value != self.regs.word(0) || self.regs.get(0x30) & 8 != 0 {
                return Err("SDMA resume requires acknowledged boundary/current DMA address".into());
            }
            let mut request = self.request.take().ok_or("SDMA resume without request")?;
            let result = self.check(&request).and_then(|()| {
                request.boundary_wait = false;
                self.schedule(cx, &mut request, Completion::Transfer)
            });
            self.request = Some(request);
            return result;
        }
        if self.request.is_some() {
            return Err("SD command configuration changed while transaction pending".into());
        }
        if offset == 0x28 && value & !0xfff != 0 {
            return Err("SD host control contains unsupported bits".into());
        }
        if offset == 4 && value & 0x8000 != 0 {
            return Err("SD block-size contains an unsupported reserved bit".into());
        }
        if offset == 0x3e && value != 0 || offset == 0x100 && value & !7 != 0 || offset == 0x104 && value != 0 {
            return Err("SD UHS/tuning/unknown vendor mode is not supported".into());
        }
        if size == 4 {
            self.regs.put_word(offset, value);
        } else {
            self.regs.set(offset, value as u16);
        }
        Ok(())
    }

    fn admitted(offset: u64, size: u8) -> bool {
        size == 2 && halfword(offset) || size == 4 && word(offset)
    }
}

impl Device for SdHost {
    fujimod_emulator_devices::checkpoint_fields!(
        card: Card,
        image: Option<MediaImage>,
        regs: Registers,
        clock: u16,
        clocks: Clocks,
        state: CardState,
        block_length: u32,
        bus_width: u8,
        application_pending: bool,
        request: Option<Request>,
        response: [u8; 16],
        counters: Counters,
        history: Vec<Finished>,
    );

    fn fingerprint(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({
            "card": self.card.fingerprint(), "image": self.image, "registers": self.regs.0.as_slice(),
            "clock": self.clock, "clocks": self.clocks.save(), "state": self.state, "block_length": self.block_length, "bus_width": self.bus_width,
            "application_pending": self.application_pending, "request": self.request, "response": self.response,
            "counters": self.counters, "history": self.history,
        }))
    }

    fn name(&self) -> &'static str {
        IMPLEMENTATION
    }

    fn read(&mut self, cx: &mut Cx<'_>, address: u64, size: u8) -> u64 {
        let offset = address.wrapping_sub(BASE);
        if offset == CLOCK && size == 2 {
            return self.clocks.read(cx, address, size);
        }
        if !Self::admitted(offset, size) {
            cx.unsupported(format!("Unknown SD0 register/access {address:#x}/{size}"));
            return 0;
        }
        self.advance(cx);
        if cx.refusal().is_some() {
            return 0;
        }
        match self.load(cx, offset, size) {
            Ok(value) => u64::from(value),
            Err(error) => {
                cx.fail(error);
                0
            }
        }
    }

    fn write(&mut self, cx: &mut Cx<'_>, address: u64, size: u8, value: u64) {
        let offset = address.wrapping_sub(BASE);
        if offset == CLOCK && size == 2 {
            if self.request.is_some() {
                cx.fail("SD clock changed during an active transaction");
                return;
            }
            self.clocks.write(cx, address, size, value);
            if cx.refusal().is_none() {
                self.clock = value as u16 & !2;
            }
            return;
        }
        if !Self::admitted(offset, size) || value >> (8 * u32::from(size)) != 0 {
            cx.unsupported(format!("Unknown SD0 register/access {address:#x}/{size}"));
            return;
        }
        self.advance(cx);
        if cx.refusal().is_some() {
            return;
        }
        if let Err(error) = self.store(cx, offset, size, value as u32) {
            cx.fail(error);
        }
    }

    fn event(&mut self, cx: &mut Cx<'_>, _token: u64) {
        self.advance(cx);
    }

    fn advance(&mut self, cx: &mut Cx<'_>) {
        self.clocks.advance(cx);
        if let Err(error) = self.complete(cx) {
            cx.fail(error);
        }
    }

    fn next_deadline(&self, now: u64) -> Option<u64> {
        let transfer = self.request.as_ref().and_then(|request| request.due).map(|(at, _)| at.max(now));
        transfer.into_iter().chain(self.clocks.next_deadline(now)).min()
    }

    fn pollable(&self, address: u64, size: u8) -> bool {
        let offset = address.wrapping_sub(BASE);
        offset != 0x20 && (Self::admitted(offset, size) || offset == CLOCK && size == 2)
    }

    fn save(&self) -> serde_json::Value {
        serde_json::json!({
            "present": self.present().ok(), "write_protected": self.protected().ok(), "state": self.state,
            "sectors": self.card.sectors(), "dirty_sectors": self.card.dirty_sectors(), "image": self.image,
            "block_length": self.block_length, "bus_width": self.bus_width, "pending": self.request.is_some(),
            "started": self.counters.started, "completed": self.counters.completed, "irqs": self.counters.irqs,
            "dma_boundaries": self.counters.dma_boundaries, "history": self.history,
        })
    }
}

/// Loads a regular whole-sector image of at most the declared capacity: its
/// nonzero sectors, within the dirty-sector bound, become the card's content.
/// Refuses an image that changes while it is read.
pub fn load_image(path: &Path, sectors: u32, max_dirty_sectors: usize) -> Result<RetainedCard, String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let fail = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(fail)?;
    let before = file.metadata().map_err(fail)?;
    let size = before.len();
    if !before.file_type().is_file() || size == 0 || size > u64::from(sectors) * SECTOR as u64 || size % SECTOR as u64 != 0 {
        return Err(format!("{}: SD image requires a bounded regular whole-sector file", path.display()));
    }
    let mut card = Card::new(sd_card::Options { sectors, max_dirty_sectors })?;
    let mut digest = Sha256::new();
    let mut chunk = vec![0; SECTOR * 2048];
    let (mut consumed, mut nonzero) = (0u64, 0u64);
    while consumed < size {
        let length = chunk.len().min((size - consumed) as usize);
        file.read_exact(&mut chunk[..length]).map_err(|_| format!("{}: SD image was truncated during import", path.display()))?;
        digest.update(&chunk[..length]);
        for (index, block) in chunk[..length].chunks_exact(SECTOR).enumerate() {
            if block.iter().any(|b| *b != 0) {
                card.write_bytes(consumed + (index * SECTOR) as u64, block)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                nonzero += 1;
            }
        }
        consumed += length as u64;
    }
    let extra = file.read(&mut chunk[..1]).map_err(fail)?;
    let after = file.metadata().map_err(fail)?;
    if extra != 0
        || (before.len(), before.mtime(), before.mtime_nsec(), before.ctime(), before.ctime_nsec())
            != (after.len(), after.mtime(), after.mtime_nsec(), after.ctime(), after.ctime_nsec())
    {
        return Err(format!("{}: SD image changed during import", path.display()));
    }
    Ok(RetainedCard {
        card,
        image: MediaImage {
            path: path.display().to_string(),
            sha256: format!("{:x}", digest.finalize()),
            sectors: size / SECTOR as u64,
            nonzero_sectors: nonzero,
        },
    })
}

/// The declared capacity and dirty-sector bound of a document's SD host adapter.
pub fn card_geometry(spec: &Adapter) -> Result<(u32, usize), String> {
    let options: Options = serde_json::from_value(spec.options.clone()).map_err(|e| format!("SD host options: {e}"))?;
    Ok((options.sectors, options.max_dirty_sectors))
}

pub fn construct(name: &str, spec: &Adapter, windows: &[Mapping], resources: &Resources) -> Result<Box<dyn Device>, String> {
    let declared: Vec<(u64, u64)> = windows.iter().map(|window| (window.base, window.size)).collect();
    if declared != WINDOWS {
        return Err(format!("{name} requires the FFF50000, FFF50030 and FFF50100 SD0 windows"));
    }
    let options: Options = serde_json::from_value(spec.options.clone()).map_err(|e| format!("{name}: {e}"))?;
    let gpio = spec.references.get("gpio").ok_or_else(|| format!("{name} requires a gpio reference"))?;
    let pads: Pads = resources.pads(gpio)?;
    Ok(Box::new(SdHost::new(options, Box::new(pads), NodeName(gpio.clone()))?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fujimod_emulator_devices::{Host, Record};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// The detect and protect pad levels.
    #[derive(Clone, Default)]
    struct Levels(Arc<[AtomicBool; 2]>);

    impl Levels {
        fn index(pin: Pin) -> Result<usize, String> {
            [DETECT, PROTECT].iter().position(|wired| *wired == pin).ok_or_else(|| "unwired test pad".to_owned())
        }

        fn set(&self, pin: Pin, high: bool) {
            self.0[Self::index(pin).unwrap()].store(high, Ordering::Relaxed);
        }
    }

    impl InputPins for Levels {
        fn level(&self, pin: Pin) -> Result<bool, String> {
            Ok(self.0[Self::index(pin)?].load(Ordering::Relaxed))
        }
    }

    struct Board {
        now: u64,
        ram: Vec<u8>,
        irqs: Vec<(usize, u32)>,
    }

    impl Host for Board {
        fn now(&self) -> u64 {
            self.now
        }
        fn schedule(&mut self, _: usize, _: u64, _: u64) {}
        fn set_irq(&mut self, _: u32, _: bool) {
            panic!("the SD host raises its owner core's line");
        }
        fn raise_irq(&mut self, cpu: usize, line: u32) -> Result<(), String> {
            self.irqs.push((cpu, line));
            Ok(())
        }
        fn ram_read(&self, address: u64, out: &mut [u8]) -> Result<(), String> {
            out.copy_from_slice(&self.ram[address as usize..address as usize + out.len()]);
            Ok(())
        }
        fn ram_write(&mut self, address: u64, bytes: &[u8]) -> Result<(), String> {
            self.ram[address as usize..address as usize + bytes.len()].copy_from_slice(bytes);
            Ok(())
        }
        fn record(&mut self, _: Record) {}
    }

    const DETECT: Pin = Pin { port: 12, pin: 7 };
    const PROTECT: Pin = Pin { port: 12, pin: 6 };
    const RCA: u32 = 0x1234_0000;

    struct Rig {
        host: SdHost,
        board: Board,
        levels: Levels,
    }

    impl Rig {
        fn new() -> Self {
            let levels = Levels::default();
            let options = Options {
                irq: Irq { core: 0, line: 78 },
                detect: DETECT,
                protect: PROTECT,
                ram_bytes: 0x10_0000,
                transfer_ticks: 100,
                clock_ticks: 2,
                sectors: 0x80_0000,
                max_dirty_sectors: 65536,
                rca: 0x1234,
            };
            let host = SdHost::new(options, Box::new(levels.clone()), NodeName("serial_gpio".into())).unwrap();
            Rig { host, board: Board { now: 0, ram: vec![0; 0x10_0000], irqs: Vec::new() }, levels }
        }

        fn write(&mut self, offset: u64, size: u8, value: u64) -> Result<(), String> {
            let mut cx = Cx::new(&mut self.board, 0, 0, 0);
            self.host.write(&mut cx, BASE + offset, size, value);
            cx.take_refusal().map_or(Ok(()), Err)
        }

        fn read(&mut self, offset: u64, size: u8) -> u64 {
            let mut cx = Cx::new(&mut self.board, 0, 0, 0);
            let value = self.host.read(&mut cx, BASE + offset, size);
            assert_eq!(cx.take_refusal(), None, "read {offset:#x}");
            value
        }

        fn elapse(&mut self) {
            self.board.now += 100;
            let mut cx = Cx::new(&mut self.board, usize::MAX, 0, 0);
            self.host.event(&mut cx, 0);
            assert_eq!(cx.take_refusal(), None);
        }

        fn powered() -> Self {
            let mut rig = Self::new();
            rig.write(CLOCK, 2, 5).unwrap();
            rig.write(0x28, 2, 0x100).unwrap();
            rig.write(0x34, 2, 0x1fff).unwrap();
            rig.write(0x38, 2, 0x1fff).unwrap();
            rig
        }

        /// Issues `word` with `argument`, completes it and acknowledges its status.
        fn command(&mut self, word: u32, argument: u32) -> u16 {
            self.write(8, 2, u64::from(argument & 0xffff)).unwrap();
            self.write(10, 2, u64::from(argument >> 16)).unwrap();
            self.write(12, 4, u64::from(word)).unwrap();
            self.elapse();
            let status = self.read(0x30, 2) as u16;
            self.write(0x30, 2, 0xffff).unwrap();
            status
        }

        fn selected() -> Self {
            let mut rig = Self::powered();
            for (word, argument) in [
                (0, 0),
                (0x081a_0000, 0x1aa),
                (0x371a_0000, 0),
                (0x2902_0000, 0x4110_0000),
                (0x0209_0000, 0),
                (0x031a_0000, 0),
                (0x0909_0000, RCA),
                (0x071b_0000, RCA),
            ] {
                rig.command(word, argument);
            }
            rig
        }

        fn response(&mut self) -> u32 {
            self.read(16, 2) as u32 | (self.read(18, 2) as u32) << 16
        }
    }

    #[test]
    fn absent_card_refuses_commands_and_reports_no_presence() {
        let mut rig = Rig::powered();
        rig.levels.set(DETECT, true);
        assert_eq!(rig.read(0x24, 4), 1 << 17);
        assert!(rig.write(12, 4, 0).is_err());
    }

    #[test]
    fn enumeration_answers_the_declared_card_and_busy_commands_complete_the_transfer() {
        let mut rig = Rig::powered();
        assert_eq!(rig.read(0x24, 4), 3 << 16);
        assert_eq!(rig.command(0x081a_0000, 0x1aa), 1);
        assert_eq!(rig.response(), 0x1aa);
        rig.command(0x371a_0000, 0);
        rig.command(0x2902_0000, 0x4110_0000);
        assert_eq!(rig.response(), 0xc010_0000);
        rig.command(0x0209_0000, 0);
        rig.command(0x031a_0000, 0);
        assert_eq!(rig.response(), RCA | 0x600);
        rig.command(0x0909_0000, RCA);
        assert_eq!(rig.command(0x071b_0000, RCA), 3);
        assert_eq!(rig.response(), 0x900);
        assert_eq!(rig.command(0x0cdb_0000, 0), 3);
        assert_eq!(rig.command(0x0d1a_0000, RCA), 1);
        assert_eq!(rig.response(), 4 << 9 | 0x100);
        assert!(rig.board.irqs.iter().all(|irq| *irq == (0, 78)));
    }

    #[test]
    fn unknown_command_words_and_byte_accesses_refuse() {
        let mut rig = Rig::selected();
        rig.write(12, 2, 0x10).unwrap();
        assert!(rig.write(14, 2, 0x113a).is_err());
        assert!(rig.write(0x29, 1, 0xf).is_err());
    }

    #[test]
    fn sdma_sector_read_copies_card_bytes_across_an_acknowledged_boundary() {
        let mut rig = Rig::selected();
        let content: Vec<u8> = (0..1024u32).map(|i| (i * 7) as u8).collect();
        rig.host.card.write_bytes(512 * 40, &content).unwrap();
        rig.write(0, 4, 0x4000_0e00).unwrap();
        rig.write(4, 2, 512).unwrap();
        rig.write(6, 2, 2).unwrap();
        rig.write(8, 2, 40).unwrap();
        rig.write(10, 2, 0).unwrap();
        rig.write(12, 4, 0x123a_0037).unwrap();
        rig.elapse();
        assert_eq!(rig.read(0x30, 2), 9);
        assert_eq!(rig.read(0, 4), 0x4000_1000);
        assert!(rig.write(0, 4, 0x4000_1000).is_err());
        rig.write(0x30, 2, 9).unwrap();
        rig.write(0, 4, 0x4000_1000).unwrap();
        rig.elapse();
        assert_eq!(rig.read(0x30, 2), 3);
        assert_eq!(rig.read(6, 2), 0);
        assert_eq!(&rig.board.ram[0xe00..0x1200], &content[..]);
    }

    #[test]
    fn pio_write_commits_whole_sectors_and_refuses_a_protected_card() {
        let mut rig = Rig::selected();
        rig.write(4, 2, 512).unwrap();
        rig.write(6, 2, 1).unwrap();
        rig.write(8, 2, 9).unwrap();
        rig.write(10, 2, 0).unwrap();
        rig.write(12, 4, 0x193a_0026).unwrap();
        rig.elapse();
        assert_eq!(rig.read(0x30, 2), 0x11);
        assert_eq!(rig.read(0x24, 4) & 0x403, 0x403);
        assert!(rig.write(0x20, 4, 1).is_err());
        rig.write(0x30, 2, 0x11).unwrap();
        for index in 0..128u64 {
            rig.write(0x20, 4, index).unwrap();
        }
        let mut sector = [0; 512];
        rig.host.card.read_bytes(9 * 512, &mut sector).unwrap();
        assert_eq!(sector, [0; 512]);
        rig.elapse();
        assert_eq!(rig.read(0x30, 2), 2);
        rig.host.card.read_bytes(9 * 512, &mut sector).unwrap();
        assert_eq!(&sector[4..8], &1u32.to_le_bytes());
        rig.write(0x30, 2, 0xffff).unwrap();
        rig.levels.set(PROTECT, true);
        rig.write(12, 2, 0x26).unwrap();
        assert!(rig.write(14, 2, 0x193a).is_err());
    }

    #[test]
    fn reset_cancels_a_pending_command() {
        let mut rig = Rig::selected();
        rig.write(8, 2, 0).unwrap();
        rig.write(10, 2, 0x1234).unwrap();
        rig.write(12, 4, 0x0d1a_0000).unwrap();
        assert_eq!(rig.read(0x24, 4) & 3, 1);
        rig.write(0x2e, 2, 0x200).unwrap();
        rig.elapse();
        assert_eq!(rig.read(0x30, 2), 0);
        assert_eq!(rig.read(0x24, 4) & 3, 0);
    }

    #[test]
    fn clock_reports_stable_after_its_settling_ticks() {
        let mut rig = Rig::new();
        rig.write(CLOCK, 2, 0x8001).unwrap();
        assert_eq!(rig.read(CLOCK, 2), 0x8001);
        rig.elapse();
        assert_eq!(rig.read(CLOCK, 2), 0x8003);
        rig.write(CLOCK, 2, 0).unwrap();
        assert_eq!(rig.read(CLOCK, 2), 0);
        assert!(rig.write(CLOCK, 2, 0x8).is_err());
    }

    #[test]
    fn listed_registers_are_exactly_the_readable_addresses() {
        let mut rig = Rig::powered();
        let mut readable = Vec::new();
        for &(base, size) in &WINDOWS {
            for address in base..base + size {
                let admitted = [1, 2, 4].into_iter().any(|width| {
                    let mut cx = Cx::new(&mut rig.board, 0, 0, 0);
                    rig.host.read(&mut cx, address, width);
                    !cx.is_unmodeled()
                });
                if admitted {
                    readable.push(address as u32);
                }
            }
        }
        assert_eq!(readable, REGISTERS);
    }
}
