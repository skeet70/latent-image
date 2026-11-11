#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

use core::ptr::{addr_of_mut, read_volatile, write_volatile};

pub mod boot_skip;
pub mod physical;
pub mod memory;
mod events;
pub use events::{EventFailure, EventOperation};

#[path = "../../first_stage/format.rs"]
pub mod boot_format;

const NAND: u32 = 0xfff5_2000;
pub const PAGE_BYTES: usize = 0x800;
pub const PAGES_PER_BLOCK: u32 = 0x40;
pub const BLOCK_BYTES: u32 = PAGE_BYTES as u32 * PAGES_PER_BLOCK;
const PROFILE_BASE: u16 = 0x380;
const PROFILE_PHYSICAL_BLOCKS: usize = 0x80;
const PROFILE_LOGICAL_BLOCKS: usize = 0x6c;
pub const PROFILE_FIRST_PHYSICAL_BLOCK: PhysicalBlock = PhysicalBlock(PROFILE_BASE);
pub const PROFILE_PHYSICAL_BLOCK_COUNT: usize = PROFILE_PHYSICAL_BLOCKS;
const UNMAPPED: u16 = u16::MAX;
const SAVED_REGISTERS: [u32; 6] = [0, 0x50, 0x58, 0x98, 0xe8, 0x78];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogicalBlock(u16);

