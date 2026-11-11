//! Sector-backed file lifetime and data operations for X-M1 1.50.
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::filesystem_catalog::{self, Entry};
use crate::filesystem_format::{Cluster, FatKind, FormatError, Layout, Sector};
use crate::filesystem_runtime::SectorIo;
use core::num::NonZeroU32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileHandle(NonZeroU32);
impl FileHandle {
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match NonZeroU32::new(raw) { Some(value) => Some(Self(value)), None => None }
    }
    pub const fn raw(self) -> u32 { self.0.get() }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenMode { Read, Write, Append }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SeekFrom { Start(u64), Current(i64), End(i64) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileError {
    InvalidHandle, TooManyOpenFiles, ReadOnly, WrongMode, EmptyTransfer,
    EndOfFile, InvalidSeek, Cancelled, NoSpace, Read, Write, Chain,
    Metadata, Flush, InvalidGeometry, Device(FormatError),
}
impl FileError {
    pub const fn status(self) -> u32 {
        match self {
            Self::InvalidHandle => 13, Self::TooManyOpenFiles => 3,
            Self::ReadOnly => 17, Self::WrongMode => 36, Self::EmptyTransfer => 35,
            Self::EndOfFile => 37, Self::InvalidSeek => 31, Self::Cancelled => 22,
            Self::NoSpace => 32, Self::Read => 15, Self::Write => 16,
            Self::Chain => 38, Self::Metadata => 12, Self::Flush => 39,
            Self::InvalidGeometry => 19, Self::Device(error) => error.status(),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CloseCompletion { pub handle: FileHandle, pub status: u32 }
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CloseOptions {
    pub final_size: Option<u64>,
    pub modified: Option<crate::filesystem_catalog::FatTimestamp>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileStat { pub bytes: u64, pub position: u64, pub first_cluster: Cluster, pub mode: OpenMode }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteOutcome { pub bytes: usize, pub error: Option<FileError> }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExfatAllocation { pub bitmap_first: Cluster, pub bitmap_bytes: u64 }

pub struct FileCancellation(core::sync::atomic::AtomicBool);
impl Default for FileCancellation { fn default() -> Self { Self::new() } }
impl FileCancellation {
    pub const fn new() -> Self { Self(core::sync::atomic::AtomicBool::new(false)) }
    pub fn cancel(&self) { self.0.store(true, core::sync::atomic::Ordering::Release); }
    fn checkpoint(&self) -> Result<(), FileError> {
        if self.0.load(core::sync::atomic::Ordering::Acquire) { Err(FileError::Cancelled) } else { Ok(()) }
    }
}

fn validate(layout: &Layout, io: &impl SectorIo) -> Result<(), FileError> {
    if !matches!(layout.sector_bytes, 512 | 2048) || layout.sector_bytes != io.sector_bytes()
        || layout.cluster_sectors == 0 || layout.clusters == 0 || layout.clusters > u32::MAX - 2 {
        return Err(FileError::InvalidGeometry);
    }
    Ok(())
}
fn valid_cluster(layout: &Layout, cluster: Cluster) -> bool {
    cluster.0 >= 2 && cluster.0 - 2 < layout.clusters
}
fn fat_address(layout: &Layout, cluster: Cluster) -> Result<(u64, usize), FileError> {
    if !valid_cluster(layout, cluster) { return Err(FileError::Chain); }
    Ok(match layout.kind {
        FatKind::Fat12 => (u64::from(cluster.0) * 3 / 2, 2),
        FatKind::Fat16 => (u64::from(cluster.0) * 2, 2),
        FatKind::Fat32 | FatKind::ExFat => (u64::from(cluster.0) * 4, 4),
    })
}
struct SectorCache { sector: Option<Sector>, bytes: [u8; 2048] }
impl SectorCache {
    fn new() -> Self { Self { sector: None, bytes: [0; 2048] } }
    fn byte(&mut self, io: &mut impl SectorIo, layout: &Layout, sector: Sector, offset: usize) -> Result<u8, FileError> {
        if self.sector != Some(sector) {
            io.read(sector, &mut self.bytes[..usize::from(layout.sector_bytes)]).map_err(|_| FileError::Chain)?;
            self.sector = Some(sector);
        }
        Ok(self.bytes[offset])
    }
}
fn fat_raw(io: &mut impl SectorIo, layout: &Layout, cluster: Cluster, cache: &mut SectorCache) -> Result<u32, FileError> {
    let (address, width) = fat_address(layout, cluster)?;
    let mut encoded = [0; 4];
    let bytes = u64::from(layout.sector_bytes);
    for (index, byte) in encoded[..width].iter_mut().enumerate() {
        let at = address + index as u64;
        let page = u32::try_from(at / bytes).map_err(|_| FileError::Chain)?;
        if page >= layout.fat_sectors.0 { return Err(FileError::Chain); }
        let sector = Sector(layout.fat_start.0.checked_add(page).ok_or(FileError::Chain)?);
        *byte = cache.byte(io, layout, sector, (at % bytes) as usize)?;
    }
    let value = u32::from_le_bytes(encoded);
    Ok(match layout.kind {
        FatKind::Fat12 if cluster.0 & 1 != 0 => (value >> 4) & 0xfff,
        FatKind::Fat12 => value & 0xfff,
        FatKind::Fat32 => value & 0x0fff_ffff,
        _ => value,
    })
}

pub fn set_fat_entry(io: &mut impl SectorIo, layout: &Layout, cluster: Cluster, value: u32) -> Result<(), FileError> {
    validate(layout, io)?;
    let (address, width) = fat_address(layout, cluster)?;
    let bytes = u64::from(layout.sector_bytes);
    let copies = if layout.kind == FatKind::ExFat { 1 } else { 2 };
    for copy in 0..copies {
        let mut cache = [0; 2048];
        let mut encoded = [0; 4];
        for index in 0..width {
            let at = address + index as u64;
            let page = u32::try_from(at / bytes).map_err(|_| FileError::Chain)?;
            if page >= layout.fat_sectors.0 { return Err(FileError::Chain); }
            let sector = layout.fat_start.0.checked_add(copy * layout.fat_sectors.0).and_then(|n| n.checked_add(page)).ok_or(FileError::Chain)?;
            io.read(Sector(sector), &mut cache[..bytes as usize]).map_err(|_| FileError::Chain)?;
            encoded[index] = cache[(at % bytes) as usize];
        }
        let old = u32::from_le_bytes(encoded);
        let updated = match layout.kind {
            FatKind::Fat12 if cluster.0 & 1 != 0 => (old & 15) | ((value & 0xfff) << 4),
            FatKind::Fat12 => (old & 0xf000) | (value & 0xfff),
            FatKind::Fat32 => (old & 0xf000_0000) | (value & 0x0fff_ffff),
            _ => value,
        }.to_le_bytes();
        let first = address / bytes;
        let last = (address + width as u64 - 1) / bytes;
        for page in first..=last {
            let sector = layout.fat_start.0.checked_add(copy * layout.fat_sectors.0).and_then(|n| n.checked_add(page as u32)).ok_or(FileError::Chain)?;
            io.read(Sector(sector), &mut cache[..bytes as usize]).map_err(|_| FileError::Chain)?;
            for (index, byte) in updated[..width].iter().enumerate() {
                let at = address + index as u64;
                if at / bytes == page { cache[(at % bytes) as usize] = *byte; }
            }
            io.write(Sector(sector), &cache[..bytes as usize]).map_err(|_| FileError::Chain)?;
        }
    }
    Ok(())
}
fn chain_at(io: &mut impl SectorIo, layout: &Layout, first: Cluster, index: u64, contiguous: bool) -> Result<Cluster, FileError> {
    if index >= u64::from(layout.clusters) || !valid_cluster(layout, first) { return Err(FileError::Chain); }
    if contiguous {
        let cluster = Cluster(first.0.checked_add(index as u32).ok_or(FileError::Chain)?);
        return if valid_cluster(layout, cluster) { Ok(cluster) } else { Err(FileError::Chain) };
    }
    let mut cluster = first;
    for _ in 0..index {
        cluster = filesystem_catalog::next_cluster(io, layout, cluster).map_err(|_| FileError::Chain)?.ok_or(FileError::Chain)?;
    }
    Ok(cluster)
}
fn bitmap_address(io: &mut impl SectorIo, layout: &Layout, bitmap: ExfatAllocation, cluster: Cluster) -> Result<(Sector, usize, u8), FileError> {
    if !valid_cluster(layout, cluster) { return Err(FileError::Chain); }
    let bit = u64::from(cluster.0 - 2);
    let offset = bit / 8;
    if offset >= bitmap.bitmap_bytes { return Err(FileError::Chain); }
    let cluster_bytes = u64::from(layout.sector_bytes) * u64::from(layout.cluster_sectors);
    let page_cluster = chain_at(io, layout, bitmap.bitmap_first, offset / cluster_bytes, false)?;
    let start = filesystem_catalog::cluster_sector(layout, page_cluster).map_err(|_| FileError::Chain)?;
    let sector = start.0.checked_add(((offset % cluster_bytes) / u64::from(layout.sector_bytes)) as u32).ok_or(FileError::Chain)?;
    Ok((Sector(sector), (offset % u64::from(layout.sector_bytes)) as usize, 1 << (bit % 8)))
}
struct AllocationScan { cache: SectorCache, bitmap_cluster: Option<(u64, Cluster)> }
impl AllocationScan {
    fn new() -> Self { Self { cache: SectorCache::new(), bitmap_cluster: None } }
    fn occupied(&mut self, io: &mut impl SectorIo, layout: &Layout, bitmap: Option<ExfatAllocation>, cluster: Cluster) -> Result<bool, FileError> {
        if layout.kind != FatKind::ExFat { return Ok(fat_raw(io, layout, cluster, &mut self.cache)? != 0); }
        let bitmap = bitmap.ok_or(FileError::InvalidGeometry)?;
        let bit = u64::from(cluster.0 - 2);
        let offset = bit / 8;
        if offset >= bitmap.bitmap_bytes { return Err(FileError::Chain); }
        let cluster_bytes = u64::from(layout.sector_bytes) * u64::from(layout.cluster_sectors);
        let target = offset / cluster_bytes;
        let (prior, first) = match self.bitmap_cluster {
            Some((index, first)) if index <= target => (index, first),
            _ => (0, bitmap.bitmap_first),
        };
        let current = chain_at(io, layout, first, target - prior, false)?;
        self.bitmap_cluster = Some((target, current));
        let first_sector = filesystem_catalog::cluster_sector(layout, current).map_err(|_| FileError::Chain)?;
        let page = (offset % cluster_bytes) / u64::from(layout.sector_bytes);
        let sector = Sector(first_sector.0.checked_add(page as u32).ok_or(FileError::Chain)?);
        let byte = self.cache.byte(io, layout, sector, (offset % u64::from(layout.sector_bytes)) as usize)?;
        Ok(byte & (1 << (bit % 8)) != 0)
    }
}
fn bitmap_set(io: &mut impl SectorIo, layout: &Layout, bitmap: ExfatAllocation, cluster: Cluster, allocated: bool) -> Result<(), FileError> {
    let (sector, offset, mask) = bitmap_address(io, layout, bitmap, cluster)?;
    let mut cache = [0; 2048];
    let bytes = usize::from(layout.sector_bytes);
    io.read(sector, &mut cache[..bytes]).map_err(|_| FileError::Chain)?;
    if allocated { cache[offset] |= mask; } else { cache[offset] &= !mask; }
    io.write(sector, &cache[..bytes]).map_err(|_| FileError::Chain)
}
fn end_marker(layout: &Layout) -> u32 {
    match layout.kind { FatKind::Fat12 => 0xfff, FatKind::Fat16 => 0xffff, FatKind::Fat32 => 0x0fff_ffff, FatKind::ExFat => u32::MAX }
}

pub fn allocate_cluster(io: &mut impl SectorIo, layout: &Layout, bitmap: Option<ExfatAllocation>, after: Option<Cluster>) -> Result<Cluster, FileError> {
    validate(layout, io)?;
    let mut scan = AllocationScan::new();
    for index in 0..layout.clusters {
        let candidate = Cluster(2 + index);
        let allocated = scan.occupied(io, layout, bitmap, candidate)?;
        if allocated { continue; }
        let zero = [0; 2048];
        let start = filesystem_catalog::cluster_sector(layout, candidate).map_err(|_| FileError::Chain)?;
        for page in 0..u32::from(layout.cluster_sectors) {
            io.write(Sector(start.0.checked_add(page).ok_or(FileError::Chain)?), &zero[..usize::from(layout.sector_bytes)]).map_err(|_| FileError::Write)?;
        }
        set_fat_entry(io, layout, candidate, end_marker(layout))?;
        if layout.kind == FatKind::ExFat { bitmap_set(io, layout, bitmap.ok_or(FileError::InvalidGeometry)?, candidate, true)?; }
        if let Some(previous) = after { set_fat_entry(io, layout, previous, candidate.0)?; }
        return Ok(candidate);
    }
    Err(FileError::NoSpace)
}

pub fn release_chain(io: &mut impl SectorIo, layout: &Layout, bitmap: Option<ExfatAllocation>, first: Cluster, bytes: u64, contiguous: bool) -> Result<(), FileError> {
    validate(layout, io)?;
    if first.0 == 0 { return Ok(()); }
    let cluster_bytes = u64::from(layout.sector_bytes) * u64::from(layout.cluster_sectors);
    let count = bytes / cluster_bytes + u64::from(bytes % cluster_bytes != 0);
    let mut cluster = first;
    for index in 0..layout.clusters {
        if !valid_cluster(layout, cluster) { return Err(FileError::Chain); }
        let next = if contiguous {
            if u64::from(index) + 1 >= count { None } else { Some(Cluster(cluster.0.checked_add(1).ok_or(FileError::Chain)?)) }
        } else { filesystem_catalog::next_cluster(io, layout, cluster).map_err(|_| FileError::Chain)? };
        set_fat_entry(io, layout, cluster, 0)?;
        if layout.kind == FatKind::ExFat { bitmap_set(io, layout, bitmap.ok_or(FileError::InvalidGeometry)?, cluster, false)?; }
        match next { Some(value) => cluster = value, None => return Ok(()) }
    }
    Err(FileError::Chain)
}

struct OpenFile {
    handle: FileHandle,
    entry: Entry,
    mode: OpenMode,
    position: u64,
    allocated_bytes: u64,
    pending: bool,
    dirty: bool,
    cluster: Option<(u64, Cluster)>,
    cache_sector: Option<Sector>,
    cache: [u8; 2048],
}

pub struct FileCpu {
    layout: Layout,
    bitmap: Option<ExfatAllocation>,
    files: [Option<OpenFile>; 16],
    generation: u32,
    write_state: crate::filesystem_format::WriteState,
}
impl FileCpu {
    pub fn new(layout: Layout, bitmap: Option<ExfatAllocation>) -> Result<Self, FileError> {
        if !matches!(layout.sector_bytes, 512 | 2048) || layout.cluster_sectors == 0
            || layout.clusters == 0 || (layout.kind == FatKind::ExFat && bitmap.is_none()) {
            return Err(FileError::InvalidGeometry);
        }
        Ok(Self { layout, bitmap, files: core::array::from_fn(|_| None), generation: 0,
            write_state: crate::filesystem_format::WriteState(0) })
    }
    pub fn set_write_state(&mut self, state: crate::filesystem_format::WriteState) { self.write_state = state; }
    fn slot(&self, handle: FileHandle) -> Result<usize, FileError> {
        self.files.iter().position(|file| file.as_ref().is_some_and(|file| file.handle == handle)).ok_or(FileError::InvalidHandle)
    }
    pub fn open(&mut self, io: &mut impl SectorIo, mut entry: Entry, mode: OpenMode) -> Result<FileHandle, FileError> {
        validate(&self.layout, io)?;
        if self.write_state.0 & 1 != 0 { return Err(FileError::Device(FormatError::Device(1))); }
        if entry.attributes.directory() { return Err(FileError::WrongMode); }
        if mode != OpenMode::Read && (entry.attributes.readonly() || self.write_state.0 & 0x10 != 0) {
            return Err(FileError::ReadOnly);
        }
        if entry.size.0 > u64::from(u32::MAX) { return Err(FileError::NoSpace); }
        if entry.valid_size.0 > entry.size.0 { return Err(FileError::Chain); }
        if entry.size.0 != 0 && !valid_cluster(&self.layout, entry.first_cluster) { return Err(FileError::Chain); }
        let slot = self.files.iter().position(Option::is_none).ok_or(FileError::TooManyOpenFiles)?;
        if self.files.iter().flatten().any(|file| file.entry.location == entry.location
            && (file.mode != OpenMode::Read || mode != OpenMode::Read)) {
            return Err(FileError::WrongMode);
        }
        if mode == OpenMode::Write {
            release_chain(io, &self.layout, self.bitmap, entry.first_cluster, entry.size.0, entry.contiguous)?;
            entry.first_cluster = Cluster(0);
            entry.size.0 = 0;
            entry.valid_size.0 = 0;
            entry.contiguous = false;
            entry.attributes.0 |= 0x20;
            filesystem_catalog::commit_file(io, &self.layout, &entry).map_err(|_| FileError::Metadata)?;
        }
        let handle = loop {
            self.generation = self.generation.wrapping_add(1);
            if let Some(handle) = FileHandle::from_raw(self.generation) {
                if self.files.iter().flatten().all(|file| file.handle != handle) { break handle; }
            }
        };
        let cluster_bytes = u64::from(self.layout.sector_bytes) * u64::from(self.layout.cluster_sectors);
        let allocated_bytes = entry.size.0.div_ceil(cluster_bytes) * cluster_bytes;
        self.files[slot] = Some(OpenFile { handle, entry, mode,
            position: if mode == OpenMode::Append { entry.size.0 } else { 0 },
            allocated_bytes, pending: false, dirty: mode == OpenMode::Write,
            cluster: None, cache_sector: None, cache: [0; 2048] });
        Ok(handle)
    }
    pub fn stat(&self, handle: FileHandle) -> Result<FileStat, FileError> {
        let file = self.files[self.slot(handle)?].as_ref().ok_or(FileError::InvalidHandle)?;
        Ok(FileStat { bytes: file.entry.size.0, position: file.position, first_cluster: file.entry.first_cluster, mode: file.mode })
    }
    pub fn metadata(&self, handle: FileHandle) -> Result<&Entry, FileError> {
        let file = self.files[self.slot(handle)?].as_ref().ok_or(FileError::InvalidHandle)?;
        Ok(&file.entry)
    }
    pub fn cancel(&mut self, handle: FileHandle) -> Result<(), FileError> {
        let slot = self.slot(handle)?;
        self.files[slot].as_mut().ok_or(FileError::InvalidHandle)?.pending = true;
        Ok(())
    }
    pub fn seek(&mut self, io: &mut impl SectorIo, handle: FileHandle, from: SeekFrom) -> Result<u64, FileError> {
        validate(&self.layout, io)?;
        let slot = self.slot(handle)?;
        let file = self.files[slot].as_mut().ok_or(FileError::InvalidHandle)?;
        let end = file.entry.size.0;
        let target = match from {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::Current(delta) => file.position.checked_add_signed(delta),
            SeekFrom::End(delta) if delta <= 0 => end.checked_add_signed(delta),
            SeekFrom::End(_) => None,
        }.filter(|position| *position <= end).ok_or(FileError::InvalidSeek)?;
        if target < end {
            file_cluster(io, &self.layout, file, target)?;
        }
        file.position = target;
        file.cache_sector = None;
        Ok(target)
    }
    pub fn read(&mut self, io: &mut impl SectorIo, handle: FileHandle, output: &mut [u8]) -> Result<usize, FileError> {
        self.read_controlled(io, handle, output, &FileCancellation::new())
    }
    pub fn read_controlled(&mut self, io: &mut impl SectorIo, handle: FileHandle, output: &mut [u8], cancellation: &FileCancellation) -> Result<usize, FileError> {
        validate(&self.layout, io)?;
        if output.is_empty() { return Err(FileError::EmptyTransfer); }
        let slot = self.slot(handle)?;
        let file = self.files[slot].as_mut().ok_or(FileError::InvalidHandle)?;
        if file.mode == OpenMode::Write { return Err(FileError::WrongMode); }
        if file.pending { return Err(FileError::Cancelled); }
        if file.position >= file.entry.size.0 { return Err(FileError::EndOfFile); }
        let requested = output.len().min((file.entry.size.0 - file.position) as usize);
        let original = file.position;
        let result = read_data(io, &self.layout, file, &mut output[..requested], cancellation);
        if let Err(error) = result {
            file.position = original;
            file.cluster = None;
            file.cache_sector = None;
            return Err(error);
        }
        Ok(requested)
    }
    pub fn write(&mut self, io: &mut impl SectorIo, handle: FileHandle, input: &[u8]) -> WriteOutcome {
        self.write_controlled(io, handle, input, &FileCancellation::new())
    }
    pub fn write_controlled(&mut self, io: &mut impl SectorIo, handle: FileHandle, input: &[u8], cancellation: &FileCancellation) -> WriteOutcome {
        let mut outcome = WriteOutcome { bytes: 0, error: None };
        let result = (|| {
            validate(&self.layout, io)?;
            if input.is_empty() { return Err(FileError::EmptyTransfer); }
            let slot = self.slot(handle)?;
            let file = self.files[slot].as_mut().ok_or(FileError::InvalidHandle)?;
            if file.mode == OpenMode::Read { return Err(FileError::WrongMode); }
            if self.write_state.0 & 0x11 != 0 { return Err(FileError::ReadOnly); }
            if file.pending { return Err(FileError::Cancelled); }
            cancellation.checkpoint()?;
            let end = file.position.checked_add(input.len() as u64).filter(|end| *end <= u64::from(u32::MAX)).ok_or(FileError::NoSpace)?;
            let cluster_bytes = u64::from(self.layout.sector_bytes) * u64::from(self.layout.cluster_sectors);
            let needed = end.div_ceil(cluster_bytes).saturating_sub(file.allocated_bytes / cluster_bytes);
            if needed != 0 {
                let mut free = 0;
                let mut scan = AllocationScan::new();
                for index in 0..self.layout.clusters {
                    cancellation.checkpoint()?;
                    let cluster = Cluster(index + 2);
                    let used = scan.occupied(io, &self.layout, self.bitmap, cluster)?;
                    if !used { free += 1; if free == needed { break; } }
                }
                if free < needed { return Err(FileError::NoSpace); }
            }
            write_data(io, &self.layout, self.bitmap, file, input, &mut outcome.bytes, cancellation)
        })();
        outcome.error = result.err();
        outcome
    }
    pub fn flush(&mut self, io: &mut impl SectorIo, handle: FileHandle) -> Result<(), FileError> {
        validate(&self.layout, io)?;
        let slot = self.slot(handle)?;
        let file = self.files[slot].as_mut().ok_or(FileError::InvalidHandle)?;
        io.flush().map_err(|_| FileError::Flush)?;
        if file.dirty {
            filesystem_catalog::commit_file(io, &self.layout, &file.entry).map_err(|_| FileError::Metadata)?;
            io.flush().map_err(|_| FileError::Flush)?;
            file.dirty = false;
        }
        Ok(())
    }
    pub fn close(&mut self, io: &mut impl SectorIo, handle: FileHandle) -> Result<CloseCompletion, FileError> {
        self.close_with_options(io, handle, CloseOptions::default())
    }
    pub fn close_with_options(&mut self, io: &mut impl SectorIo, handle: FileHandle, options: CloseOptions) -> Result<CloseCompletion, FileError> {
        let slot = self.slot(handle)?;
        let mut file = self.files[slot].take().ok_or(FileError::InvalidHandle)?;
        validate(&self.layout, io)?;
        io.flush().map_err(|_| FileError::Flush)?;
        if let Some(bytes) = options.final_size {
            if file.mode == OpenMode::Read || bytes > file.entry.size.0 { return Err(FileError::InvalidSeek); }
            truncate_file(io, &self.layout, self.bitmap, &mut file, bytes)?;
        }
        if let Some(timestamp) = options.modified {
            if file.mode != OpenMode::Read { file.entry.modified = timestamp; file.dirty = true; }
        }
        if file.dirty {
            filesystem_catalog::commit_file(io, &self.layout, &file.entry).map_err(|_| FileError::Metadata)?;
            io.flush().map_err(|_| FileError::Flush)?;
        }
        Ok(CloseCompletion { handle, status: 0 })
    }
}

fn file_cluster(io: &mut impl SectorIo, layout: &Layout, file: &mut OpenFile, position: u64) -> Result<Cluster, FileError> {
    let cluster_bytes = u64::from(layout.sector_bytes) * u64::from(layout.cluster_sectors);
    let target = position / cluster_bytes;
    let (index, first) = match file.cluster {
        Some((index, cluster)) if index <= target => (index, cluster),
        _ => (0, file.entry.first_cluster),
    };
    let cluster = chain_at(io, layout, first, target - index, file.entry.contiguous)?;
    file.cluster = Some((target, cluster));
    Ok(cluster)
}
fn data_sector(io: &mut impl SectorIo, layout: &Layout, file: &mut OpenFile) -> Result<Sector, FileError> {
    let position = file.position;
    let cluster = file_cluster(io, layout, file, position)?;
    let first = filesystem_catalog::cluster_sector(layout, cluster).map_err(|_| FileError::Chain)?;
    let page = (file.position / u64::from(layout.sector_bytes)) % u64::from(layout.cluster_sectors);
    Ok(Sector(first.0.checked_add(page as u32).ok_or(FileError::Chain)?))
}
fn read_data(io: &mut impl SectorIo, layout: &Layout, file: &mut OpenFile, output: &mut [u8], cancellation: &FileCancellation) -> Result<(), FileError> {
    let bytes = usize::from(layout.sector_bytes);
    let mut done = 0;
    while done < output.len() {
        if file.pending { return Err(FileError::Cancelled); }
        cancellation.checkpoint()?;
        if file.position >= file.entry.valid_size.0 {
            output[done..].fill(0);
            file.position += (output.len() - done) as u64;
            return Ok(());
        }
        let sector = data_sector(io, layout, file).map_err(|_| FileError::Read)?;
        let offset = (file.position % bytes as u64) as usize;
        let count = (bytes - offset).min(output.len() - done).min((file.entry.valid_size.0 - file.position) as usize);
        if offset == 0 && count == bytes {
            io.read(sector, &mut output[done..done + count]).map_err(|_| FileError::Read)?;
        } else {
            if file.cache_sector != Some(sector) {
                io.read(sector, &mut file.cache[..bytes]).map_err(|_| FileError::Read)?;
                file.cache_sector = Some(sector);
            }
            output[done..done + count].copy_from_slice(&file.cache[offset..offset + count]);
        }
        file.position += count as u64;
        done += count;
    }
    Ok(())
}
fn extend_file(io: &mut impl SectorIo, layout: &Layout, bitmap: Option<ExfatAllocation>, file: &mut OpenFile) -> Result<(), FileError> {
    let cluster_bytes = u64::from(layout.sector_bytes) * u64::from(layout.cluster_sectors);
    let count = file.allocated_bytes / cluster_bytes;
    if file.entry.contiguous && count != 0 {
        for index in 0..count {
            let cluster = Cluster(file.entry.first_cluster.0.checked_add(index as u32).ok_or(FileError::Chain)?);
            let next = if index + 1 == count { end_marker(layout) } else { cluster.0 + 1 };
            set_fat_entry(io, layout, cluster, next)?;
        }
        file.entry.contiguous = false;
        file.dirty = true;
    }
    let previous = if count == 0 { None } else {
        let position = file.allocated_bytes - 1;
        Some(file_cluster(io, layout, file, position)?)
    };
    let cluster = allocate_cluster(io, layout, bitmap, previous)?;
    if count == 0 { file.entry.first_cluster = cluster; }
    file.allocated_bytes += cluster_bytes;
    file.cluster = Some((count, cluster));
    file.dirty = true;
    Ok(())
}
fn write_data(io: &mut impl SectorIo, layout: &Layout, bitmap: Option<ExfatAllocation>, file: &mut OpenFile, input: &[u8], done: &mut usize, cancellation: &FileCancellation) -> Result<(), FileError> {
    let bytes = usize::from(layout.sector_bytes);
    initialize_gap(io, layout, file, cancellation)?;
    while *done < input.len() {
        if file.pending { return Err(FileError::Cancelled); }
        cancellation.checkpoint()?;
        if file.position == file.allocated_bytes { extend_file(io, layout, bitmap, file)?; }
        let sector = data_sector(io, layout, file)?;
        let offset = (file.position % bytes as u64) as usize;
        let count = (bytes - offset).min(input.len() - *done);
        if offset == 0 && count == bytes {
            io.write(sector, &input[*done..*done + count]).map_err(|_| FileError::Write)?;
            file.cache_sector = None;
        } else {
            if file.cache_sector != Some(sector) {
                io.read(sector, &mut file.cache[..bytes]).map_err(|_| FileError::Write)?;
                file.cache_sector = Some(sector);
            }
            file.cache[offset..offset + count].copy_from_slice(&input[*done..*done + count]);
            if io.write(sector, &file.cache[..bytes]).is_err() {
                file.cache_sector = None;
                return Err(FileError::Write);
            }
        }
        file.position += count as u64;
        file.entry.size.0 = file.entry.size.0.max(file.position);
        file.entry.valid_size.0 = file.entry.valid_size.0.max(file.position);
        file.entry.attributes.0 |= 0x20;
        file.dirty = true;
        *done += count;
    }
    Ok(())
}

fn truncate_file(io: &mut impl SectorIo, layout: &Layout, bitmap: Option<ExfatAllocation>, file: &mut OpenFile, bytes: u64) -> Result<(), FileError> {
    let cluster_bytes = u64::from(layout.sector_bytes) * u64::from(layout.cluster_sectors);
    let keep = bytes.div_ceil(cluster_bytes);
    let allocated = file.allocated_bytes / cluster_bytes;
    if keep < allocated {
        if keep == 0 {
            release_chain(io, layout, bitmap, file.entry.first_cluster, file.allocated_bytes, file.entry.contiguous)?;
            file.entry.first_cluster = Cluster(0);
        } else {
            let last = chain_at(io, layout, file.entry.first_cluster, keep - 1, file.entry.contiguous)?;
            let tail = chain_at(io, layout, file.entry.first_cluster, keep, file.entry.contiguous)?;
            if !file.entry.contiguous { set_fat_entry(io, layout, last, end_marker(layout))?; }
            release_chain(io, layout, bitmap, tail, (allocated - keep) * cluster_bytes, file.entry.contiguous)?;
        }
        file.allocated_bytes = keep * cluster_bytes;
    }
    file.entry.size.0 = bytes;
    file.entry.valid_size.0 = file.entry.valid_size.0.min(bytes);
    file.dirty = true;
    Ok(())
}

fn initialize_gap(io: &mut impl SectorIo, layout: &Layout, file: &mut OpenFile, cancellation: &FileCancellation) -> Result<(), FileError> {
    let target = file.position;
    let bytes = usize::from(layout.sector_bytes);
    let result = (|| {
        while file.entry.valid_size.0 < target {
            cancellation.checkpoint()?;
            file.position = file.entry.valid_size.0;
            let sector = data_sector(io, layout, file)?;
            let offset = (file.position % bytes as u64) as usize;
            let count = (bytes - offset).min((target - file.position) as usize);
            if offset == 0 { file.cache[..bytes].fill(0); }
            else { io.read(sector, &mut file.cache[..bytes]).map_err(|_| FileError::Write)?; }
            file.cache[offset..offset + count].fill(0);
            io.write(sector, &file.cache[..bytes]).map_err(|_| FileError::Write)?;
            file.entry.valid_size.0 += count as u64;
            file.dirty = true;
        }
        Ok(())
    })();
    file.position = target;
    file.cache_sector = None;
    result
}

#[cfg(test)]
#[path = "filesystem_file_cpu_tests.rs"]
mod tests;
