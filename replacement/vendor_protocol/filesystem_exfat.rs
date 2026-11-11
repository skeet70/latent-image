//! Original exFAT writer derived from X-M1 1.50 at 0x0070acd4.
//! Proprietary upcase and OEM inputs remain in the owner-supplied image.

use super::filesystem_format::{Cluster, FatKind, FormatControl, FormatError, Layout, MetadataStatus, Sector, VolumeDescriptor, WriteState};
use super::filesystem_runtime::SectorIo;

const IMAGE_BASE: usize = 0x0061c400;
const IMAGE_BYTES: usize = 0x282800;
const UPCASE_ADDRESS: usize = 0x00812e2c;
const PARAMETERS_ADDRESS: usize = 0x00814548;
const UPCASE_BYTES: usize = 0x16cc;

#[derive(Clone, Copy)]
pub struct ExfatTables<'a> {
    upcase: &'a [u8; UPCASE_BYTES],
    parameters: &'a [u8; 16],
}

impl<'a> ExfatTables<'a> {
    /// The caller authenticates image 4 against the SHA-256 in filesystem_exfat_evidence.json.
    pub fn from_image4(image: &'a [u8]) -> Result<Self, FormatError> {
        if image.len() != IMAGE_BYTES { return Err(FormatError::Layout); }
        let upcase = image[UPCASE_ADDRESS - IMAGE_BASE..UPCASE_ADDRESS - IMAGE_BASE + UPCASE_BYTES].try_into().map_err(|_| FormatError::Layout)?;
        let parameters = image[PARAMETERS_ADDRESS - IMAGE_BASE..PARAMETERS_ADDRESS - IMAGE_BASE + 16].try_into().map_err(|_| FormatError::Layout)?;
        Self::checked(upcase, parameters)
    }

    fn checked(upcase: &'a [u8; UPCASE_BYTES], parameters: &'a [u8; 16]) -> Result<Self, FormatError> {
        if checksum(0, upcase) != 0xe619d30d || checksum(0, parameters) != 0xd3e40108 {
            return Err(FormatError::Layout);
        }
        Ok(Self { upcase, parameters })
    }

    /// # Safety
    /// The authenticated X-M1 1.50 image must be readable and immutable at 0x00812e2c..0x008144f8 and 0x00814548..0x00814558 for the returned lifetime.
    pub unsafe fn from_mapped_xm1() -> Result<Self, FormatError> {
        let upcase = unsafe { &*(UPCASE_ADDRESS as *const [u8; UPCASE_BYTES]) };
        let parameters = unsafe { &*(PARAMETERS_ADDRESS as *const [u8; 16]) };
        Self::checked(upcase, parameters)
    }
}