impl LogicalBlock {
    pub fn new(value: u32) -> Result<Self, Error> {
        if value < PROFILE_LOGICAL_BLOCKS as u32 { Ok(Self(value as u16)) } else { Err(Error::Range) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteOffset(u32);

impl ByteOffset {
    pub const ZERO: Self = Self(0);
    pub fn new(value: u32) -> Result<Self, Error> {
        if value <= PROFILE_LOGICAL_BLOCKS as u32 * BLOCK_BYTES { Ok(Self(value)) } else { Err(Error::Range) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockCount(u16);

impl BlockCount {
    pub fn new(value: u32) -> Result<Self, Error> {
        if value <= PROFILE_LOGICAL_BLOCKS as u32 { Ok(Self(value as u16)) } else { Err(Error::Range) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalBlock(u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalPage(u32);

impl PhysicalBlock {
    pub fn new(index: u32) -> Result<Self, Error> {
        if index < 1024 { Ok(Self(index as u16)) } else { Err(Error::Range) }
    }

    pub const fn index(self) -> u32 { self.0 as u32 }

    fn page(self, offset: u32) -> PhysicalPage {
        PhysicalPage(u32::from(self.0) * PAGES_PER_BLOCK + offset)
    }
}

impl PhysicalPage {
    pub fn new(index: u32) -> Result<Self, Error> {
        if index < 1024 * PAGES_PER_BLOCK { Ok(Self(index)) } else { Err(Error::Range) }
    }

    pub const fn index(self) -> u32 { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeout(u32);

impl Timeout {
    pub const fn new(value: u32) -> Self { Self(value) }
    fn polls(self) -> u32 { self.0.wrapping_mul(625).max(1) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitMode { Poll, Interrupt }

impl WaitMode {
    fn parse(value: u32) -> Result<Self, Error> {
        match value { 0 => Ok(Self::Poll), 1 => Ok(Self::Interrupt), _ => Err(Error::Range) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verification { Disabled, ReadBack }

impl Verification {
    fn parse(value: u32) -> Result<Self, Error> {
        match value { 0 => Ok(Self::Disabled), 1 => Ok(Self::ReadBack), _ => Err(Error::Range) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NotConfigured, MapUnavailable, Range, UnsupportedObserver, NoReplacement, SkipBudget, Timeout, TaskContext, ReadFault, ProgramFailed, VerifyFailed, Semaphore(SemaphoreFailure), PinRoute(PinRouteFailure), Event(EventFailure), Image(memory::ImageError),
}

impl Error {
    pub const fn request_status(self) -> i32 {
        match self {
            Self::NotConfigured | Self::MapUnavailable => -10,
            Self::Range | Self::UnsupportedObserver => -30,
            Self::NoReplacement | Self::SkipBudget => -40,
            Self::Timeout | Self::TaskContext | Self::ReadFault | Self::ProgramFailed | Self::VerifyFailed | Self::Semaphore(_) | Self::PinRoute(_) | Self::Event(_) | Self::Image(_) => -255,
        }
    }
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NandSemaphore { Lifecycle = 36, State = 52 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemaphoreOperation { Wait, Signal }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OsalStatus(i32);

impl OsalStatus {
    pub const fn code(self) -> i32 { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemaphoreFailure {
    pub semaphore: NandSemaphore,
    pub operation: SemaphoreOperation,
    pub status: OsalStatus,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinClaim { Released = 0, Held = 11 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PinRouteFailure {
    pub claim: PinClaim,
    pub status: OsalStatus,
}

extern "C" {
    fn xm1_power_semaphore_wait(identifier: i32, timeout: i32) -> i32;
    fn xm1_power_semaphore_signal(identifier: i32) -> i32;
    fn xm1_power_resource_route_request(selector: u32, slot: u32, level: u32) -> i32;
}

unsafe fn semaphore_wait(semaphore: NandSemaphore) -> Result<(), Error> {
    let status = OsalStatus(unsafe { xm1_power_semaphore_wait(semaphore as i32, -1) });
    if status.0 == 0 { Ok(()) } else { Err(Error::Semaphore(SemaphoreFailure { semaphore, operation: SemaphoreOperation::Wait, status })) }
}

unsafe fn semaphore_signal(semaphore: NandSemaphore) -> Result<(), Error> {
    let status = OsalStatus(unsafe { xm1_power_semaphore_signal(semaphore as i32) });
    if status.0 == 0 { Ok(()) } else { Err(Error::Semaphore(SemaphoreFailure { semaphore, operation: SemaphoreOperation::Signal, status })) }
}

unsafe fn claim_pins(claim: PinClaim) -> Result<(), Error> {
    let status = OsalStatus(unsafe { xm1_power_resource_route_request(0x70a, 29, claim as u32) });
    if status.0 == 0 { Ok(()) } else { Err(Error::PinRoute(PinRouteFailure { claim, status })) }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeviceEnablePhase { Enable0, Clear1, Enable2, Other }

unsafe fn device_enable_phase() -> DeviceEnablePhase {
    match unsafe { read(0x9412a0) } {
        0 => DeviceEnablePhase::Enable0,
        1 => DeviceEnablePhase::Clear1,
        2 => DeviceEnablePhase::Enable2,
        _ => DeviceEnablePhase::Other,
    }
}

unsafe fn enable_device_presence() {
    unsafe {
        if device_enable_phase() != DeviceEnablePhase::Enable0 && device_enable_phase() != DeviceEnablePhase::Enable2 { return; }
        if device_enable_phase() == DeviceEnablePhase::Enable0 || device_enable_phase() == DeviceEnablePhase::Enable2 {
            if read_volatile(0x94129c as *const u8) == 0 { write_volatile(0x94129c as *mut u8, 1); }
        }
        if device_enable_phase() == DeviceEnablePhase::Clear1 { write_volatile(0x94129c as *mut u8, 0); }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Transfer { Tagged, Masked }

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PageReadBytes(u16);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReadStatus { Clean, TaggedFault(PageReadBytes) }

impl ReadStatus {
    fn require_clean(self) -> Result<(), Error> {
        match self { Self::Clean => Ok(()), Self::TaggedFault(_) => Err(Error::ReadFault) }
    }

    fn is_complete(self) -> bool {
        match self { Self::Clean => true, Self::TaggedFault(bytes) => usize::from(bytes.0) == PAGE_BYTES }
    }
}

mod endpoint {
    pub trait Sealed {}
}

/// # Safety
/// Native commands require exclusive controller, pad, clock and aperture ownership.
pub trait MediaEndpoint: endpoint::Sealed {
    fn require_configured(&self) -> Result<(), Error>;
    unsafe fn spare(&mut self, page: PhysicalPage, timeout: Timeout) -> Result<[u8; 16], Error>;
    unsafe fn erase(&mut self, block: PhysicalBlock, timeout: Timeout) -> Result<(), Error>;
    unsafe fn read_page(&mut self, page: PhysicalPage, output: &mut [u8; PAGE_BYTES], transfer: Transfer, timeout: Timeout) -> Result<ReadStatus, Error>;
    unsafe fn set_tag(&mut self, block: Option<LogicalBlock>, bad: bool);
    unsafe fn program_page(&mut self, page: PhysicalPage, data: &[u8; PAGE_BYTES], transfer: Transfer, timeout: Timeout) -> Result<(), Error>;

    unsafe fn mark_bad(&mut self, block: PhysicalBlock, scratch: &mut [u8; PAGE_BYTES], transfer: Transfer, timeout: Timeout) -> Result<(), Error> {
        scratch.fill(0xff);
        unsafe { mark_bad_pages(self, block, scratch, transfer, timeout) }
    }
}

unsafe fn mark_bad_pages<E: MediaEndpoint + ?Sized>(endpoint: &mut E, block: PhysicalBlock, erased: &[u8; PAGE_BYTES], transfer: Transfer, timeout: Timeout) -> Result<(), Error> {
    unsafe {
        endpoint.set_tag(None, true);
        for page in 0..PAGES_PER_BLOCK {
            match endpoint.program_page(block.page(page), erased, transfer, timeout) {
                Ok(()) | Err(Error::ProgramFailed) => {},
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}

struct MappingTag(u16);

fn mapping_tag(block: Option<LogicalBlock>) -> MappingTag {
    MappingTag(match block {
        None => 0xffff,
        Some(block) => {
            let tag = (u32::from(block.0) % 1000) * 2 | 0x1000;
            (tag | (tag.count_ones() & 1)) as u16
        },
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DriverState { Stopped, Configured }

pub struct Controller {
    state: DriverState,
    saved: [u32; 6],
    wait_mode: WaitMode,
}

unsafe fn read(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn write(address: u32, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }

impl Controller {
    const fn new() -> Self { Self { state: DriverState::Stopped, saved: [0; 6], wait_mode: WaitMode::Poll } }

    unsafe fn configure(&mut self) {
        if self.state == DriverState::Configured { return; }
        unsafe {
            write(0xfff8_8028, read(0xfff8_8028) & !0x0300_0000);
            write(0xfff8_8030, read(0xfff8_8030) | 0x800);
            enable_device_presence();
            for (index, offset) in SAVED_REGISTERS.into_iter().enumerate() {
                self.saved[index] = read(NAND + offset);
            }
            write(NAND + 0xe8, 0x10003);
            write(NAND, 1);
            write(NAND + 0x50, 0x2231_2131);
            write(NAND + 0x78, 0x605);
            write(NAND + 4, 0x108);
            write(NAND + 8, 0);
            write(NAND + 0x98, 0x8000_0000);
            write(NAND + 0x58, 0x330);
            self.finish();
        }
        self.wait_mode = WaitMode::Poll;
        self.state = DriverState::Configured;
    }

    unsafe fn shutdown(&mut self) {
        if self.state == DriverState::Stopped { return; }
        unsafe {
            for (index, offset) in SAVED_REGISTERS.into_iter().enumerate() {
                let value = if offset == 0x98 { self.saved[index] & 0x8000_0000 } else { self.saved[index] };
                write(NAND + offset, value);
            }
        }
        self.state = DriverState::Stopped;
    }

    fn require_configured(&self) -> Result<(), Error> {
        if self.state == DriverState::Configured { Ok(()) } else { Err(Error::NotConfigured) }
    }

    unsafe fn finish(&mut self) {
        unsafe { write(NAND + 0x10, 0); write(NAND + 0x18, 0x80a3); }
    }

    unsafe fn prepare_command(&mut self) -> Result<events::PendingCommand, Error> {
        let result = unsafe { events::prepare(self.wait_mode) };
        if result.is_err() {
            unsafe {
                write(NAND + 8, 0);
                if self.wait_mode == WaitMode::Interrupt { write(NAND + 0x54, 0x80001); }
                self.finish();
            }
        }
        result
    }

    unsafe fn wait(&mut self, pending: events::PendingCommand, timeout: Timeout) -> Result<(), Error> {
        let status = unsafe { events::wait(pending, timeout) };
        if status.is_err() { unsafe { write(NAND + 8, 0); } }
        status
    }

    unsafe fn address(&mut self, page: PhysicalPage, sector: u32) {
        unsafe {
            write(NAND + 0x60, page.0);
            write(NAND + 0x20, if sector == 4 { 0x200 } else { sector * 0x210 });
            write(NAND + 0x24, page.0 & 0x00ff_ffff);
        }
    }

    unsafe fn spare(&mut self, page: PhysicalPage, timeout: Timeout) -> Result<[u8; 16], Error> {
        self.require_configured()?;
        unsafe {
            write(NAND + 0x1c, 0x0030_0000);
            write(NAND + 0x18, 0x8083);
            self.address(page, 4);
            write(NAND + 4, 0x108);
            let pending = self.prepare_command()?;
            write(NAND + 8, 0x0080_2041);
            let status = self.wait(pending, timeout);
            if let Err(error) = status { self.finish(); return Err(error); }
            let mut spare = [0; 16];
            for index in 0..4 {
                spare[index * 4..index * 4 + 4].copy_from_slice(&read(NAND + 0x400 + index as u32 * 4).to_le_bytes());
            }
            self.finish();
            Ok(spare)
        }
    }

    unsafe fn erase(&mut self, block: PhysicalBlock, timeout: Timeout) -> Result<(), Error> {
        self.require_configured()?;
        unsafe {
            write(NAND + 0x1c, 0x60d0_7000);
            write(NAND + 0x18, 0x8093);
            self.address(block.page(0), 0);
            write(NAND + 4, 0x108);
            let pending = self.prepare_command()?;
            write(NAND + 8, 0x0080_2046);
            let status = self.wait(pending, timeout);
            let failed = read(NAND + 0x28) & 0x0100_0000 != 0;
            self.finish();
            status?;
            if failed { Err(Error::ProgramFailed) } else { Ok(()) }
        }
    }

    unsafe fn read_page(&mut self, page: PhysicalPage, data: &mut [u8; PAGE_BYTES], transfer: Transfer, timeout: Timeout) -> Result<ReadStatus, Error> {
        self.require_configured()?;
        unsafe {
            write(NAND, 1);
            write(NAND + 0x98, if transfer == Transfer::Tagged { 0x8000_0000 } else { 0 });
            for sector in 0..4 {
                write(NAND + 0x1c, 0x0030_0001);
                write(NAND + 0x18, 0x8083);
                self.address(page, sector);
                write(NAND + 4, 0x108);
                let pending = self.prepare_command()?;
                write(NAND + 8, if transfer == Transfer::Tagged { 0x0080_20f3 } else { 0x0080_20c3 });
                let status = self.wait(pending, timeout);
                self.finish();
                status?;
                for word in 0..128 {
                    let offset = sector as usize * 512 + word * 4;
                    data[offset..offset + 4].copy_from_slice(&read(NAND + 0x400 + word as u32 * 4).to_le_bytes());
                }
                if transfer == Transfer::Tagged && read(NAND + 4) & 0x100 != 0 { return Ok(ReadStatus::TaggedFault(PageReadBytes(((sector + 1) * 512) as u16))); }
            }
        }
        Ok(ReadStatus::Clean)
    }

    unsafe fn set_tag(&mut self, block: Option<LogicalBlock>, bad: bool) {
        let tag = u32::from(mapping_tag(block).0);
        unsafe {
            write(NAND + 0x70, tag << 16);
            let repeated = (tag << 16) | tag;
            write(NAND + 0x40, repeated);
            write(NAND + 0x44, if bad { 0xffe0_ffff } else { u32::MAX });
            write(NAND + 0x48, u32::MAX);
            write(NAND + 0x4c, u32::MAX);
        }
    }

    unsafe fn set_spare(&mut self, spare: &[u8; 16]) {
        unsafe {
            for (index, bytes) in spare.chunks_exact(4).enumerate() {
                write(NAND + 0x40 + index as u32 * 4, u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
            }
        }
    }

    unsafe fn program_page(&mut self, page: PhysicalPage, data: &[u8; PAGE_BYTES], transfer: Transfer, timeout: Timeout) -> Result<(), Error> {
        self.require_configured()?;
        unsafe {
            write(NAND, 1);
            write(NAND + 0x98, if transfer == Transfer::Tagged { 0x8000_0000 } else { 0 });
            for sector in 0..4 {
                for word in 0..128 {
                    let offset = sector as usize * 512 + word * 4;
                    let value = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
                    write(NAND + 0x400 + word as u32 * 4, value);
                }
                let (command, trigger) = match sector {
                    0 => (0x8000_0001, 0x0088_2062),
                    1 | 2 => (1, 0x0088_2004),
                    _ => (0x0010_7001, 0x0080_2064),
                };
                write(NAND + 0x1c, command);
                write(NAND + 0x5c, 0x1010);
                self.address(page, sector);
                write(NAND + 0x18, 0x8093);
                write(NAND + 4, 0x108);
                let pending = self.prepare_command()?;
                write(NAND + 8, trigger);
                let status = self.wait(pending, timeout);
                if sector == 3 { self.finish(); }
                if let Err(error) = status { self.finish(); return Err(error); }
                if read(NAND + 0x28) & 0x0100_0000 != 0 { return Err(Error::ProgramFailed); }
            }
        }
        Ok(())
    }

}

impl endpoint::Sealed for Controller {}

impl MediaEndpoint for Controller {
    fn require_configured(&self) -> Result<(), Error> { Controller::require_configured(self) }
    unsafe fn spare(&mut self, page: PhysicalPage, timeout: Timeout) -> Result<[u8; 16], Error> {
        unsafe { Controller::spare(self, page, timeout) }
    }
    unsafe fn erase(&mut self, block: PhysicalBlock, timeout: Timeout) -> Result<(), Error> {
        unsafe { Controller::erase(self, block, timeout) }
    }
    unsafe fn read_page(&mut self, page: PhysicalPage, output: &mut [u8; PAGE_BYTES], transfer: Transfer, timeout: Timeout) -> Result<ReadStatus, Error> {
        unsafe { Controller::read_page(self, page, output, transfer, timeout) }
    }
    unsafe fn set_tag(&mut self, block: Option<LogicalBlock>, bad: bool) {
        unsafe { Controller::set_tag(self, block, bad); }
    }
    unsafe fn program_page(&mut self, page: PhysicalPage, data: &[u8; PAGE_BYTES], transfer: Transfer, timeout: Timeout) -> Result<(), Error> {
        unsafe { Controller::program_page(self, page, data, transfer, timeout) }
    }
}

struct ProfileMap {
    mounted: bool,
    blocks: [u16; PROFILE_LOGICAL_BLOCKS],
    free: [u8; PROFILE_PHYSICAL_BLOCKS / 8],
    cursor: u16,
    used: u16,
}

impl ProfileMap {
    const fn new() -> Self {
        Self { mounted: false, blocks: [UNMAPPED; PROFILE_LOGICAL_BLOCKS], free: [0; PROFILE_PHYSICAL_BLOCKS / 8], cursor: UNMAPPED, used: 0 }
    }

    fn lookup(&self, block: LogicalBlock) -> Option<PhysicalBlock> {
        match self.blocks[block.0 as usize] { UNMAPPED => None, relative => Some(PhysicalBlock(PROFILE_BASE + relative)) }
    }

    fn set_free(&mut self, block: PhysicalBlock, available: bool) {
        let relative = block.0 - PROFILE_BASE;
        let byte = &mut self.free[relative as usize / 8];
        let bit = 1 << (relative & 7);
        if available { *byte |= bit; self.cursor = self.cursor.min(relative); } else { *byte &= !bit; }
    }

    fn candidate(&self) -> Result<PhysicalBlock, Error> {
        if self.used as usize >= PROFILE_PHYSICAL_BLOCKS - PROFILE_LOGICAL_BLOCKS { return Err(Error::NoReplacement); }
        for index in (self.cursor & !7) as usize..PROFILE_PHYSICAL_BLOCKS {
            if self.free[index / 8] & (1 << (index & 7)) != 0 { return Ok(PhysicalBlock(PROFILE_BASE + index as u16)); }
        }
        Err(Error::NoReplacement)
    }

    fn store(&mut self, logical: LogicalBlock, physical: Option<PhysicalBlock>) {
        self.blocks[logical.0 as usize] = physical.map_or(UNMAPPED, |block| block.0 - PROFILE_BASE);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpareKind { Erased, Mapped, Bad }

fn classify(spare: &[u8; 16]) -> SpareKind {
    if spare.iter().all(|&byte| byte == 0xff) { SpareKind::Erased }
    else if spare[4] != 0xff || spare[5] != 0xff { SpareKind::Bad }
    else { SpareKind::Mapped }
}

fn decode_tag(spare: &[u8; 16]) -> Option<LogicalBlock> {
    let first = u16::from_be_bytes([spare[0], spare[1]]);
    let second = u16::from_be_bytes([spare[2], spare[3]]);
    let valid = |tag: u16| tag.count_ones() & 1 == 0;
    let tag = if first == second || !valid(second) {
        if !valid(first) { return None; }
        first
    } else {
        if valid(first) { return None; }
        second
    };
    LogicalBlock::new(u32::from((tag >> 1) & 0xf7ff)).ok()
}

pub struct ApplicationNand<E: MediaEndpoint = Controller> {
    controller: E,
    map: ProfileMap,
    page: [u8; PAGE_BYTES],
    comparison: [u8; PAGE_BYTES],
}

impl ApplicationNand {
    pub const fn new() -> Self {
        Self { controller: Controller::new(), map: ProfileMap::new(), page: [0; PAGE_BYTES], comparison: [0; PAGE_BYTES] }
    }
    /// # Safety
    /// The caller exclusively owns the NAND controller, pad route, clock gate and DMA aperture across both cores and interrupts until shutdown. The MMIO region has device-memory attributes.
    pub unsafe fn configure(&mut self) {
        unsafe { self.controller.configure(); }
    }

    /// # Safety
    /// The configure ownership contract remains in force.
    pub unsafe fn shutdown(&mut self) { unsafe { self.controller.shutdown(); } }

}

impl<E: MediaEndpoint> ApplicationNand<E> {
    fn from_endpoint(controller: E) -> Self {
        Self { controller, map: ProfileMap::new(), page: [0; PAGE_BYTES], comparison: [0; PAGE_BYTES] }
    }

    pub fn is_configured(&self) -> bool { self.controller.require_configured().is_ok() }
    pub fn is_mounted(&self) -> bool { self.map.mounted }


    unsafe fn retire(&mut self, block: PhysicalBlock, transfer: Transfer, timeout: Timeout) -> Result<(), Error> {
        self.map.set_free(block, false);
        self.map.used += 1;
        unsafe { self.controller.mark_bad(block, &mut self.page, transfer, timeout) }
    }

    /// # Safety
    /// The configure ownership contract remains in force. Scanning may erase inconsistent mapping records in physical blocks 0x380 through 0x3ff.
    pub unsafe fn build_profile_map(&mut self, timeout: Timeout) -> Result<(), Error> {
        self.controller.require_configured()?;
        self.map = ProfileMap::new();
        for relative in 0..PROFILE_PHYSICAL_BLOCKS {
            let block = PhysicalBlock(PROFILE_BASE + relative as u16);
            let spare = unsafe { self.controller.spare(block.page(0), timeout)? };
            match classify(&spare) {
                SpareKind::Bad => { self.map.used += 1; continue; },
                SpareKind::Erased => {},
                SpareKind::Mapped => {
                    let first = decode_tag(&spare);
                    let last = decode_tag(&unsafe { self.controller.spare(block.page(PAGES_PER_BLOCK - 1), timeout)? });
                    if let Some(logical) = first {
                        if last == first && self.map.lookup(logical).is_none() {
                            self.map.store(logical, Some(block));
                            continue;
                        }
                    }
                    match unsafe { self.controller.erase(block, timeout) } {
                        Ok(()) => {},
                        Err(Error::ProgramFailed) => { unsafe { self.retire(block, Transfer::Tagged, timeout)?; } continue; },
                        Err(error) => return Err(error),
                    }
                }
            }
            self.map.set_free(block, true);
        }
        self.map.mounted = true;
        Ok(())
    }

    fn range(&self, block: LogicalBlock, offset: ByteOffset, length: usize) -> Result<u32, Error> {
        self.controller.require_configured()?;
        if !self.map.mounted { return Err(Error::MapUnavailable); }
        let start = u32::from(block.0).checked_mul(BLOCK_BYTES).and_then(|start| start.checked_add(offset.0)).ok_or(Error::Range)?;
        let end = start.checked_add(u32::try_from(length).map_err(|_| Error::Range)?).ok_or(Error::Range)?;
        if end > PROFILE_LOGICAL_BLOCKS as u32 * BLOCK_BYTES { return Err(Error::Range); }
        Ok(start)
    }

    unsafe fn load_logical_page(&mut self, page: u32, transfer: Transfer, timeout: Timeout) -> Result<(), Error> {
        let logical = LogicalBlock::new(page / PAGES_PER_BLOCK)?;
        match self.map.lookup(logical) {
            None => self.page.fill(0xff),
            Some(physical) => unsafe { self.controller.read_page(physical.page(page % PAGES_PER_BLOCK), &mut self.page, transfer, timeout)?.require_clean()?; },
        }
        Ok(())
    }

    /// # Safety
    /// The configure ownership contract remains in force.
    pub unsafe fn read_bytes(&mut self, block: LogicalBlock, offset: ByteOffset, output: &mut [u8], timeout: Timeout) -> Result<(), Error> {
        unsafe { self.read_range(block, offset, output, Transfer::Masked, timeout) }
    }

    /// # Safety
    /// The configure ownership contract remains in force.
    pub unsafe fn read_blocks(&mut self, first: LogicalBlock, output: &mut [u8], timeout: Timeout) -> Result<(), Error> {
        if output.len() % BLOCK_BYTES as usize != 0 { return Err(Error::Range); }
        unsafe { self.read_range(first, ByteOffset::ZERO, output, Transfer::Tagged, timeout) }
    }

    unsafe fn read_range(&mut self, block: LogicalBlock, offset: ByteOffset, output: &mut [u8], transfer: Transfer, timeout: Timeout) -> Result<(), Error> {
        let start = self.range(block, offset, output.len())?;
        let mut address = start;
        let mut remaining = output;
        while !remaining.is_empty() {
            let offset = address as usize % PAGE_BYTES;
            let count = remaining.len().min(PAGE_BYTES - offset);
            if offset == 0 && count == PAGE_BYTES {
                let logical = LogicalBlock::new(address / BLOCK_BYTES)?;
                let output: &mut [u8; PAGE_BYTES] = (&mut remaining[..PAGE_BYTES]).try_into().map_err(|_| Error::Range)?;
                match self.map.lookup(logical) {
                    None => output.fill(0xff),
                    Some(physical) => unsafe { self.controller.read_page(physical.page(address / PAGE_BYTES as u32 % PAGES_PER_BLOCK), output, transfer, timeout)?.require_clean()?; },
                }
            } else {
                unsafe { self.load_logical_page(address / PAGE_BYTES as u32, transfer, timeout)?; }
                remaining[..count].copy_from_slice(&self.page[offset..offset + count]);
            }
            remaining = &mut remaining[count..];
            address += count as u32;
        }
        Ok(())
    }

    /// # Safety
    /// The configure ownership contract remains in force.
    pub unsafe fn erase_blocks(&mut self, first: LogicalBlock, count: BlockCount, timeout: Timeout) -> Result<(), Error> {
        let bytes = u32::from(count.0) * BLOCK_BYTES;
        self.range(first, ByteOffset::ZERO, bytes as usize)?;
        for index in u32::from(first.0)..u32::from(first.0) + u32::from(count.0) {
            let logical = LogicalBlock::new(index)?;
            if let Some(physical) = self.map.lookup(logical) {
                match unsafe { self.controller.erase(physical, timeout) } {
                    Ok(()) => { self.map.store(logical, None); self.map.set_free(physical, true); },
                    Err(Error::ProgramFailed) => unsafe { self.retire(physical, Transfer::Tagged, timeout)?; },
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(())
    }

    unsafe fn verify_page(controller: &mut E, comparison: &mut [u8; PAGE_BYTES], expected: &[u8; PAGE_BYTES], physical: PhysicalPage, transfer: Transfer, timeout: Timeout) -> Result<(), Error> {
        let status = unsafe { controller.read_page(physical, comparison, transfer, timeout)? };
        if status.is_complete() && expected == &*comparison { Ok(()) } else { Err(Error::VerifyFailed) }
    }

    unsafe fn replacement(&mut self, logical: LogicalBlock, data: &[u8], verification: Verification, timeout: Timeout) -> Result<(), Error> {
        let previous = self.map.lookup(logical);
        loop {
            let candidate = self.map.candidate()?;
            let spare = unsafe { self.controller.spare(candidate.page(0), timeout)? };
            self.map.set_free(candidate, false);
            if classify(&spare) == SpareKind::Bad { self.map.used += 1; continue; }
            unsafe { self.controller.set_tag(Some(logical), false); }
            let mut failed = false;
            for page in 0..PAGES_PER_BLOCK {
                let offset = page as usize * PAGE_BYTES;
                let expected: &[u8; PAGE_BYTES] = (&data[offset..offset + PAGE_BYTES]).try_into().map_err(|_| Error::Range)?;
                let status = unsafe { self.controller.program_page(candidate.page(page), expected, Transfer::Tagged, timeout) };
                let status = status.and_then(|()| {
                    if verification == Verification::ReadBack { unsafe { Self::verify_page(&mut self.controller, &mut self.comparison, expected, candidate.page(page), Transfer::Tagged, timeout) } } else { Ok(()) }
                });
                match status {
                    Ok(()) => {},
                    Err(Error::ProgramFailed | Error::VerifyFailed) => { failed = true; break; },
                    Err(error) => return Err(error),
                }
            }
            if failed { unsafe { self.retire(candidate, Transfer::Tagged, timeout)?; } continue; }
            if let Some(previous) = previous {
                match unsafe { self.controller.erase(previous, timeout) } {
                    Ok(()) => self.map.set_free(previous, true),
                    Err(Error::ProgramFailed) => unsafe { self.retire(previous, Transfer::Tagged, timeout)?; },
                    Err(error) => return Err(error),
                }
            }
            self.map.store(logical, Some(candidate));
            return Ok(());
        }
    }

    /// # Safety
    /// The configure ownership contract remains in force.
    pub unsafe fn write_blocks(&mut self, first: LogicalBlock, data: &[u8], verification: Verification, timeout: Timeout) -> Result<(), Error> {
        if data.len() % BLOCK_BYTES as usize != 0 { return Err(Error::Range); }
        self.range(first, ByteOffset::ZERO, data.len())?;
        for (index, block) in data.chunks_exact(BLOCK_BYTES as usize).enumerate() {
            unsafe { self.replacement(LogicalBlock::new(u32::from(first.0) + index as u32)?, block, verification, timeout)?; }
        }
        Ok(())
    }

    unsafe fn relocate_masked(&mut self, logical: LogicalBlock, previous: PhysicalBlock, timeout: Timeout) -> Result<PhysicalBlock, Error> {
        loop {
            let candidate = self.map.candidate()?;
            let spare = unsafe { self.controller.spare(candidate.page(0), timeout)? };
            self.map.set_free(candidate, false);
            if classify(&spare) == SpareKind::Bad { self.map.used += 1; continue; }
            let mut failed = false;
            unsafe { self.controller.set_tag(Some(logical), false); }
            for page in 0..PAGES_PER_BLOCK {
                unsafe { self.controller.read_page(previous.page(page), &mut self.page, Transfer::Masked, timeout)?; }
                match unsafe { self.controller.program_page(candidate.page(page), &self.page, Transfer::Masked, timeout) } {
                    Ok(()) => {},
                    Err(Error::ProgramFailed) => { failed = true; break; },
                    Err(error) => return Err(error),
                }
            }
            if failed { unsafe { self.retire(candidate, Transfer::Masked, timeout)?; } continue; }
            unsafe { self.retire(previous, Transfer::Masked, timeout)?; }
            self.map.store(logical, Some(candidate));
            return Ok(candidate);
        }
    }

    unsafe fn allocate_masked(&mut self, logical: LogicalBlock, target_page: u32, offset: usize, data: &[u8], verification: Verification, timeout: Timeout) -> Result<PhysicalBlock, Error> {
        loop {
            let candidate = self.map.candidate()?;
            let spare = unsafe { self.controller.spare(candidate.page(0), timeout)? };
            self.map.set_free(candidate, false);
            if classify(&spare) == SpareKind::Bad { self.map.used += 1; continue; }
            unsafe { self.controller.set_tag(Some(logical), false); }
            let mut failed = false;
            for page in 0..PAGES_PER_BLOCK {
                self.page.fill(0xff);
                if page == target_page { self.page[offset..offset + data.len()].copy_from_slice(data); }
                let status = unsafe { self.controller.program_page(candidate.page(page), &self.page, Transfer::Masked, timeout) };
                let status = status.and_then(|()| {
                    if page == target_page && verification == Verification::ReadBack { unsafe { Self::verify_page(&mut self.controller, &mut self.comparison, &self.page, candidate.page(page), Transfer::Masked, timeout) } } else { Ok(()) }
                });
                match status {
                    Ok(()) => {},
                    Err(Error::ProgramFailed | Error::VerifyFailed) => { failed = true; break; },
                    Err(error) => return Err(error),
                }
            }
            if failed { unsafe { self.retire(candidate, Transfer::Masked, timeout)?; } continue; }
            self.map.store(logical, Some(candidate));
            return Ok(candidate);
        }
    }

    /// NAND programming preserves zero bits.
    /// # Safety
    /// The configure ownership contract remains in force.
    pub unsafe fn write_bytes(&mut self, first: LogicalBlock, offset: ByteOffset, data: &[u8], verification: Verification, timeout: Timeout) -> Result<(), Error> {
        let mut address = self.range(first, offset, data.len())?;
        let mut remaining = data;
        while !remaining.is_empty() {
            let logical = LogicalBlock::new(address / BLOCK_BYTES)?;
            let page_index = (address / PAGE_BYTES as u32) % PAGES_PER_BLOCK;
            let offset = address as usize % PAGE_BYTES;
            let count = remaining.len().min(PAGE_BYTES - offset);
            loop {
                unsafe { self.load_logical_page(address / PAGE_BYTES as u32, Transfer::Masked, timeout)?; }
                let mut changed = false;
                for (old, &new) in self.page[offset..offset + count].iter_mut().zip(&remaining[..count]) {
                    changed |= *old & new != *old;
                    *old &= new;
                }
                if !changed { break; }
                let physical = match self.map.lookup(logical) {
                    Some(physical) => physical,
                    None => { unsafe { self.allocate_masked(logical, page_index, offset, &remaining[..count], verification, timeout)?; } break; },
                };
                unsafe { self.controller.set_tag(Some(logical), false); }
                let status = unsafe { self.controller.program_page(physical.page(page_index), &self.page, Transfer::Masked, timeout) };
                let status = status.and_then(|()| {
                    if verification == Verification::ReadBack { unsafe { Self::verify_page(&mut self.controller, &mut self.comparison, &self.page, physical.page(page_index), Transfer::Masked, timeout) } } else { Ok(()) }
                });
                match status {
                    Ok(()) => break,
                    Err(Error::ProgramFailed | Error::VerifyFailed) => { unsafe { self.relocate_masked(logical, physical, timeout)?; } },
                    Err(error) => return Err(error),
                }
            }
            address += count as u32;
            remaining = &remaining[count..];
        }
        Ok(())
    }
}

impl Default for ApplicationNand {
    fn default() -> Self { Self::new() }
}

static mut APPLICATION: ApplicationNand = ApplicationNand::new();

fn status(result: Result<(), Error>) -> i32 { result.map_or_else(Error::request_status, |()| 0) }

unsafe fn application() -> &'static mut ApplicationNand { unsafe { &mut *addr_of_mut!(APPLICATION) } }

unsafe fn with_state<T>(operation: impl FnOnce(&mut ApplicationNand) -> Result<T, Error>) -> Result<T, Error> {
    unsafe { semaphore_wait(NandSemaphore::State)?; }
    let app = unsafe { application() };
    app.controller.wait_mode = WaitMode::Poll;
    let result = operation(app);
    app.controller.wait_mode = WaitMode::Poll;
    let released = unsafe { semaphore_signal(NandSemaphore::State) };
    match result {
        Ok(value) => released.map(|()| value),
        Err(error) => Err(error),
    }
}

unsafe fn configure_native() -> Result<(), Error> {
    unsafe { semaphore_wait(NandSemaphore::Lifecycle)?; }
    if let Err(error) = unsafe { semaphore_wait(NandSemaphore::State) } {
        let _ = unsafe { semaphore_signal(NandSemaphore::Lifecycle) };
        return Err(error);
    }
    if let Err(error) = unsafe { claim_pins(PinClaim::Held) } {
        let _ = unsafe { claim_pins(PinClaim::Released) };
        let _ = unsafe { semaphore_signal(NandSemaphore::Lifecycle) };
        let _ = unsafe { semaphore_signal(NandSemaphore::State) };
        return Err(error);
    }
    unsafe { application().configure(); }
    unsafe { semaphore_signal(NandSemaphore::State) }
}

unsafe fn shutdown_native() -> Result<(), Error> {
    unsafe { semaphore_wait(NandSemaphore::State)?; }
    let configured = unsafe { application().is_configured() };
    if configured { unsafe { application().shutdown(); } }
    let lifecycle = if configured { unsafe { semaphore_signal(NandSemaphore::Lifecycle) } } else { Ok(()) };
    let pins = if configured { unsafe { claim_pins(PinClaim::Released) } } else { Ok(()) };
    let state = unsafe { semaphore_signal(NandSemaphore::State) };
    lifecycle.and(pins).and(state)
}

unsafe fn transfer_request<T>(mode: u32, slot: u32, operation: impl FnOnce(&mut ApplicationNand) -> Result<T, Error>) -> Result<T, Error> {
    let wait_mode = WaitMode::parse(mode)?;
    if slot != 0 { return Err(Error::MapUnavailable); }
    let temporary = unsafe { with_state(|app| Ok(!app.is_configured()))? };
    if temporary { unsafe { configure_native()?; } }
    let result = unsafe { with_state(|app| {
        app.controller.wait_mode = wait_mode;
        operation(app)
    }) };
    if temporary {
        let shutdown = unsafe { shutdown_native() };
        if result.is_ok() { shutdown?; }
    }
    result
}

unsafe fn source<'a>(address: u32, length: u32) -> Result<&'a [u8], Error> {
    address.checked_add(length).ok_or(Error::Range)?;
    if length == 0 { return Ok(&[]); }
    if address == 0 || length > isize::MAX as u32 { return Err(Error::Range); }
    Ok(unsafe { core::slice::from_raw_parts(address as *const u8, length as usize) })
}

/// # Safety
/// Requests are aligned arrays of the documented extent. Calls originate in task context with owned OSAL semaphores 36 and 52 and the shared pad-route constructor initialized. Other NAND users honor the same semaphore protocol. Request and data buffers are exclusively owned, disjoint, and do not overlap APPLICATION or executing code. NAND MMIO has device-memory attributes.
// Image4 0x6fbbe4 never reads the normalized configure descriptor.
#[no_mangle]
pub unsafe extern "C" fn xm1_app_nand_configure(_request: *const [u32; 2]) -> i32 {
    status(unsafe { configure_native() })
}

/// # Safety
/// The native export ownership contract remains in force.
#[no_mangle]
pub unsafe extern "C" fn xm1_app_nand_shutdown() { let _ = unsafe { shutdown_native() }; }

/// # Safety
/// The native export ownership contract remains in force. This scans and repairs the profile mapping region.
#[no_mangle]
pub unsafe extern "C" fn xm1_app_nand_build_profile_map() -> i32 {
    status(unsafe { with_state(|app| app.build_profile_map(Timeout::new(1000))) })
}

/// # Safety
/// The native export ownership contract remains in force. The request has five words: first block, block count, event mode, timeout, map slot.
#[no_mangle]
pub unsafe extern "C" fn xm1_app_nand_erase(request: *mut [u32; 5]) -> i32 {
    let request = unsafe { &*request };
    let result = (|| {
        let first = LogicalBlock::new(request[0])?;
        let count = BlockCount::new(request[1])?;
        let timeout = Timeout::new(request[3]);
        unsafe { transfer_request(request[2], request[4], |app| app.erase_blocks(first, count, timeout)) }
    })();
    status(result)
}

/// # Safety
/// The native export ownership contract remains in force. The request has seven words: first block, block count, source, verification, event mode, timeout, map slot.
#[no_mangle]
pub unsafe extern "C" fn xm1_app_nand_write_blocks(request: *mut [u32; 7]) -> i32 {
    let request = unsafe { &*request };
    let result = (|| {
        let first = LogicalBlock::new(request[0])?;
        let verification = Verification::parse(request[3])?;
        let timeout = Timeout::new(request[5]);
        let length = request[1].checked_mul(BLOCK_BYTES).ok_or(Error::Range)?;
        let data = unsafe { source(request[2], length)? };
        unsafe { transfer_request(request[4], request[6], |app| app.write_blocks(first, data, verification, timeout)) }
    })();
    status(result)
}

/// # Safety
/// The native export ownership contract remains in force. The request has eight words: first block, byte offset, byte count, source, verification, event mode, timeout, map slot.
#[no_mangle]
pub unsafe extern "C" fn xm1_app_nand_write_bytes(request: *mut [u32; 8]) -> i32 {
    let request = unsafe { &*request };
    let result = (|| {
        let first = LogicalBlock::new(request[0])?;
        let offset = ByteOffset::new(request[1])?;
        let verification = Verification::parse(request[4])?;
        let timeout = Timeout::new(request[6]);
        let data = unsafe { source(request[3], request[2])? };
        unsafe { transfer_request(request[5], request[7], |app| app.write_bytes(first, offset, data, verification, timeout)) }
    })();
    status(result)
}

/// # Safety
/// The native export ownership contract remains in force. The request has seven words: first block, byte offset, byte count, destination, event mode, timeout, map slot. The destination is writable for byte count bytes.
#[no_mangle]
pub unsafe extern "C" fn xm1_app_nand_read_bytes(request: *mut [u32; 7]) -> i32 {
    let request = unsafe { &*request };
    let result = (|| {
        let first = LogicalBlock::new(request[0])?;
        let offset = ByteOffset::new(request[1])?;
        let timeout = Timeout::new(request[5]);
        request[3].checked_add(request[2]).ok_or(Error::Range)?;
        if request[2] > isize::MAX as u32 || (request[3] == 0 && request[2] != 0) { return Err(Error::Range); }
        let output = if request[2] == 0 { &mut [] } else { unsafe { core::slice::from_raw_parts_mut(request[3] as *mut u8, request[2] as usize) } };
        unsafe { transfer_request(request[4], request[6], |app| app.read_bytes(first, offset, output, timeout)) }
    })();
    status(result)
}

/// # Safety
/// The native export ownership contract remains in force. The request has six words: first block, block count, destination, event mode, timeout, map slot. The destination is writable for block count times BLOCK_BYTES bytes.
#[no_mangle]
pub unsafe extern "C" fn xm1_app_nand_read_blocks(request: *mut [u32; 6]) -> i32 {
    let request = unsafe { &*request };
    let result = (|| {
        let first = LogicalBlock::new(request[0])?;
        let timeout = Timeout::new(request[4]);
        let length = request[1].checked_mul(BLOCK_BYTES).ok_or(Error::Range)?;
        request[2].checked_add(length).ok_or(Error::Range)?;
        if length > isize::MAX as u32 || (request[2] == 0 && length != 0) { return Err(Error::Range); }
        let output = if length == 0 { &mut [] } else { unsafe { core::slice::from_raw_parts_mut(request[2] as *mut u8, length as usize) } };
        unsafe { transfer_request(request[3], request[5], |app| app.read_blocks(first, output, timeout)) }
    })();
    status(result)
}

/// # Safety
/// The native export ownership contract remains in force. The request has four words: first physical block, block count, event mode, timeout. Output is writable for block count plus one words.
#[no_mangle]
pub unsafe extern "C" fn xm1_app_nand_read_bad_blocks(request: *mut [u32; 4], output: *mut u32) -> i32 {
    let request = unsafe { &*request };
    let result = (|| {
        let wait_mode = WaitMode::parse(request[2])?;
        let range = physical::PhysicalRange::new(PhysicalBlock::new(request[0])?, physical::PhysicalBlockCount::new(request[1])?)?;
        let timeout = Timeout::new(request[3]);
        unsafe { physical::with_physical(|nand| {
            nand.set_wait_mode(wait_mode);
            write_volatile(output, 0);
            let mut count = 0;
            nand.scan_bad_blocks(range, |block| {
                count += 1;
                write_volatile(output.add(count), block.index());
                write_volatile(output, count as u32);
            }, timeout)
        }) }
    })();
    status(result)
}

/// # Safety
/// The native export ownership contract remains in force. The eight request words are first physical block, good block count, bad-block budget, destination, bad-block count output, event mode, timeout, observer. The destination is writable for good block count times BLOCK_BYTES bytes.
#[no_mangle]
pub unsafe extern "C" fn xm1_app_nand_read_indexed(request: *mut [u32; 8], use_table: u32) -> i32 {
    let request = unsafe { &mut *request };
    request[4] = 0;
    let result = unsafe { with_state(|app| {
        app.controller.wait_mode = WaitMode::parse(request[5])?;
        if request[7] != 0 { return Err(Error::UnsupportedObserver); }
        if use_table > 1 { return Err(Error::Range); }
        let first = request[0];
        let count = request[1];
        if first.checked_add(count).ok_or(Error::Range)? > 1024 { return Err(Error::Range); }
        let length = count.checked_mul(BLOCK_BYTES).ok_or(Error::Range)?;
        request[3].checked_add(length).ok_or(Error::Range)?;
        if request[3] == 0 && length != 0 { return Err(Error::Range); }
        let output = if length == 0 { &mut [] } else { unsafe { core::slice::from_raw_parts_mut(request[3] as *mut u8, length as usize) } };
        app.controller.require_configured()?;
        let timeout = Timeout::new(request[6]);
        let mut budget = request[2];
        let mut block = first;
        let mut completed = 0;
        let mut read_fault = false;
        while completed != count {
            if block >= 1024 { return Err(Error::Range); }
            let physical = PhysicalBlock(block as u16);
            let bad = if use_table == 0 {
                classify(&unsafe { app.controller.spare(physical.page(0), timeout)? }) == SpareKind::Bad
            } else {
                let table = unsafe { boot_skip::BootSkipTable::from_boot() }.map_err(|_| Error::MapUnavailable)?;
                let bad = table.iter().any(|entry| entry.index() == block);
                bad
            };
            if bad {
                if request[2] != 0 {
                    if budget == 0 { return Err(Error::SkipBudget); }
                    budget -= 1;
                }
                request[4] += 1;
            } else {
                for page in 0..PAGES_PER_BLOCK {
                    let start = completed as usize * BLOCK_BYTES as usize + page as usize * PAGE_BYTES;
                    let destination: &mut [u8; PAGE_BYTES] = (&mut output[start..start + PAGE_BYTES]).try_into().map_err(|_| Error::Range)?;
                    match unsafe { app.controller.read_page(physical.page(page), destination, Transfer::Tagged, timeout) } {
                        Ok(ReadStatus::Clean) => {},
                        Ok(ReadStatus::TaggedFault(_)) => { read_fault = true; break; },
                        Err(error) => return Err(error),
                    }
                }
                completed += 1;
            }
            block += 1;
        }
        if read_fault { Err(Error::ReadFault) } else { Ok(()) }
    }) };
    status(result)
}
