use core::ptr::read_volatile;

use super::MediaEndpoint;
use super::{classify, read, source, status, with_state, write, ApplicationNand, Error, PhysicalBlock, PhysicalPage, SpareKind, Timeout, Transfer, Verification, WaitMode, BLOCK_BYTES, NAND, PAGE_BYTES, PAGES_PER_BLOCK};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalBlockCount(u16);

impl PhysicalBlockCount {
    pub fn new(count: u32) -> Result<Self, Error> {
        if count <= 1024 { Ok(Self(count as u16)) } else { Err(Error::Range) }
    }
    pub const fn blocks(self) -> u32 { self.0 as u32 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalRange {
    first: PhysicalBlock,
    count: PhysicalBlockCount,
}

impl PhysicalRange {
    pub fn new(first: PhysicalBlock, count: PhysicalBlockCount) -> Result<Self, Error> {
        if first.index() + count.blocks() <= 1024 { Ok(Self { first, count }) } else { Err(Error::Range) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadBlockCount(u16);

impl BadBlockCount {
    pub const ZERO: Self = Self(0);
    pub const fn blocks(self) -> u32 { self.0 as u32 }
    fn increment(&mut self) -> Result<(), Error> {
        if self.0 == 1024 { return Err(Error::Range); }
        self.0 += 1;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadBlockBudget { Unlimited, Limited(PhysicalBlockCount) }

impl BadBlockBudget {
    pub fn from_request(value: u32) -> Result<Self, Error> {
        if value == 0 { Ok(Self::Unlimited) } else { PhysicalBlockCount::new(value).map(Self::Limited) }
    }
    fn consume(&mut self) -> Result<(), Error> {
        match self {
            Self::Unlimited => Ok(()),
            Self::Limited(count) if count.0 != 0 => { count.0 -= 1; Ok(()) },
            Self::Limited(_) => Err(Error::SkipBudget),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockState { Erased, Programmed, Bad }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErasePolicy { PreserveErased, Force, LiveConfiguration }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpareLayout { DataOnly, Interleaved }

impl SpareLayout {
    fn page_bytes(self) -> usize {
        PAGE_BYTES + if self == Self::Interleaved { 64 } else { 0 }
    }
}

pub struct PhysicalNand<'a> {
    application: &'a mut ApplicationNand,
}

/// # Safety
/// The native export memory and OSAL contract remains in force. LiveConfiguration requires the initialized configuration-byte pointer at 0x96ca50 and readable byte 0x276. The closure does not re-enter NAND exports.
pub unsafe fn with_physical<T>(operation: impl FnOnce(&mut PhysicalNand<'_>) -> Result<T, Error>) -> Result<T, Error> {
    unsafe { with_state(|application| {
        application.controller.require_configured()?;
        operation(&mut PhysicalNand { application })
    }) }
}

impl PhysicalNand<'_> {
    pub fn set_wait_mode(&mut self, mode: WaitMode) { self.application.controller.wait_mode = mode; }

    pub fn read_page(&mut self, page: PhysicalPage, output: &mut [u8; PAGE_BYTES], timeout: Timeout) -> Result<(), Error> {
        unsafe { self.application.controller.read_page(page, output, Transfer::Tagged, timeout)?.require_clean() }
    }

    pub fn read_spare(&mut self, page: PhysicalPage, timeout: Timeout) -> Result<[u8; 16], Error> {
        unsafe { self.application.controller.spare(page, timeout) }
    }

    pub fn program_page(&mut self, page: PhysicalPage, data: &[u8; PAGE_BYTES], spare: &[u8; 16], timeout: Timeout) -> Result<(), Error> {
        unsafe {
            self.application.controller.set_spare(spare);
            self.application.controller.program_page(page, data, Transfer::Tagged, timeout)
        }
    }

    pub fn erase_block(&mut self, block: PhysicalBlock, timeout: Timeout) -> Result<(), Error> {
        unsafe { self.application.controller.erase(block, timeout) }
    }

    pub fn block_state(&mut self, block: PhysicalBlock, timeout: Timeout) -> Result<BlockState, Error> {
        let spare = unsafe { self.application.controller.spare(block.page(0), timeout)? };
        Ok(match classify(&spare) { SpareKind::Erased => BlockState::Erased, SpareKind::Mapped => BlockState::Programmed, SpareKind::Bad => BlockState::Bad })
    }

    pub fn read_block(&mut self, block: PhysicalBlock, output: &mut [u8], timeout: Timeout) -> Result<(), Error> {
        if output.len() != BLOCK_BYTES as usize { return Err(Error::Range); }
        for (page, output) in output.chunks_exact_mut(PAGE_BYTES).enumerate() {
            let output: &mut [u8; PAGE_BYTES] = output.try_into().map_err(|_| Error::Range)?;
            unsafe { self.application.controller.read_page(block.page(page as u32), output, Transfer::Tagged, timeout)?.require_clean()?; }
        }
        Ok(())
    }

    /// Data transfer uses the controller's masked mode. Interleaved records reserve 64 spare bytes; only the first 16 are written.
    pub fn read_with_spare(&mut self, range: PhysicalRange, layout: SpareLayout, output: &mut [u8], timeout: Timeout) -> Result<(), Error> {
        let pages = range.count.blocks() * PAGES_PER_BLOCK;
        let stride = layout.page_bytes();
        if output.len() != pages as usize * stride { return Err(Error::Range); }
        let first_page = range.first.index() * PAGES_PER_BLOCK;
        for index in 0..pages {
            let page = PhysicalPage(first_page + index);
            let offset = index as usize * stride;
            let data: &mut [u8; PAGE_BYTES] = (&mut output[offset..offset + PAGE_BYTES]).try_into().map_err(|_| Error::Range)?;
            unsafe { self.application.controller.read_page(page, data, Transfer::Masked, timeout)?.require_clean()?; }
            if layout == SpareLayout::Interleaved {
                let spare = unsafe { self.application.controller.spare(page, timeout)? };
                output[offset + PAGE_BYTES..offset + PAGE_BYTES + spare.len()].copy_from_slice(&spare);
            }
        }
        Ok(())
    }

    pub fn mark_bad(&mut self, block: PhysicalBlock, timeout: Timeout) -> Result<(), Error> {
        unsafe { self.application.controller.mark_bad(block, &mut self.application.page, Transfer::Tagged, timeout) }
    }

    pub fn erase_blocks(&mut self, range: PhysicalRange, policy: ErasePolicy, timeout: Timeout, bad: &mut BadBlockCount) -> Result<(), Error> {
        *bad = BadBlockCount::ZERO;
        for index in range.first.index()..range.first.index() + range.count.blocks() {
            let block = PhysicalBlock::new(index)?;
            let state = self.block_state(block, timeout)?;
            if state == BlockState::Bad { bad.increment()?; continue; }
            let erase = state == BlockState::Programmed || match policy {
                ErasePolicy::PreserveErased => false,
                ErasePolicy::Force => true,
                ErasePolicy::LiveConfiguration => unsafe { read_volatile((read(0x96ca50) as *const u8).add(0x276)) != 0 },
            };
            if erase {
                match unsafe { self.application.controller.erase(block, timeout) } {
                    Ok(()) => {},
                    Err(Error::ProgramFailed) => { self.mark_bad(block, timeout)?; bad.increment()?; },
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(())
    }

    fn matches_block(&mut self, block: PhysicalBlock, expected: &[u8], timeout: Timeout) -> Result<bool, Error> {
        for (page, expected) in expected.chunks_exact(PAGE_BYTES).enumerate() {
            let status = unsafe { self.application.controller.read_page(block.page(page as u32), &mut self.application.comparison, Transfer::Tagged, timeout)? };
            if !status.is_complete() || expected != self.application.comparison.as_slice() { return Ok(false); }
        }
        Ok(true)
    }

    fn program_block(&mut self, block: PhysicalBlock, data: &[u8], timeout: Timeout) -> Result<(), Error> {
        unsafe {
            for offset in [0x40, 0x44, 0x48, 0x4c] { write(NAND + offset, u32::MAX); }
        }
        for (page, data) in data.chunks_exact(PAGE_BYTES).enumerate() {
            let data: &[u8; PAGE_BYTES] = data.try_into().map_err(|_| Error::Range)?;
            unsafe { self.application.controller.program_page(block.page(page as u32), data, Transfer::Tagged, timeout)?; }
        }
        Ok(())
    }

    pub fn write_blocks(&mut self, first: PhysicalBlock, data: &[u8], verification: Verification, mut budget: BadBlockBudget, timeout: Timeout, bad: &mut BadBlockCount) -> Result<(), Error> {
        *bad = BadBlockCount::ZERO;
        if data.len() % BLOCK_BYTES as usize != 0 { return Err(Error::Range); }
        let count = PhysicalBlockCount::new(u32::try_from(data.len() / BLOCK_BYTES as usize).map_err(|_| Error::Range)?)?;
        PhysicalRange::new(first, count)?;
        let mut index = first.index();
        let mut remaining = data;
        while !remaining.is_empty() {
            let block = PhysicalBlock::new(index)?;
            index += 1;
            let expected = &remaining[..BLOCK_BYTES as usize];
            match self.block_state(block, timeout)? {
                BlockState::Bad => { budget.consume()?; bad.increment()?; continue; },
                BlockState::Erased => {},
                BlockState::Programmed => {
                    if self.matches_block(block, expected, timeout)? {
                        remaining = &remaining[BLOCK_BYTES as usize..];
                        continue;
                    }
                    match unsafe { self.application.controller.erase(block, timeout) } {
                        Ok(()) => {},
                        Err(Error::ProgramFailed) => {
                            self.mark_bad(block, timeout)?;
                            budget.consume()?;
                            bad.increment()?;
                            continue;
                        },
                        Err(error) => return Err(error),
                    }
                }
            }
            match self.program_block(block, expected, timeout) {
                Ok(()) => {},
                Err(Error::ProgramFailed) => {
                    self.mark_bad(block, timeout)?;
                    budget.consume()?;
                    bad.increment()?;
                    continue;
                },
                Err(error) => return Err(error),
            }
            if verification == Verification::ReadBack {
                let comparison = self.matches_block(block, expected, timeout);
                let state = self.block_state(block, timeout)?;
                if !comparison? || state == BlockState::Bad {
                    self.mark_bad(block, timeout)?;
                    budget.consume()?;
                    bad.increment()?;
                    continue;
                }
            }
            remaining = &remaining[BLOCK_BYTES as usize..];
        }
        Ok(())
    }

    pub fn scan_bad_blocks(&mut self, range: PhysicalRange, mut visit: impl FnMut(PhysicalBlock), timeout: Timeout) -> Result<(), Error> {
        for index in range.first.index()..range.first.index() + range.count.blocks() {
            let block = PhysicalBlock::new(index)?;
            if self.block_state(block, timeout)? == BlockState::Bad { visit(block); }
        }
        Ok(())
    }
}

/// # Safety
/// The native export contract remains in force. The request words are first physical block, block count, bad-block count output, event mode, timeout. Live configuration policy byte 0x276 is initialized.
#[no_mangle]
pub unsafe extern "C" fn xm1_app_nand_erase_physical(request: *mut [u32; 5]) -> i32 {
    let request = unsafe { &mut *request };
    request[2] = 0;
    let mut bad = BadBlockCount::ZERO;
    let result = (|| {
        let wait_mode = WaitMode::parse(request[3])?;
        let range = PhysicalRange::new(PhysicalBlock::new(request[0])?, PhysicalBlockCount::new(request[1])?)?;
        let timeout = Timeout::new(request[4]);
        unsafe { with_physical(|nand| {
            nand.set_wait_mode(wait_mode);
            nand.erase_blocks(range, ErasePolicy::LiveConfiguration, timeout, &mut bad)
        }) }
    })();
    request[2] = bad.blocks();
    status(result)
}

/// # Safety
/// The native export contract remains in force. The request words are first physical block, good block count, bad-block budget, source, verification, bad-block count output, event mode, timeout. Source is readable for good block count times BLOCK_BYTES bytes.
#[no_mangle]
pub unsafe extern "C" fn xm1_app_nand_write_physical(request: *mut [u32; 8]) -> i32 {
    let request = unsafe { &mut *request };
    request[5] = 0;
    let mut bad = BadBlockCount::ZERO;
    let result = (|| {
        let wait_mode = WaitMode::parse(request[6])?;
        let first = PhysicalBlock::new(request[0])?;
        let count = PhysicalBlockCount::new(request[1])?;
        PhysicalRange::new(first, count)?;
        let length = count.blocks().checked_mul(BLOCK_BYTES).ok_or(Error::Range)?;
        let data = unsafe { source(request[3], length)? };
        let verification = Verification::parse(request[4])?;
        let budget = BadBlockBudget::from_request(request[2])?;
        let timeout = Timeout::new(request[7]);
        unsafe { with_physical(|nand| {
            nand.set_wait_mode(wait_mode);
            nand.write_blocks(first, data, verification, budget, timeout, &mut bad)
        }) }
    })();
    request[5] = bad.blocks();
    status(result)
}

/// # Safety
/// The native export contract remains in force. The six request words are first physical block, block count, destination, include-spare boolean, event-mode boolean, timeout. Destination is writable for count times 64 times the selected page stride.
#[no_mangle]
pub unsafe extern "C" fn xm1_app_nand_read_physical(request: *mut [u32; 6]) -> i32 {
    let request = unsafe { &*request };
    let result = (|| {
        let range = PhysicalRange::new(PhysicalBlock::new(request[0])?, PhysicalBlockCount::new(request[1])?)?;
        let layout = if request[3] == 0 { SpareLayout::DataOnly } else { SpareLayout::Interleaved };
        let wait_mode = if request[4] == 0 { WaitMode::Poll } else { WaitMode::Interrupt };
        let length = range.count.blocks() * PAGES_PER_BLOCK * layout.page_bytes() as u32;
        request[2].checked_add(length).ok_or(Error::Range)?;
        if request[2] == 0 && length != 0 { return Err(Error::Range); }
        let output = if length == 0 { &mut [] } else { unsafe { core::slice::from_raw_parts_mut(request[2] as *mut u8, length as usize) } };
        let timeout = Timeout::new(request[5]);
        unsafe { with_physical(|nand| {
            nand.set_wait_mode(wait_mode);
            nand.read_with_spare(range, layout, output, timeout)
        }) }
    })();
    status(result)
}