fn checksum(seed: u32, bytes: &[u8]) -> u32 {
    bytes.iter().fold(seed, |sum, &byte| sum.rotate_right(1).wrapping_add(u32::from(byte)))
}
fn put32(out: &mut [u8], at: usize, value: u32) { out[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
fn put64(out: &mut [u8], at: usize, value: u64) { out[at..at + 8].copy_from_slice(&value.to_le_bytes()); }
fn write(io: &mut impl SectorIo, control: &FormatControl, sector: u32, bytes: &[u8]) -> Result<(), FormatError> {
    control.checkpoint()?;
    io.write(Sector(sector), bytes).map_err(|_| FormatError::Write)
}
fn pair(io: &mut impl SectorIo, control: &FormatControl, partition: u32, offset: u32, bytes: &[u8]) -> Result<(), FormatError> {
    write(io, control, partition + offset, bytes)?;
    write(io, control, partition + offset + 12, bytes)
}

pub fn format_exfat(io: &mut impl SectorIo, volume: &mut VolumeDescriptor, layout: Layout, control: &FormatControl, tables: ExfatTables<'_>) -> Result<(), FormatError> {
    let pages = u32::from(layout.cluster_sectors);
    if layout.kind != FatKind::ExFat || io.sector_bytes() != 512 || layout.sector_bytes != 512 || !pages.is_power_of_two() || pages < 256 || layout.partition.0.checked_mul(2).is_none() {
        return Err(FormatError::Layout);
    }
    let partition = layout.partition.0;
    let clusters = volume.sectors.0.checked_sub(partition * 2).ok_or(FormatError::Layout)? / pages;
    if clusters < 3 { return Err(FormatError::Layout); }
    let zero_end = partition.checked_add(layout.fat_sectors.0.checked_mul(2).ok_or(FormatError::Layout)?).and_then(|n| n.checked_add(pages * 3)).ok_or(FormatError::Layout)?;
    if zero_end > volume.sectors.0 { return Err(FormatError::Layout); }
    let mut sector = [0u8; 2048];
    for address in 0..zero_end { write(io, control, address, &sector[..512])?; }
    layout.partition_sector(volume, &mut sector);
    write(io, control, 0, &sector[..512])?;
    layout.boot_sector(volume, &mut sector);
    pair(io, control, partition, 0, &sector[..512])?;
    let mut boot_checksum = 0u32;
    for (offset, &byte) in sector[..512].iter().enumerate() {
        if !matches!(offset, 106 | 107 | 112) { boot_checksum = boot_checksum.rotate_right(1).wrapping_add(u32::from(byte)); }
    }
    sector[..512].fill(0);
    sector[510] = 0x55;
    sector[511] = 0xaa;
    for offset in 1..9 {
        boot_checksum = checksum(boot_checksum, &sector[..512]);
        write(io, control, partition + offset, &sector[..512])?;
    }
    for offset in 13..21 { write(io, control, partition + offset, &sector[..512])?; }
    sector[..512].fill(0);
    sector[..16].copy_from_slice(tables.parameters);
    put32(&mut sector, 16, partition * 256);
    boot_checksum = checksum(boot_checksum, &sector[..512]);
    pair(io, control, partition, 9, &sector[..512])?;
    sector[..512].fill(0);
    pair(io, control, partition, 10, &sector[..512])?;
    for offset in (0..512).step_by(4) { put32(&mut sector, offset, boot_checksum); }
    pair(io, control, partition, 11, &sector[..512])?;
    sector[..512].fill(0);
    put32(&mut sector, 0, 0xfffffff8);
    for offset in [4, 8, 12, 16] { put32(&mut sector, offset, u32::MAX); }
    write(io, control, layout.fat_start.0, &sector[..512])?;

    // The stock bitmap zeroing length uses the truncated byte count.
    let bitmap_floor = clusters / 8;
    let bitmap_pages = bitmap_floor.div_ceil(512);
    let bitmap_clusters = if bitmap_pages == 0 { 1 } else { (bitmap_pages - 1) / pages + 1 };
    sector[..512].fill(0);
    for page in 0..bitmap_pages { write(io, control, layout.data_start.0 + page, &sector[..512])?; }
    let mut upcase_cluster_offset = bitmap_clusters;
    let mut upcase_page = 0u32;
    for bytes in tables.upcase.chunks(512) {
        sector[..512].fill(0);
        sector[..bytes.len()].copy_from_slice(bytes);
        write(io, control, layout.data_start.0 + pages * upcase_cluster_offset + upcase_page, &sector[..512])?;
        if upcase_page < pages { upcase_page += 1; } else { upcase_cluster_offset += 1; upcase_page = 0; }
    }
    sector[..512].fill(0);
    sector[0] = 3;
    sector[32] = 0x81;
    put32(&mut sector, 52, 2);
    let bitmap_bytes = clusters.div_ceil(8);
    put64(&mut sector, 56, u64::from(bitmap_bytes));
    sector[64] = 0x82;
    put32(&mut sector, 68, checksum(0, tables.upcase));
    put32(&mut sector, 84, bitmap_clusters + 2);
    put64(&mut sector, 88, UPCASE_BYTES as u64);
    write(io, control, layout.data_start.0 + (upcase_cluster_offset + 1) * pages, &sector[..512])?;
    sector[..512].fill(0);
    let allocated = upcase_cluster_offset + 2;
    if allocated > 4096 { return Err(FormatError::Layout); }
    for bit in 0..allocated { sector[(bit / 8) as usize] |= 1 << (bit & 7); }
    write(io, control, layout.data_start.0, &sector[..512])?;
    control.checkpoint()?;
    io.flush().map_err(|_| FormatError::Flush)?;

    let mut used = 0u32;
    let mut next_free = None;
    for page in 0..bitmap_bytes.div_ceil(512) {
        control.checkpoint()?;
        io.read(Sector(layout.data_start.0 + page), &mut sector[..512]).map_err(|_| FormatError::Scan)?;
        let first = page * 4096;
        for bit in 0..(clusters - first).min(4096) {
            if sector[(bit / 8) as usize] & (1 << (bit & 7)) != 0 { used += 1; }
            else if next_free.is_none() { next_free = Some(Cluster(first + bit + 2)); }
        }
    }
    volume.layout = Some(layout);
    volume.sector_bytes = 512;
    volume.next_free = next_free.unwrap_or(Cluster(0));
    volume.free_count = clusters - used;
    volume.metadata_status = MetadataStatus(0);
    volume.write_state = WriteState(0);
    Ok(())
}

fn read_sector(io: &mut impl SectorIo, control: &FormatControl, address: u32, bytes: &mut [u8; 512]) -> Result<(), FormatError> {
    control.checkpoint()?;
    io.read(Sector(address), bytes).map_err(|_| FormatError::Scan)
}

fn cluster_sector(layout: Layout, cluster: Cluster) -> Result<u32, FormatError> {
    if cluster.0 < 2 || cluster.0 - 2 >= layout.clusters { return Err(FormatError::Scan); }
    (cluster.0 - 2).checked_mul(u32::from(layout.cluster_sectors)).and_then(|offset| layout.data_start.0.checked_add(offset)).ok_or(FormatError::Scan)
}

struct ChainCursor {
    cluster: Cluster,
    anchor: Cluster,
    power: u32,
    distance: u32,
}
impl ChainCursor {
    fn new(cluster: Cluster) -> Self { Self { cluster, anchor: cluster, power: 1, distance: 0 } }
    fn advance(&mut self, io: &mut impl SectorIo, layout: Layout, control: &FormatControl) -> Result<bool, FormatError> {
        cluster_sector(layout, self.cluster)?;
        let mut bytes = [0; 512];
        let fat_sector = layout.fat_start.0.checked_add(self.cluster.0 / 128).ok_or(FormatError::Scan)?;
        if self.cluster.0 / 128 >= layout.fat_sectors.0 { return Err(FormatError::Scan); }
        read_sector(io, control, fat_sector, &mut bytes)?;
        let offset = (self.cluster.0 % 128) as usize * 4;
        let next = Cluster(u32::from_le_bytes(bytes[offset..offset + 4].try_into().map_err(|_| FormatError::Scan)?));
        if next.0 >= 0xfffffff8 { return Ok(false); }
        cluster_sector(layout, next)?;
        self.distance += 1;
        if next == self.anchor { return Err(FormatError::Scan); }
        if self.distance == self.power {
            self.anchor = next;
            self.power = self.power.saturating_mul(2);
            self.distance = 0;
        }
        self.cluster = next;
        Ok(true)
    }
}

/// Reads the active allocation-bitmap entry and follows its FAT chain.
pub fn scan_free(io: &mut impl SectorIo, volume: &mut VolumeDescriptor, control: &FormatControl) -> Result<(), FormatError> {
    let layout = volume.layout.ok_or(FormatError::Layout)?;
    if layout.kind != FatKind::ExFat || io.sector_bytes() != 512 || layout.sector_bytes != 512 || layout.cluster_sectors == 0 {
        return Err(FormatError::Layout);
    }
    let mut bytes = [0; 512];
    let mut root = ChainCursor::new(layout.root);
    let (bitmap_cluster, bitmap_bytes) = 'directory: loop {
        let first = cluster_sector(layout, root.cluster)?;
        for page in 0..u32::from(layout.cluster_sectors) {
            read_sector(io, control, first.checked_add(page).ok_or(FormatError::Scan)?, &mut bytes)?;
            for entry in bytes.chunks_exact(32) {
                if entry[0] == 0 { return Err(FormatError::Scan); }
                if entry[0] == 0x81 && entry[1] & 1 == 0 {
                    let cluster = Cluster(u32::from_le_bytes(entry[20..24].try_into().map_err(|_| FormatError::Scan)?));
                    let length = u64::from_le_bytes(entry[24..32].try_into().map_err(|_| FormatError::Scan)?);
                    break 'directory (cluster, length);
                }
            }
        }
        if !root.advance(io, layout, control)? { return Err(FormatError::Scan); }
    };
    if bitmap_bytes < u64::from(layout.clusters.div_ceil(8)) { return Err(FormatError::Scan); }
    let mut bitmap = ChainCursor::new(bitmap_cluster);
    let mut remaining = layout.clusters;
    let mut free = 0u32;
    let mut next_free = Cluster(0);
    while remaining != 0 {
        let first = cluster_sector(layout, bitmap.cluster)?;
        for page in 0..u32::from(layout.cluster_sectors) {
            read_sector(io, control, first.checked_add(page).ok_or(FormatError::Scan)?, &mut bytes)?;
            for bit in 0..remaining.min(4096) {
                if bytes[(bit / 8) as usize] & (1 << (bit & 7)) == 0 {
                    if free == 0 { next_free = Cluster(layout.clusters - remaining + bit + 2); }
                    free += 1;
                }
            }
            remaining -= remaining.min(4096);
            if remaining == 0 { break; }
        }
        if remaining != 0 && !bitmap.advance(io, layout, control)? { return Err(FormatError::Scan); }
    }
    while bitmap.advance(io, layout, control)? {}
    volume.free_count = free;
    volume.next_free = next_free;
    Ok(())
}

#[cfg(test)]
#[path = "filesystem_exfat_tests.rs"]
mod tests;
