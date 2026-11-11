//! X-M1 format transactions and filesystem snapshot publication.
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::filesystem_format::{Cluster, FatKind, FormatControl, FormatError, Formatter, Layout, MetadataStatus, Sector, VolumeDescriptor, WriteState};
use crate::filesystem_exfat::ExfatTables;
use crate::filesystem_service::{self, FilesystemService};

pub trait SectorIo {
    fn sector_bytes(&self) -> u16;
    fn read(&mut self, sector: Sector, output: &mut [u8]) -> Result<(), FormatError>;
    fn write(&mut self, sector: Sector, bytes: &[u8]) -> Result<(), FormatError>;
    fn flush(&mut self) -> Result<(), FormatError>;
}

pub struct HostVolume<'a> { bytes: &'a mut [u8], sector_bytes: u16 }
impl<'a> HostVolume<'a> {
    pub fn adopt(bytes: &'a mut [u8], sector_bytes: u16) -> Result<Self, FormatError> {
        if !matches!(sector_bytes, 512 | 2048) || bytes.len() % usize::from(sector_bytes) != 0 {
            return Err(FormatError::InvalidGeometry);
        }
        Ok(Self { bytes, sector_bytes })
    }
    fn range(&self, sector: Sector, length: usize) -> Result<core::ops::Range<usize>, FormatError> {
        if length % usize::from(self.sector_bytes) != 0 { return Err(FormatError::InvalidGeometry); }
        let start = usize::try_from(sector.0).ok().and_then(|sector| sector.checked_mul(usize::from(self.sector_bytes))).ok_or(FormatError::Device(3))?;
        let end = start.checked_add(length).filter(|end| *end <= self.bytes.len()).ok_or(FormatError::Device(3))?;
        Ok(start..end)
    }
    pub fn bytes(&self) -> &[u8] { self.bytes }
}
impl SectorIo for HostVolume<'_> {
    fn sector_bytes(&self) -> u16 { self.sector_bytes }
    fn read(&mut self, sector: Sector, output: &mut [u8]) -> Result<(), FormatError> {
        let range = self.range(sector, output.len())?;
        output.copy_from_slice(&self.bytes[range]); Ok(())
    }
    fn write(&mut self, sector: Sector, bytes: &[u8]) -> Result<(), FormatError> {
        let range = self.range(sector, bytes.len())?;
        self.bytes[range].copy_from_slice(bytes); Ok(())
    }
    fn flush(&mut self) -> Result<(), FormatError> { Ok(()) }
}

fn write(io: &mut impl SectorIo, control: &FormatControl, sector: u32, bytes: &[u8]) -> Result<(), FormatError> {
    control.checkpoint()?;
    io.write(Sector(sector), bytes).map_err(|_| FormatError::Write)
}
fn zero(io: &mut impl SectorIo, control: &FormatControl, first: u32, count: u32, buffer: &mut [u8; 2048]) -> Result<(), FormatError> {
    let bytes = usize::from(io.sector_bytes());
    buffer[..bytes].fill(0);
    for offset in 0..count { write(io, control, first.wrapping_add(offset), &buffer[..bytes])?; }
    Ok(())
}
fn put32(buffer: &mut [u8], offset: usize, value: u32) { buffer[offset..offset+4].copy_from_slice(&value.to_le_bytes()); }

