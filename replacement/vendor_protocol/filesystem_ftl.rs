//! General mapped NAND storage. Source qualification is recorded in filesystem_ftl_evidence.json.

use crate::application_nand::{Error as NandError, PhysicalBlock, PhysicalPage, Timeout, BLOCK_BYTES, PAGE_BYTES, PAGES_PER_BLOCK};
use crate::application_nand::physical::PhysicalNand;
use crate::application_nand::memory::{HostConfiguration, ImageProvenance, MemoryNand};
use super::filesystem_format::{FormatError, Sector};
use super::filesystem_runtime::SectorIo;

const MAX_LOGICAL_BLOCKS: usize = 1000;
const DEFAULT_SECTOR_BYTES: u16 = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalBlock(u16);
impl LogicalBlock {
    pub fn new(index: u16) -> Result<Self, FtlError> {
        if usize::from(index) < MAX_LOGICAL_BLOCKS { Ok(Self(index)) } else { Err(FtlError::Range) }
    }
    pub const fn index(self) -> u16 { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FtlGeometry {
    base: PhysicalBlock,
    physical_blocks: u16,
    logical_blocks: u16,
    sector_bytes: u16,
}

impl FtlGeometry {
    pub fn from_mapped_xm1() -> Result<Self, FtlError> {
        Self::new(PhysicalBlock::new(0x13a)?, 0x246, 0x232)?.with_sector_bytes(2048)
    }
    pub fn new(base: PhysicalBlock, physical_blocks: u16, logical_blocks: u16) -> Result<Self, FtlError> {
        if logical_blocks == 0 || usize::from(logical_blocks) > MAX_LOGICAL_BLOCKS
            || logical_blocks >= physical_blocks || base.index() + u32::from(physical_blocks) > 1024 {
            return Err(FtlError::Geometry);
        }
        Ok(Self { base, physical_blocks, logical_blocks, sector_bytes: DEFAULT_SECTOR_BYTES })
    }
    pub fn with_sector_bytes(mut self, sector_bytes: u16) -> Result<Self, FtlError> {
        if !matches!(sector_bytes, 512 | 2048) { return Err(FtlError::Geometry); }
        self.sector_bytes = sector_bytes;
        Ok(self)
    }
    pub const fn sector_bytes(self) -> u16 { self.sector_bytes }
    pub const fn base(self) -> PhysicalBlock { self.base }
    pub const fn physical_blocks(self) -> u16 { self.physical_blocks }
    pub const fn logical_blocks(self) -> u16 { self.logical_blocks }
    pub const fn sector_count(self) -> u32 { self.logical_blocks as u32 * (BLOCK_BYTES / self.sector_bytes as u32) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FtlError { Geometry, Range, NoReplacement, Nand(NandError) }
impl From<NandError> for FtlError { fn from(error: NandError) -> Self { Self::Nand(error) } }
impl From<FtlError> for FormatError {
    fn from(error: FtlError) -> Self {
        Self::Device(match error {
            FtlError::Geometry => 1, FtlError::Range => 5, FtlError::NoReplacement => 10,
            FtlError::Nand(error) => error.request_status() as u32,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpareRecord { Erased, Bad, Mapped(Option<LogicalBlock>) }

pub fn decode_spare(spare: &[u8; 16]) -> SpareRecord {
    if spare.iter().all(|&value| value == 0xff) { return SpareRecord::Erased; }
    if spare[4] != 0xff || spare[5] != 0xff { return SpareRecord::Bad; }
    SpareRecord::Mapped(decode_tag(spare))
}

fn decode_tag(spare: &[u8; 16]) -> Option<LogicalBlock> {
    let first = u16::from_be_bytes([spare[0], spare[1]]);
    let second = u16::from_be_bytes([spare[2], spare[3]]);
    let even = |tag: u16| tag.count_ones() & 1 == 0;
    let tag = if first == second || !even(second) {
        even(first).then_some(first)
    } else if !even(first) { Some(second) } else { None };
    tag.and_then(|tag| LogicalBlock::new((tag >> 1) & 0xf7ff).ok())
}

pub fn encode_spare(logical: LogicalBlock) -> [u8; 16] {
    let tag = (logical.0 << 1) | 0x1000;
    let tag = tag | (tag.count_ones() as u16 & 1);
    let mut spare = [0xff; 16];
    spare[..2].copy_from_slice(&tag.to_be_bytes());
    spare[2..4].copy_from_slice(&tag.to_be_bytes());
    spare
}

pub struct FtlMap {
    geometry: FtlGeometry,
    entries: [Option<PhysicalBlock>; MAX_LOGICAL_BLOCKS],
    free: [u32; 32],
    cursor: u16,
    bad: u16,
}

impl FtlMap {
    fn new(geometry: FtlGeometry) -> Self {
        Self { geometry, entries: [None; MAX_LOGICAL_BLOCKS], free: [0; 32], cursor: u16::MAX, bad: 0 }
    }
    pub fn physical(&self, logical: LogicalBlock) -> Result<Option<PhysicalBlock>, FtlError> {
        if logical.0 >= self.geometry.logical_blocks { return Err(FtlError::Range); }
        Ok(self.entries[usize::from(logical.0)])
    }
    pub const fn bad_blocks(&self) -> u16 { self.bad }
    fn set_free(&mut self, block: PhysicalBlock, free: bool) {
        let relative = (block.index() - self.geometry.base.index()) as u16;
        let word = usize::from(relative / 32);
        let bit = 1 << (relative % 32);
        if free { self.free[word] |= bit; self.cursor = self.cursor.min(relative); }
        else { self.free[word] &= !bit; }
    }
    fn allocate(&mut self) -> Result<PhysicalBlock, FtlError> {
        if self.bad >= self.geometry.physical_blocks - self.geometry.logical_blocks { return Err(FtlError::NoReplacement); }
        for relative in (self.cursor & !7)..self.geometry.physical_blocks {
            if self.free[usize::from(relative / 32)] & (1 << (relative % 32)) != 0 {
                let block = PhysicalBlock::new(self.geometry.base.index() + u32::from(relative))?;
                self.cursor = relative;
                self.set_free(block, false);
                return Ok(block);
            }
        }
        Err(FtlError::NoReplacement)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CacheMedium { Application, Image(HostConfiguration, ImageProvenance) }
/// Dirty slots belong to their mounting medium until a successful flush.
pub struct FtlCacheBlock {
    logical: Option<LogicalBlock>,
    geometry: Option<FtlGeometry>,
    medium: Option<CacheMedium>,
    dirty: bool,
    rank: usize,
    bytes: [u8; BLOCK_BYTES as usize],
}
impl FtlCacheBlock {
    pub const fn new() -> Self { Self { logical: None, geometry: None, medium: None, dirty: false, rank: 0, bytes: [0xff; BLOCK_BYTES as usize] } }
}
impl Default for FtlCacheBlock { fn default() -> Self { Self::new() } }


enum FtlMedium<'nand, 'state> {
    Application(&'nand mut PhysicalNand<'state>),
    Image(&'nand mut MemoryNand<'state>),
}
impl FtlMedium<'_, '_> {
    fn identity(&self) -> CacheMedium {
        match self {
            Self::Application(_) => CacheMedium::Application,
            Self::Image(image) => CacheMedium::Image(image.configuration(), image.provenance()),
        }
    }
    fn read_spare(&mut self, page: PhysicalPage, timeout: Timeout) -> Result<[u8; 16], NandError> {
        match self { Self::Application(nand) => nand.read_spare(page, timeout), Self::Image(image) => image.read_spare(page) }
    }
    fn read_block(&mut self, block: PhysicalBlock, output: &mut [u8], timeout: Timeout) -> Result<(), NandError> {
        match self { Self::Application(nand) => nand.read_block(block, output, timeout), Self::Image(image) => image.read_block(block, output) }
    }
    fn program_page(&mut self, page: PhysicalPage, data: &[u8; PAGE_BYTES], spare: &[u8; 16], timeout: Timeout) -> Result<(), NandError> {
        match self { Self::Application(nand) => nand.program_page(page, data, spare, timeout), Self::Image(image) => image.program_page(page, data, spare) }
    }
    fn erase_block(&mut self, block: PhysicalBlock, timeout: Timeout) -> Result<(), NandError> {
        match self { Self::Application(nand) => nand.erase_block(block, timeout), Self::Image(image) => image.erase_block(block) }
    }
    fn mark_bad(&mut self, block: PhysicalBlock, timeout: Timeout) -> Result<(), NandError> {
        match self { Self::Application(nand) => nand.mark_bad(block, timeout), Self::Image(image) => image.mark_bad(block, timeout) }
    }
}

pub struct FilesystemFtl<'nand, 'state, 'cache> {
    nand: FtlMedium<'nand, 'state>,
    map: FtlMap,
    cache: &'cache mut [FtlCacheBlock],
    timeout: Timeout,
}

impl<'nand, 'state, 'cache> FilesystemFtl<'nand, 'state, 'cache> {
    pub fn mount(nand: &'nand mut PhysicalNand<'state>, geometry: FtlGeometry, cache: &'cache mut [FtlCacheBlock], timeout: Timeout) -> Result<Self, FtlError> {
        Self::mount_medium(FtlMedium::Application(nand), geometry, cache, timeout)
    }
    pub fn mount_image(image: &'nand mut MemoryNand<'state>, geometry: FtlGeometry, cache: &'cache mut [FtlCacheBlock], timeout: Timeout) -> Result<Self, FtlError> {
        Self::mount_medium(FtlMedium::Image(image), geometry, cache, timeout)
    }
    fn mount_medium(nand: FtlMedium<'nand, 'state>, geometry: FtlGeometry, cache: &'cache mut [FtlCacheBlock], timeout: Timeout) -> Result<Self, FtlError> {
        if cache.is_empty() { return Err(FtlError::Geometry); }
        let medium = nand.identity();
        for (rank, slot) in cache.iter_mut().enumerate() {
            if slot.dirty {
                if slot.geometry != Some(geometry) || slot.medium != Some(medium)
                    || slot.logical.is_none_or(|logical| logical.0 >= geometry.logical_blocks) {
                    return Err(FtlError::Geometry);
                }
            } else { slot.logical = None; }
            slot.geometry = Some(geometry);
            slot.medium = Some(medium);
            slot.rank = rank;
        }
        let mut engine = Self { nand, map: FtlMap::new(geometry), cache, timeout };
        for relative in 0..geometry.physical_blocks {
            let block = PhysicalBlock::new(geometry.base.index() + u32::from(relative))?;
            let first = decode_spare(&engine.nand.read_spare(PhysicalPage::new(block.index() * PAGES_PER_BLOCK)?, timeout)?);
            match first {
                SpareRecord::Bad => { engine.map.bad += 1; continue; },
                SpareRecord::Erased => {},
                SpareRecord::Mapped(first) => {
                    let last = decode_tag(&engine.nand.read_spare(PhysicalPage::new((block.index() + 1) * PAGES_PER_BLOCK - 1)?, timeout)?);
                    if let Some(logical) = first {
                        if logical.0 < geometry.logical_blocks && last == Some(logical)
                            && engine.map.entries[usize::from(logical.0)].is_none() {
                            engine.map.entries[usize::from(logical.0)] = Some(block);
                            continue;
                        }
                    }
                    if !engine.erase_or_retire(block)? { continue; }
                },
            }
            engine.map.set_free(block, true);
        }
        Ok(engine)
    }
    pub fn map(&self) -> &FtlMap { &self.map }
    pub const fn geometry(&self) -> FtlGeometry { self.map.geometry }
    fn retire(&mut self, block: PhysicalBlock) -> Result<(), FtlError> {
        self.map.set_free(block, false);
        self.map.bad += 1;
        self.nand.mark_bad(block, self.timeout)?;
        Ok(())
    }
    fn erase_or_retire(&mut self, block: PhysicalBlock) -> Result<bool, FtlError> {
        match self.nand.erase_block(block, self.timeout) {
            Ok(()) => Ok(true),
            Err(NandError::ProgramFailed) => { self.retire(block)?; Ok(false) },
            Err(error) => Err(error.into()),
        }
    }
    fn flush_slot(&mut self, index: usize) -> Result<(), FtlError> {
        if !self.cache[index].dirty { return Ok(()); }
        let logical = self.cache[index].logical.ok_or(FtlError::Range)?;
        let previous = self.map.physical(logical)?;
        let spare = encode_spare(logical);
        loop {
            let candidate = self.map.allocate()?;
            if decode_spare(&self.nand.read_spare(PhysicalPage::new(candidate.index() * PAGES_PER_BLOCK)?, self.timeout)?) == SpareRecord::Bad {
                self.map.bad += 1;
                continue;
            }
            let mut failed = false;
            for page in 0..PAGES_PER_BLOCK {
                let offset = page as usize * PAGE_BYTES;
                let bytes = (&self.cache[index].bytes[offset..offset + PAGE_BYTES]).try_into().map_err(|_| FtlError::Range)?;
                match self.nand.program_page(PhysicalPage::new(candidate.index() * PAGES_PER_BLOCK + page)?, bytes, &spare, self.timeout) {
                    Ok(()) => {},
                    Err(NandError::ProgramFailed | NandError::VerifyFailed) => { failed = true; break; },
                    Err(error) => return Err(error.into()),
                }
            }
            if failed { self.retire(candidate)?; continue; }
            if let Some(previous) = previous {
                if self.erase_or_retire(previous)? { self.map.set_free(previous, true); }
            }
            self.map.entries[usize::from(logical.0)] = Some(candidate);
            self.cache[index].dirty = false;
            return Ok(());
        }
    }
    fn cached(&mut self, logical: LogicalBlock) -> Result<usize, FtlError> {
        let index = if let Some(index) = self.cache.iter().position(|slot| slot.logical == Some(logical)) {
            index
        } else {
            let index = self.cache.iter().position(|slot| slot.logical.is_none()).unwrap_or_else(|| {
                self.cache.iter().enumerate().max_by_key(|(_, slot)| slot.rank).map(|(index, _)| index).unwrap_or(0)
            });
            self.flush_slot(index)?;
            self.cache[index].logical = None;
            match self.map.physical(logical)? {
                Some(block) => self.nand.read_block(block, &mut self.cache[index].bytes, self.timeout)?,
                None => self.cache[index].bytes.fill(0xff),
            }
            self.cache[index].logical = Some(logical);
            index
        };
        let rank = self.cache[index].rank;
        for slot in self.cache.iter_mut() { if slot.rank < rank { slot.rank += 1; } }
        self.cache[index].rank = 0;
        Ok(index)
    }
    fn range(&self, sector: Sector, bytes: usize) -> Result<(), FtlError> {
        let sector_bytes = usize::from(self.geometry().sector_bytes);
        if bytes % sector_bytes != 0 { return Err(FtlError::Range); }
        let sectors = u32::try_from(bytes / sector_bytes).map_err(|_| FtlError::Range)?;
        if sector.0.checked_add(sectors).ok_or(FtlError::Range)? > self.geometry().sector_count() { return Err(FtlError::Range); }
        Ok(())
    }
    pub fn read(&mut self, sector: Sector, output: &mut [u8]) -> Result<(), FtlError> {
        self.range(sector, output.len())?;
        let sector_bytes = usize::from(self.geometry().sector_bytes);
        for (offset, output) in output.chunks_exact_mut(sector_bytes).enumerate() {
            let byte = (sector.0 + offset as u32) * sector_bytes as u32;
            let logical = LogicalBlock((byte / BLOCK_BYTES) as u16);
            let within = (byte % BLOCK_BYTES) as usize;
            let slot = self.cached(logical)?;
            output.copy_from_slice(&self.cache[slot].bytes[within..within + sector_bytes]);
        }
        Ok(())
    }
    pub fn write(&mut self, sector: Sector, bytes: &[u8]) -> Result<(), FtlError> {
        self.range(sector, bytes.len())?;
        let sector_bytes = usize::from(self.geometry().sector_bytes);
        for (offset, input) in bytes.chunks_exact(sector_bytes).enumerate() {
            let byte = (sector.0 + offset as u32) * sector_bytes as u32;
            let logical = LogicalBlock((byte / BLOCK_BYTES) as u16);
            let within = (byte % BLOCK_BYTES) as usize;
            let slot = self.cached(logical)?;
            self.cache[slot].bytes[within..within + sector_bytes].copy_from_slice(input);
            self.cache[slot].dirty = true;
        }
        Ok(())
    }
    pub fn flush(&mut self) -> Result<(), FtlError> {
        for rank in (0..self.cache.len()).rev() {
            if let Some(index) = self.cache.iter().position(|slot| slot.rank == rank) { self.flush_slot(index)?; }
        }
        Ok(())
    }
}

impl SectorIo for FilesystemFtl<'_, '_, '_> {
    fn sector_bytes(&self) -> u16 { self.geometry().sector_bytes }
    fn read(&mut self, sector: Sector, output: &mut [u8]) -> Result<(), FormatError> { FilesystemFtl::read(self, sector, output).map_err(Into::into) }
    fn write(&mut self, sector: Sector, bytes: &[u8]) -> Result<(), FormatError> { FilesystemFtl::write(self, sector, bytes).map_err(Into::into) }
    fn flush(&mut self) -> Result<(), FormatError> { FilesystemFtl::flush(self).map_err(Into::into) }
}

#[cfg(test)]
#[path = "filesystem_ftl_tests.rs"]
mod tests;
