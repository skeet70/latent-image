//! X-M1 1.50 remote media catalog CPU.
//! Source pin and instruction extents are recorded in the adjacent evidence file.

use crate::filesystem_catalog::{Catalog, CatalogError, Entry, EntryFilter, EntryName, FileSize, FatTimestamp, MediaPath, Pattern};
use crate::filesystem_runtime::SectorIo;
use super::super::discovery::executor_objects::{ObjectHandle, ObjectVolume};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectFormat { Jpeg, Raw, Movie, Directory { image: bool } }
impl ObjectFormat {
    pub const fn wire(self) -> u32 {
        match self { Self::Jpeg => 0x80003801, Self::Raw => 0xb103, Self::Movie => 0x300d, Self::Directory { image: false } => 0x3001, Self::Directory { image: true } => 0x80003001 }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectMetadata {
    pub handle: ObjectHandle,
    pub format: ObjectFormat,
    pub parent: Option<ObjectHandle>,
    pub size: FileSize,
    pub protected: bool,
    pub path: MediaPath,
    pub created: FatTimestamp,
    pub modified: FatTimestamp,
    next: Option<usize>,
}
impl ObjectMetadata {
    pub fn from_entry(directory: &MediaPath, entry: Entry, format: ObjectFormat, handle: ObjectHandle, parent: Option<ObjectHandle>) -> Result<Self, CatalogFailure> {
        let mut object = metadata(directory, entry, format)?;
        object.handle = handle;
        object.parent = parent;
        Ok(object)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogMode { Primary, Mode3 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogState { Empty, Scanning(CatalogMode), Ready(CatalogMode), Cancelled, Failed }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogPublication { pub count: u32, pub mode: CatalogMode }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogFailure { Filesystem(CatalogError), StorageCapacity, Cancelled, InvalidName, InvalidHandle }
impl From<CatalogError> for CatalogFailure {
    fn from(error: CatalogError) -> Self {
        if error == CatalogError::Io(crate::filesystem_format::FormatError::Cancelled) { Self::Cancelled } else { Self::Filesystem(error) }
    }
}

impl CatalogFailure {
    pub const fn status(self) -> i32 {
        match self {
            Self::Cancelled => -11,
            Self::StorageCapacity => -12,
            Self::Filesystem(CatalogError::Io(error)) => error.status() as i32,
            Self::InvalidName | Self::InvalidHandle | Self::Filesystem(_) => -1,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogGeneration(u64);
pub struct CatalogRuntime<'a> {
    objects: &'a mut [Option<ObjectMetadata>],
    scratch: &'a mut [Option<ObjectMetadata>],
    count: usize,
    state: CatalogState,
    generation: CatalogGeneration,
}
impl<'a> CatalogRuntime<'a> {
    pub fn new(objects: &'a mut [Option<ObjectMetadata>], scratch: &'a mut [Option<ObjectMetadata>]) -> Self {
        objects.fill(None);
        scratch.fill(None);
        Self { objects, scratch, count: 0, state: CatalogState::Empty, generation: CatalogGeneration(0) }
    }
    pub fn state(&self) -> CatalogState { self.state }
    pub fn publication_generation(&self) -> CatalogGeneration { self.generation }
    pub fn objects(&self) -> impl Iterator<Item = &ObjectMetadata> { self.objects[..self.count].iter().flatten() }
    pub fn object(&self, handle: ObjectHandle) -> Option<&ObjectMetadata> {
        let index = usize::from(handle.ordinal()).checked_sub(1)?;
        self.objects.get(index)?.as_ref().filter(|object| object.handle == handle)
    }
    pub fn remove(&mut self, handle: ObjectHandle) -> bool {
        let Some(index) = usize::from(handle.ordinal()).checked_sub(1) else { return false; };
        let Some(slot) = self.objects.get_mut(index) else { return false; };
        if slot.as_ref().is_some_and(|object| object.handle == handle) {
            *slot = None;
            self.generation.0 = self.generation.0.wrapping_add(1);
            true
        } else { false }
    }
    pub fn next_handle(&self, volume: ObjectVolume) -> Result<ObjectHandle, CatalogFailure> {
        if self.count >= self.objects.len().min(30000) { return Err(CatalogFailure::StorageCapacity); }
        Ok(ObjectHandle::from_ordinal(volume, (self.count + 1) as u16))
    }
    pub fn insert(&mut self, object: ObjectMetadata) -> Result<(), CatalogFailure> {
        if object.handle != self.next_handle(object.handle.volume())? { return Err(CatalogFailure::InvalidHandle); }
        self.objects[self.count] = Some(object);
        self.count += 1;
        self.generation.0 = self.generation.0.wrapping_add(1);
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.generation.0 = self.generation.0.wrapping_add(1);
        self.scratch.fill(None);
        self.state = CatalogState::Cancelled;
    }
    pub fn teardown(&mut self) {
        self.generation.0 = self.generation.0.wrapping_add(1);
        self.objects.fill(None);
        self.scratch.fill(None);
        self.count = 0;
        self.state = CatalogState::Empty;
    }
}

fn numbered_folder(name: &[u8]) -> Option<usize> {
    if name.len() != 8 || !name.iter().all(|byte| valid_name_byte(*byte)) { return None; }
    decimal(&name[..3])
}
fn numbered_file(name: &[u8]) -> Option<usize> {
    if name.len() != 12 || name[8] != b'.' || !name[..4].iter().all(|byte| valid_name_byte(*byte)) { return None; }
    decimal(&name[4..8])
}
fn valid_name_byte(byte: u8) -> bool { byte.is_ascii_digit() || byte.is_ascii_uppercase() || byte == b'_' }

impl CatalogRuntime<'_> {
    pub fn initialize_primary<I: SectorIo>(&mut self, _: &mut Catalog<'_, I>) -> Result<(), CatalogFailure> {
        self.teardown();
        Ok(())
    }
    pub fn scan_primary<I: SectorIo>(&mut self, fs: &mut Catalog<'_, I>) -> Result<CatalogPublication, CatalogFailure> {
        self.walk(fs, CatalogMode::Primary)
    }
    pub fn initialize_mode3<I: SectorIo>(&mut self, fs: &mut Catalog<'_, I>) -> Result<CatalogPublication, CatalogFailure> {
        self.teardown();
        self.walk(fs, CatalogMode::Mode3)
    }
    pub fn start_mode3(&self) -> Result<CatalogPublication, CatalogFailure> {
        Ok(CatalogPublication { count: self.objects().count() as u32, mode: CatalogMode::Mode3 })
    }
    fn walk<I: SectorIo>(&mut self, fs: &mut Catalog<'_, I>, mode: CatalogMode) -> Result<CatalogPublication, CatalogFailure> {
        self.generation.0 = self.generation.0.wrapping_add(1);
        self.state = CatalogState::Scanning(mode);
        self.objects.fill(None);
        self.count = 0;
        let result = self.walk_inner(fs, mode);
        self.scratch.fill(None);
        self.state = match result {
            Ok(_) => CatalogState::Ready(mode),
            Err(CatalogFailure::Cancelled) => CatalogState::Cancelled,
            Err(_) => CatalogState::Failed,
        };
        result
    }
    fn walk_inner<I: SectorIo>(&mut self, fs: &mut Catalog<'_, I>, mode: CatalogMode) -> Result<CatalogPublication, CatalogFailure> {
        let root = MediaPath::dcim(fs.current_drive());
        let mut folders = [[0u8; 8]; 2000];
        let mut overflow = 0usize;
        let mut scan = fs.scan(&root, Pattern::all(), EntryFilter::Directories)?;
        loop {
            let entry = match scan.next() { Ok(Some(entry)) => entry, Ok(None) => break, Err(error) => { scan.cancel(); return Err(error.into()); } };
            let mut name = [0; 255];
            let Ok(length) = entry.name.ascii(&mut name) else { continue };
            if length > 8 { continue; }
            let mut folder = [0; 8];
            folder[..length].copy_from_slice(&name[..length]);
            if let Some(number) = numbered_folder(&name[..length]) {
                match folders[number][0] {
                    0 => folders[number] = folder,
                    1 => push_folder(&mut folders, &mut overflow, folder),
                    _ => {
                        let previous = folders[number];
                        push_folder(&mut folders, &mut overflow, previous);
                        push_folder(&mut folders, &mut overflow, folder);
                        folders[number][0] = 1;
                    }
                }
            } else { push_folder(&mut folders, &mut overflow, folder); }
        }
        scan.close();
        let folder_count = 1000 + overflow;
        for step in 0..folder_count {
            let index = match mode { CatalogMode::Primary => step, CatalogMode::Mode3 => folder_count - 1 - step };
            let folder = &folders[index];
            if folder[0] <= 1 { continue; }
            let length = folder.iter().position(|byte| *byte == 0).unwrap_or(8);
            let name = core::str::from_utf8(&folder[..length]).map_err(|_| CatalogFailure::InvalidName)?;
            let path = root.join(&EntryName::parse(name)?)?;
            self.walk_directory(fs, &path, mode)?;
            if self.count == 30000 { break; }
        }
        Ok(CatalogPublication { count: self.count as u32, mode })
    }
    fn walk_directory<I: SectorIo>(&mut self, fs: &mut Catalog<'_, I>, directory: &MediaPath, mode: CatalogMode) -> Result<(), CatalogFailure> {
        self.scratch.fill(None);
        let mut numbered = [u32::MAX; 10000];
        let mut used = 0usize;
        let mut image_overflow = OverflowChain::new(30000);
        let mut movie_overflow = OverflowChain::new(10000);
        let filters: &[(ObjectFormat, &str)] = match mode {
            CatalogMode::Primary => &[(ObjectFormat::Jpeg, "*.JPG"), (ObjectFormat::Raw, "*.RAF"), (ObjectFormat::Movie, "*.MOV")],
            CatalogMode::Mode3 => &[(ObjectFormat::Jpeg, "*.JPG"), (ObjectFormat::Movie, "*.MOV")],
        };
        for &(format, pattern) in filters {
            let mut scan = fs.scan(directory, Pattern::parse(pattern)?, EntryFilter::Files)?;
            loop {
                let entry = match scan.next() { Ok(Some(entry)) => entry, Ok(None) => break, Err(error) => { scan.cancel(); return Err(error.into()); } };
                let mut name = [0; 255];
                let number = entry.name.ascii(&mut name).ok().and_then(|length| numbered_file(&name[..length]));
                let overflow = if format == ObjectFormat::Movie { &mut movie_overflow } else { &mut image_overflow };
                let is_overflow = if let Some(number) = number {
                    let previous = numbered[number];
                    if previous == u32::MAX {
                        numbered[number] = used as u32;
                        false
                    } else {
                        if previous != u32::MAX - 1 { overflow.append(self.scratch, previous as usize); }
                        numbered[number] = u32::MAX - 1;
                        true
                    }
                } else { true };
                if is_overflow && overflow.full() { continue; }
                if used == self.scratch.len() { scan.cancel(); return Err(CatalogFailure::StorageCapacity); }
                self.scratch[used] = Some(metadata(directory, entry, format)?);
                if is_overflow { overflow.append(self.scratch, used); }
                used += 1;
            }
            scan.close();
        }
        for step in 0..10000 {
            let number = match mode { CatalogMode::Primary => step, CatalogMode::Mode3 => 9999 - step };
            if numbered[number] < u32::MAX - 1 { self.publish(numbered[number] as usize)?; }
        }
        for chain in [image_overflow, movie_overflow] {
            let mut current = chain.first;
            while let Some(index) = current {
                current = self.scratch[index].as_ref().and_then(|object| object.next);
                self.publish(index)?;
            }
        }
        Ok(())
    }
    fn publish(&mut self, index: usize) -> Result<(), CatalogFailure> {
        if self.count == 30000 { return Ok(()); }
        if self.count == self.objects.len() { return Err(CatalogFailure::StorageCapacity); }
        if let Some(mut object) = self.scratch[index].take() {
            object.next = None;
            object.handle = ObjectHandle::from_ordinal(ObjectVolume::Primary, (self.count + 1) as u16);
            self.objects[self.count] = Some(object);
            self.count += 1;
        }
        Ok(())
    }
}
fn push_folder(folders: &mut [[u8; 8]; 2000], overflow: &mut usize, name: [u8; 8]) {
    if *overflow < 1000 { folders[1000 + *overflow] = name; *overflow += 1; }
}
struct OverflowChain { first: Option<usize>, last: Option<usize>, count: usize, limit: usize }
impl OverflowChain {
    fn new(limit: usize) -> Self { Self { first: None, last: None, count: 0, limit } }
    fn full(&self) -> bool { self.count == self.limit }
    fn append(&mut self, scratch: &mut [Option<ObjectMetadata>], index: usize) {
        if self.full() { return; }
        if let Some(previous) = self.last { scratch[previous].as_mut().unwrap().next = Some(index); } else { self.first = Some(index); }
        self.last = Some(index);
        self.count += 1;
    }
}
fn metadata(directory: &MediaPath, entry: Entry, format: ObjectFormat) -> Result<ObjectMetadata, CatalogFailure> {
    Ok(ObjectMetadata {
        handle: ObjectHandle::from_ordinal(ObjectVolume::Primary, 0), format, parent: None,
        size: entry.size, protected: entry.attributes.protected(), path: directory.join(&entry.name)?,
        created: entry.created, modified: entry.modified, next: None,
    })
}
fn decimal(bytes: &[u8]) -> Option<usize> {
    bytes.iter().try_fold(0usize, |value, byte| byte.is_ascii_digit().then(|| value * 10 + usize::from(*byte - b'0')))
}

#[cfg(test)]
#[path = "vendor_wireless_service_task_interface_modes_catalog_tests.rs"]
mod tests;