pub fn format(io: &mut impl SectorIo, volume: &mut VolumeDescriptor, control: &FormatControl,
    exfat: Option<ExfatTables<'_>>) -> Result<(), FormatError> {
    if volume.write_state.0 & 1 != 0 { return Err(FormatError::Device(1)); }
    if volume.sectors.0 == 0 { return Err(FormatError::EmptyMedium); }
    if volume.write_state.0 & 0x10 != 0 { return Err(FormatError::WriteProtected); }
    control.checkpoint()?;
    volume.metadata_status = MetadataStatus(0);
    volume.write_state.0 |= 8;
    io.flush().map_err(|_| FormatError::Flush)?;
    let layout = Layout::select(volume)?;
    if io.sector_bytes() != layout.sector_bytes || !matches!(layout.sector_bytes, 512 | 2048) {
        return Err(FormatError::InvalidGeometry);
    }
    if layout.kind == FatKind::ExFat {
        return crate::filesystem_exfat::format_exfat(io, volume, layout, control,
            exfat.ok_or(FormatError::InvalidExtendedLength)?);
    }
    let mut buffer = [0; 2048];
    let bytes = usize::from(layout.sector_bytes);
    let capacity = matches!(layout.formatter, Formatter::Capacity(_));
    if capacity {
        let count = if layout.kind == FatKind::Fat32 {
            layout.data_start.0.wrapping_add(u32::from(layout.cluster_sectors))
        } else { layout.data_start.0 };
        zero(io, control, 0, count, &mut buffer)?;
    }
    layout.partition_sector(volume, &mut buffer);
    write(io, control, 0, &buffer[..bytes])?;
    if !capacity { zero(io, control, 1, layout.partition.0.wrapping_sub(1), &mut buffer)?; }
    layout.boot_sector(volume, &mut buffer);
    write(io, control, layout.partition.0, &buffer[..bytes])?;
    if layout.kind == FatKind::Fat32 {
        write(io, control, layout.partition.0.wrapping_add(6), &buffer[..bytes])?;
        volume.layout = Some(layout);
        volume.sector_bytes = layout.sector_bytes;
        buffer.fill(0);
        put32(&mut buffer, 0, 0x41615252);
        put32(&mut buffer, 484, 0x61417272);
        put32(&mut buffer, 488, u32::MAX);
        put32(&mut buffer, 492, u32::MAX);
        buffer[510] = 0x55; buffer[511] = 0xaa;
        for offset in [1, 7] { write(io, control, layout.partition.0.wrapping_add(offset), &buffer[..bytes])?; }
        buffer[..510].fill(0);
        for offset in [2, 8] { write(io, control, layout.partition.0.wrapping_add(offset), &buffer[..bytes])?; }
        if !capacity {
            zero(io, control, layout.fat_start.0,
                layout.fat_sectors.0.wrapping_mul(2).wrapping_add(65), &mut buffer)?;
        }
        buffer.fill(0);
        if capacity {
            put32(&mut buffer, 0, 0x0fff_fff8);
            put32(&mut buffer, 4, 0x0fff_ffff);
        } else {
            put32(&mut buffer, 0, 0xffff_fff8);
            put32(&mut buffer, 4, 0xffff_ffff);
        }
        put32(&mut buffer, 8, 0x0fff_ffff);
        for copy in 0..2 {
            write(io, control, layout.fat_start.0.wrapping_add(copy * layout.fat_sectors.0), &buffer[..bytes])?;
        }
    } else {
        volume.layout = Some(layout);
        volume.sector_bytes = layout.sector_bytes;
        for copy in 0..2 {
            let start = layout.fat_start.0.wrapping_add(copy * layout.fat_sectors.0);
            buffer.fill(0);
            buffer[..3].copy_from_slice(&[0xf8, 0xff, 0xff]);
            if layout.kind == FatKind::Fat16 { buffer[3] = 0xff; }
            write(io, control, start, &buffer[..bytes])?;
            if !capacity { zero(io, control, start.wrapping_add(1), layout.fat_sectors.0.wrapping_sub(1), &mut buffer)?; }
        }
        if !capacity {
            let root_count = match layout.formatter {
                Formatter::Tables(index) => u32::from(crate::filesystem_format::TABLE_ROOT_PAGES[usize::from(index)]),
                Formatter::Small => 0x2000 / u32::from(layout.sector_bytes),
                _ => u32::from(layout.root_entries) * 32 / u32::from(layout.sector_bytes),
            };
            zero(io, control, layout.fat_start.0.wrapping_add(layout.fat_sectors.0.wrapping_mul(2)), root_count, &mut buffer)?;
        }
    }
    io.flush().map_err(|_| FormatError::Write)?;
    scan_free(io, volume, control)?;
    volume.metadata_status = MetadataStatus(0);
    volume.write_state = WriteState(0);
    Ok(())
}

pub fn scan_free(io: &mut impl SectorIo, volume: &mut VolumeDescriptor, control: &FormatControl) -> Result<(), FormatError> {
    let layout = volume.layout.ok_or(FormatError::Layout)?;
    if layout.kind == FatKind::ExFat || !matches!(layout.sector_bytes, 512 | 2048)
        || io.sector_bytes() != layout.sector_bytes { return Err(FormatError::InvalidGeometry); }
    let bytes = u32::from(layout.sector_bytes);
    let mut buffer = [0u8; 2048];
    let mut loaded = None;
    let mut free = 0;
    let seed = if layout.kind == FatKind::Fat32 { 4 } else { 2 };
    let mut next = None;
    for cluster in 2..layout.clusters.checked_add(2).ok_or(FormatError::Scan)? {
        control.checkpoint()?;
        let (offset, width) = match layout.kind {
            FatKind::Fat12 => (u64::from(cluster) * 3 / 2, 2),
            FatKind::Fat16 => (u64::from(cluster) * 2, 2),
            FatKind::Fat32 => (u64::from(cluster) * 4, 4),
            FatKind::ExFat => unreachable!(),
        };
        let mut encoded = [0u8; 4];
        for index in 0..width {
            let address = offset + index as u64;
            let sector = u32::try_from(address / u64::from(bytes)).map_err(|_| FormatError::Scan)?;
            if sector >= layout.fat_sectors.0 { return Err(FormatError::Scan); }
            if loaded != Some(sector) {
                io.read(Sector(layout.fat_start.0.wrapping_add(sector)), &mut buffer[..bytes as usize]).map_err(|_| FormatError::Scan)?;
                loaded = Some(sector);
            }
            encoded[index] = buffer[(address % u64::from(bytes)) as usize];
        }
        let entry = u32::from_le_bytes(encoded);
        let entry = match layout.kind {
            FatKind::Fat12 if cluster & 1 != 0 => (entry >> 4) & 0xfff,
            FatKind::Fat12 => entry & 0xfff,
            FatKind::Fat32 => entry & 0x0fff_ffff,
            _ => entry,
        };
        if entry == 0 {
            free += 1;
            if cluster >= seed && next.is_none() { next = Some(Cluster(cluster)); }
        }
    }
    volume.free_count = free;
    volume.next_free = next.unwrap_or(Cluster(0));
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityError {
    Layout,
    Medium(FormatError),
    Allocation(crate::filesystem_capacity::AllocationError),
}

pub struct FilesystemRuntime {
    service: FilesystemService,
    volume: VolumeDescriptor,
    control: FormatControl,
    identifier: filesystem_service::VolumeIdentifier,
}
impl FilesystemRuntime {
    pub fn new(volume: VolumeDescriptor, identifier: filesystem_service::VolumeIdentifier) -> Self {
        let mut service = FilesystemService::new();
        service.initialize();
        let mut runtime = Self { service, volume, control: FormatControl::new(), identifier };
        runtime.publish();
        runtime
    }
    pub fn service(&self) -> &FilesystemService { &self.service }
    pub fn service_mut(&mut self) -> &mut FilesystemService { &mut self.service }
    pub fn volume(&self) -> &VolumeDescriptor { &self.volume }
    pub fn cancellation(&self) -> &FormatControl { &self.control }
    pub fn refresh_after_file_change(&mut self, io: &mut impl SectorIo) -> Result<(), FormatError> {
        self.service.invalidate_catalog();
        self.service.invalidate_device_summary();
        self.control.reset();
        let result = io.flush().map_err(|_| FormatError::Flush).and_then(|()| {
            match self.volume.layout.ok_or(FormatError::Layout)?.kind {
                FatKind::ExFat => crate::filesystem_exfat::scan_free(io, &mut self.volume, &self.control),
                _ => scan_free(io, &mut self.volume, &self.control),
            }
        });
        if result.is_ok() { self.publish(); } else { self.service.allocation_capacity_unavailable(); }
        result
    }
    pub fn refresh_allocation_capacity(&mut self, io: &mut impl SectorIo, metadata: &mut [u8]) -> Result<(), CapacityError> {
        use crate::filesystem_capacity::{AllocationEncoding, AllocationGeometry, SectorsPerCluster};
        let layout = self.volume.layout.ok_or(CapacityError::Layout)?;
        let encoding = match layout.kind {
            FatKind::Fat12 => AllocationEncoding::Packed12,
            FatKind::Fat16 => AllocationEncoding::Words16,
            _ => return Err(CapacityError::Layout),
        };
        if !matches!(layout.sector_bytes, 512 | 2048) || io.sector_bytes() != layout.sector_bytes
            || metadata.len() % usize::from(layout.sector_bytes) != 0
            || metadata.len() as u64 > u64::from(layout.fat_sectors.0) * u64::from(layout.sector_bytes) {
            return Err(CapacityError::Layout);
        }
        let geometry = AllocationGeometry {
            sectors_per_cluster: SectorsPerCluster::parse(layout.cluster_sectors).ok_or(CapacityError::Layout)?,
            data_page: layout.data_start.0, data_clusters: layout.clusters, encoding,
        };
        if let Err(error) = io.read(layout.fat_start, metadata) {
            self.service.allocation_capacity_unavailable();
            return Err(CapacityError::Medium(error));
        }
        self.service.refresh_allocation_capacity(geometry, metadata).map_err(CapacityError::Allocation)
    }
    pub fn format(&mut self, io: &mut impl SectorIo, exfat: Option<ExfatTables<'_>>) -> Result<(), FormatError> {
        self.control.reset();
        self.service.invalidate_device_summary();
        self.service.invalidate_catalog();
        let result = format(io, &mut self.volume, &self.control, exfat);
        self.publish();
        result
    }
    pub fn format_with_control(&mut self, io: &mut impl SectorIo, control: &FormatControl,
        exfat: Option<ExfatTables<'_>>) -> Result<(), FormatError> {
        self.service.invalidate_device_summary();
        self.service.invalidate_catalog();
        let result = format(io, &mut self.volume, control, exfat);
        self.publish();
        result
    }
    pub fn publish(&mut self) {
        let volume = &self.volume;
        let format = match volume.format {
            crate::filesystem_format::FormatClass::Tables => filesystem_service::VolumeFormat::Unformatted,
            crate::filesystem_format::FormatClass::Type6 => filesystem_service::VolumeFormat::Type6,
            crate::filesystem_format::FormatClass::Capacity => filesystem_service::VolumeFormat::Type7,
            crate::filesystem_format::FormatClass::Other(value) => filesystem_service::VolumeFormat::from_device(u8::try_from(value).unwrap_or(255)),
        };
        let sector_bytes = if volume.sector_bytes == 0 { 512 } else { volume.sector_bytes };
        let megabytes = volume.sectors.0 / (0x100000 / u32::from(sector_bytes));
        let rounded_megabytes = if megabytes > 0x40000 { 0x80000 } else { megabytes.max(1).next_power_of_two() };
        let cluster_bytes = volume.layout.map_or(0, |layout| u32::from(layout.sector_bytes) * u32::from(layout.cluster_sectors));
        self.service.refresh_device_summary(&filesystem_service::DeviceSummary {
            sectors: filesystem_service::SectorCount(volume.sectors.0),
            write_state: filesystem_service::MediaWriteState::from_device(volume.write_state.0),
            format, metadata_status: volume.metadata_status.0 != 0,
            rounded_megabytes,
            free_bytes: filesystem_service::FreeBytes(u64::from(volume.free_count) * u64::from(cluster_bytes)),
            free_clusters: volume.free_count, cluster_bytes: filesystem_service::ClusterBytes(cluster_bytes),
            geometry: filesystem_service::Geometry { cylinders: volume.geometry.cylinders,
                heads: volume.geometry.heads.get(), sectors_per_track: volume.geometry.sectors_per_track.get(), sector_bytes },
            identifier: self.identifier, metadata_present: volume.layout.is_some(),
        });
        self.service.refresh_cached_capacity();
    }
}

#[cfg(target_arch = "arm")]
struct MappedVolume(u32);
#[cfg(target_arch = "arm")]
struct MappedStorageContext(u32);
#[cfg(target_arch = "arm")]
pub struct NativeFilesystemRuntime {
    runtime: FilesystemRuntime,
    volume: MappedVolume,
    context: Option<MappedStorageContext>,
    control: FormatControl,
}
#[cfg(target_arch = "arm")]
impl NativeFilesystemRuntime {
    /// # Safety
    /// The caller exclusively owns initialized filesystem device B and its storage-service publications.
    pub unsafe fn adopt_internal() -> Result<Self, FormatError> {
        unsafe {
            let word = |address: u32| core::ptr::read_volatile(address as *const u32);
            let half = |address: u32| core::ptr::read_volatile(address as *const u16);
            let byte = |address: u32| core::ptr::read_volatile(address as *const u8);
            let root = word(0x008f_c87c);
            if root == 0 { return Err(FormatError::Device(2)); }
            let address = root.checked_add(0xa4).ok_or(FormatError::Device(2))?;
            if word(address) == u32::MAX { return Err(FormatError::Device(2)); }
            let geometry = crate::filesystem_format::Geometry {
                cylinders: half(address + 0x86),
                heads: core::num::NonZeroU16::new(half(address + 0x88)).ok_or(FormatError::InvalidGeometry)?,
                sectors_per_track: core::num::NonZeroU16::new(half(address + 0x8a)).ok_or(FormatError::InvalidGeometry)?,
            };
            let mut volume = VolumeDescriptor {
                sectors: crate::filesystem_format::SectorCount(word(address + 4)), geometry,
                format: crate::filesystem_format::FormatClass::from_raw(word(address + 0x7c)),
                write_state: WriteState(byte(address + 0x78)), metadata_status: MetadataStatus(word(address + 0x80)),
                sector_bytes: half(address + 0x1c), layout: None,
                next_free: Cluster(word(address + 0x3c)), free_count: word(address + 0x40),
            };
            if matches!(volume.sector_bytes, 512 | 2048) && half(address + 0x48) != 0 {
                let mut layout = Layout::select(&volume)?;
                layout.kind = match byte(address + 0x4b) {
                    0 => FatKind::Fat12, 1 => FatKind::Fat16, 2 => FatKind::Fat32,
                    3 => FatKind::ExFat, _ => return Err(FormatError::InvalidGeometry),
                };
                layout.partition = Sector(word(address + 0x24));
                layout.sector_bytes = volume.sector_bytes;
                layout.cluster_sectors = half(address + 0x48);
                layout.reserved_sectors = half(address + 0x1e);
                layout.root_entries = half(address + 0x20);
                layout.fat_sectors = crate::filesystem_format::SectorCount(word(address + 0x18));
                layout.fat_start = Sector(word(address + 0x28));
                layout.root = Cluster(word(address + 0x2c));
                layout.data_start = Sector(word(address + 0x30));
                layout.clusters = word(address + 0x34);
                volume.layout = Some(layout);
            }
            let mut identifier = [0; 16];
            for (offset, output) in identifier.iter_mut().enumerate() { *output = byte(address + 0x8c + offset as u32); }
            let context = word(0x0091_0204);
            Ok(Self { runtime: FilesystemRuntime::new(volume, filesystem_service::VolumeIdentifier(identifier)),
                volume: MappedVolume(address), context: if context == 0 { None } else { Some(MappedStorageContext(context)) },
                control: FormatControl::from_mapped_xm1() })
        }
    }
    pub fn service(&self) -> &FilesystemService { self.runtime.service() }
    pub fn cancellation(&self) -> &FormatControl { &self.control }
    pub fn refresh_after_file_change(&mut self, io: &mut impl SectorIo) -> Result<(), FormatError> {
        let result = self.runtime.refresh_after_file_change(io);
        unsafe { self.publish(); }
        result
    }
    /// # Safety
    /// Canonical application NAND is configured under lifecycle ownership; no other NAND caller runs until return.
    pub unsafe fn format_nand(&mut self, cache: &mut [crate::filesystem_ftl::FtlCacheBlock],
        timeout: crate::application_nand::Timeout) -> Result<(), FormatError> {
        self.control.reset();
        let geometry = crate::filesystem_ftl::FtlGeometry::from_mapped_xm1().map_err(FormatError::from)?;
        let result = unsafe { crate::application_nand::physical::with_physical(|nand| {
            Ok(match crate::filesystem_ftl::FilesystemFtl::mount(nand, geometry, cache, timeout) {
                Ok(mut io) => self.runtime.format_with_control(&mut io, &self.control, None),
                Err(error) => Err(FormatError::from(error)),
            })
        }) }.map_err(|error| FormatError::Device(error.request_status() as u32))?;
        unsafe { self.publish(); }
        result
    }
    unsafe fn publish(&mut self) {
        unsafe {
            let word = |offset: u32, value: u32| core::ptr::write_volatile((self.volume.0 + offset) as *mut u32, value);
            let half = |offset: u32, value: u16| core::ptr::write_volatile((self.volume.0 + offset) as *mut u16, value);
            let byte = |offset: u32, value: u8| core::ptr::write_volatile((self.volume.0 + offset) as *mut u8, value);
            let volume = self.runtime.volume();
            byte(0x78, volume.write_state.0);
            word(0x80, volume.metadata_status.0);
            word(0x3c, volume.next_free.0);
            word(0x40, volume.free_count);
            if let Some(layout) = volume.layout {
                half(0x1c, layout.sector_bytes);
                half(0x1e, layout.reserved_sectors);
                half(0x20, layout.root_entries);
                word(0x18, layout.fat_sectors.0);
                word(0x24, layout.partition.0 & 0xffff);
                word(0x28, layout.fat_start.0);
                word(0x2c, layout.root.0);
                word(0x30, layout.data_start.0);
                word(0x34, layout.clusters);
                word(0x38, layout.clusters.wrapping_add(1));
                half(0x48, layout.cluster_sectors);
                byte(0x46, 0xf8);
                byte(0x4a, if layout.kind == FatKind::ExFat { 1 } else { 2 });
                byte(0x4b, match layout.kind { FatKind::Fat12 => 0, FatKind::Fat16 => 1, FatKind::Fat32 => 2, FatKind::ExFat => 3 });
                if layout.kind == FatKind::Fat32 { half(0x9c, 1); half(0x9e, 6); }
            }
            if let Some(context) = &self.context {
                let mut record = [0; 0x90];
                let mut media_status = 0;
                let ready = self.runtime.service().media_status(&mut media_status) == filesystem_service::SnapshotStatus::Ready;
                if let Ok(selector) = filesystem_service::SnapshotSelector::parse(40) {
                    if ready && self.runtime.service().query(selector, &mut record).is_ok() {
                        for range in [0..0x3c, 0x70..0x7c] {
                            for index in range {
                                core::ptr::write_volatile((context.0 + 0x16bd8 + index as u32) as *mut u8, record[index]);
                            }
                        }
                        for offset in [0x16c68, 0x16c6b] { core::ptr::write_volatile((context.0 + offset) as *mut u8, 1); }
                    }
                }
                if !ready {
                    for offset in [0x16c68, 0x16c6b] { core::ptr::write_volatile((context.0 + offset) as *mut u8, 0); }
                }
                for offset in [0x1f4, 0x1f5, 0x16c69, 0x16c6a] {
                    core::ptr::write_volatile((context.0 + offset) as *mut u8, 0);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "filesystem_runtime_tests.rs"]
mod tests;
