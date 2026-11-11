//! X-M1 volume formatting at 0x0070acd4..0x0070e328.
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3. NONMATCHING C was resolved against authenticated image-4 instructions.

use core::num::NonZeroU16;
use core::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sector(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SectorCount(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cluster(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Geometry {
    pub cylinders: u16,
    pub heads: NonZeroU16,
    pub sectors_per_track: NonZeroU16,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormatClass { Tables, Type6, Capacity, Other(u32) }
impl FormatClass {
    pub const fn from_raw(value: u32) -> Self {
        match value { 0 => Self::Tables, 6 => Self::Type6, 7 => Self::Capacity, n => Self::Other(n) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FatKind { Fat12, Fat16, Fat32, ExFat }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteState(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetadataStatus(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VolumeDescriptor {
    pub sectors: SectorCount,
    pub geometry: Geometry,
    pub format: FormatClass,
    pub write_state: WriteState,
    pub metadata_status: MetadataStatus,
    pub sector_bytes: u16,
    pub layout: Option<Layout>,
    pub next_free: Cluster,
    pub free_count: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormatError {
    InvalidGeometry,
    EmptyMedium,
    WriteProtected,
    Layout,
    Cancelled,
    Finish,
    Write,
    Finalize,
    Scan,
    Flush,
    InvalidExtendedLength,
    Device(u32),
}
impl FormatError {
    pub const fn status(self) -> u32 {
        match self {
            Self::InvalidGeometry => 1, Self::EmptyMedium => 2,
            Self::WriteProtected => 0x12, Self::Layout => 0x13,
            Self::Cancelled => 0x16, Self::Finish => 0x17,
            Self::Write => 0x19, Self::Finalize => 0x1a,
            Self::Scan => 0x1b, Self::Flush => 0x27,
            Self::InvalidExtendedLength => 0x110, Self::Device(n) => n,
        }
    }
}
enum CancellationFlag { Owned(AtomicU32), Mapped(&'static AtomicU32) }
pub struct FormatControl { requested: CancellationFlag }
impl Default for FormatControl { fn default() -> Self { Self::new() } }
impl FormatControl {
    pub const fn new() -> Self { Self { requested: CancellationFlag::Owned(AtomicU32::new(0)) } }
    /// # Safety
    /// X-M1's aligned format cancellation word is mapped and exclusively assigned to format cancellation.
    pub unsafe fn from_mapped_xm1() -> Self {
        Self { requested: CancellationFlag::Mapped(unsafe { &*(0x0096_cfe8 as *const AtomicU32) }) }
    }
    fn flag(&self) -> &AtomicU32 {
        match &self.requested { CancellationFlag::Owned(flag) => flag, CancellationFlag::Mapped(flag) => flag }
    }
    pub fn cancel(&self) { self.flag().store(1, Ordering::Release); }
    pub fn reset(&self) { self.flag().store(0, Ordering::Release); }
    pub fn checkpoint(&self) -> Result<(), FormatError> {
        if self.flag().load(Ordering::Acquire) == 0 { Ok(()) } else { Err(FormatError::Cancelled) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Formatter { Tables(u8), Small, Type6Fat32, Capacity(u8) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Layout {
    pub kind: FatKind,
    pub partition: Sector,
    pub volume_sectors: SectorCount,
    pub sector_bytes: u16,
    pub cluster_sectors: u16,
    pub reserved_sectors: u16,
    pub fat_sectors: SectorCount,
    pub fat_start: Sector,
    pub data_start: Sector,
    pub root: Cluster,
    pub root_entries: u16,
    pub clusters: u32,
    partition_type: u8,
    pub(crate) formatter: Formatter,
}

const CLUSTER_PAGES: [u16; 20] = [16,16,16,16,32,32,32,32,32,32,32,64,64,64,64,64,256,256,512,512];
const ALIGN_PAGES: [u32; 20] = [16,16,16,16,32,32,32,64,64,128,128,128,8192,8192,8192,8192,32768,32768,65536,65536];
const TABLE_TRACKS: [u8; 22] = [4,8,8,16,16,16,32,32,32,63,63,63,63,63,0,0,8,8,16,16,32,32];
const TABLE_CLUSTERS: [u8; 22] = [8,8,16,16,32,32,32,32,32,32,32,64,64,64,0,0,1,1,1,1,1,1];
const TABLE_TYPES: [u8; 22] = [1,1,1,1,1,1,1,6,6,6,6,6,6,6,0,0,1,0,2,0,2,0];
const TABLE_FATS: [u16; 22] = [1,2,2,3,3,6,12,32,63,125,250,250,250,250,771,771,1795,3847,3855,16672,16705,0];
const TABLE_END_HEAD: [u8; 22] = [3,3,3,3,3,7,7,15,15,15,32,65,65,65,0,0,124,0,124,0,249,0];
const TABLE_END_CYL: [u16; 22] = [124,124,249,249,499,499,499,499,999,1014,984,984,984,984,16,0,16,0,16,0,16,0];
const TABLE_START_TRACK: [u8; 22] = [1,3,3,9,9,3,23,15,17,53,59,60,60,60,0,0,16,16,16,16,16,16];
const TABLE_START_HEAD: [u8; 22] = [3,1,3,1,2,2,1,1,1,0,0,1,1,1,0,0,0,0,0,0,0,0];
pub(crate) const TABLE_ROOT_PAGES: [u8; 22] = [16,16,16,16,16,16,16,16,16,16,16,16,16,16,0,0,3,1,3,1,2,2];

fn cluster_pages(capacity_mib: u32) -> u16 {
    match capacity_mib { 0..=15 => 8, 16..=127 => 4, 128..=255 => 8, 256..=511 => 16, 512..=1023 => 32, _ => 64 }
}
fn ceil(n: u32, d: u32) -> u32 { n / d + u32::from(n % d != 0) }
fn adjusted_sectors(v: &VolumeDescriptor) -> u32 {
    v.sectors.0.wrapping_add(u32::from(v.geometry.sectors_per_track.get()).wrapping_mul(!u32::from(v.geometry.heads.get())))
}

impl Layout {
    pub fn select(v: &VolumeDescriptor) -> Result<Self, FormatError> {
        if v.sectors.0 == 0 { return Err(FormatError::EmptyMedium); }
        match v.format {
            FormatClass::Tables => Self::tables(v),
            FormatClass::Type6 => Self::small(v),
            FormatClass::Capacity => Self::capacity(v),
            FormatClass::Other(_) => {
                let pages = cluster_pages((v.sectors.0 & 0x7fffff) >> 11);
                if adjusted_sectors(v) / u32::from(pages) >= 0xfff6 { Self::type6_fat32(v) } else { Self::small(v) }
            }
        }
    }
    fn tables(v: &VolumeDescriptor) -> Result<Self, FormatError> {
        let mib = v.sectors.0 >> 11;
        let index = if mib < 2 { 0 } else { (32 - (mib - 1).leading_zeros()) as usize };
        let rounded = 1u32 << index;
        let pages = u32::from(TABLE_CLUSTERS[index]);
        if pages == 0 { return Err(FormatError::InvalidGeometry); }
        let partition = u32::from(TABLE_TRACKS[index]) * u32::from(TABLE_START_HEAD[index]) + u32::from(TABLE_START_TRACK[index]);
        let total = if rounded <= 128 { rounded * 2000 } else { v.sectors.0 / pages * pages };
        let fat = u32::from(TABLE_FATS[index]);
        let root_entries = 256;
        let fat_start = partition.wrapping_add(1) & 0xffff;
        let data = fat_start + fat * 2 + 16;
        Ok(Self { kind: if TABLE_TYPES[index] == 1 { FatKind::Fat12 } else { FatKind::Fat16 }, partition: Sector(partition), volume_sectors: SectorCount(total.wrapping_sub(partition)), sector_bytes: 512, cluster_sectors: pages as u16, reserved_sectors: 1, fat_sectors: SectorCount(fat), fat_start: Sector(fat_start), data_start: Sector(data), root: Cluster(fat_start + fat * 2), root_entries, clusters: v.sectors.0.wrapping_sub(data) / pages, partition_type: TABLE_TYPES[index], formatter: Formatter::Tables(index as u8) })
    }
    fn small(v: &VolumeDescriptor) -> Result<Self, FormatError> {
        let bytes = if v.format == FormatClass::Type6 { 2048u16 } else { 512 };
        let mib = v.sectors.0.wrapping_mul(u32::from(bytes)) >> 20;
        let pages = cluster_pages(mib);
        let bits = if mib < 16 { 12 } else { 16 };
        let total = adjusted_sectors(v);
        let entries = u32::from(bytes) * 8 / bits;
        let clusters = total / u32::from(pages);
        let mut fat = ceil(clusters, entries);
        let overhead_clusters = (ceil(0x4000, u32::from(bytes)) + 1 + u32::from(pages) + fat * 2) / u32::from(pages);
        if entries < entries.wrapping_mul(fat).wrapping_sub(clusters).wrapping_add(overhead_clusters) { fat = fat.wrapping_sub(1); }
        let mut heads = v.geometry.heads.get();
        let mut cyl = v.geometry.cylinders;
        while cyl > 1024 {
            heads = heads.wrapping_mul(2);
            if heads >= 256 { return Err(FormatError::InvalidGeometry); }
            cyl >>= 1;
        }
        let partition = u32::from(v.geometry.sectors_per_track.get());
        let fat_start = partition.wrapping_add(1) & 0xffff;
        let data = fat_start.wrapping_add(fat.wrapping_mul(2)).wrapping_add(0x4000 / u32::from(bytes));
        Ok(Self { kind: if bits == 12 { FatKind::Fat12 } else { FatKind::Fat16 }, partition: Sector(partition), volume_sectors: SectorCount(total), sector_bytes: bytes, cluster_sectors: pages, reserved_sectors: 1, fat_sectors: SectorCount(fat), fat_start: Sector(fat_start), data_start: Sector(data), root: Cluster(fat_start.wrapping_add(fat.wrapping_mul(2))), root_entries: 512, clusters: partition.wrapping_sub(data).wrapping_add(total) / u32::from(pages), partition_type: if bits == 12 { 1 } else if mib < 32 { 4 } else { 6 }, formatter: Formatter::Small })
    }
    fn type6_fat32(v: &VolumeDescriptor) -> Result<Self, FormatError> {
        let total = adjusted_sectors(v);
        let clusters = total >> 6;
        let mut fat = (clusters + 127) >> 7;
        if (fat * 128 - clusters) + ((fat * 2 + 0x61) >> 6) > 128 { fat -= 1; }
        let partition = u32::from(v.geometry.sectors_per_track.get());
        let fat_start = partition.wrapping_add(32) & 0xffff;
        let data = fat_start.wrapping_add(fat * 2);
        let mut heads = u32::from(v.geometry.heads.get());
        let mut cyl = u32::from(v.geometry.cylinders);
        while cyl > 1024 && heads.wrapping_sub(1) as i32 <= 255 { heads = (heads * 2) & 0xffff; cyl >>= 1; }
        Ok(Self { kind: FatKind::Fat32, partition: Sector(partition), volume_sectors: SectorCount(total), sector_bytes: 512, cluster_sectors: 64, reserved_sectors: 32, fat_sectors: SectorCount(fat), fat_start: Sector(fat_start), data_start: Sector(data), root: Cluster(2), root_entries: 0, clusters: partition.wrapping_sub(data).wrapping_add(total) / 64, partition_type: if heads.wrapping_sub(1) as i32 <= 255 { 11 } else { 12 }, formatter: Formatter::Type6Fat32 })
    }
    fn capacity(v: &VolumeDescriptor) -> Result<Self, FormatError> {
        let mut mode = 0usize;
        while mode < 19 && v.sectors.0 > (2048u32 << mode) { mode += 1; }
        if (12..16).contains(&mode) { mode = 12; }
        let pages = CLUSTER_PAGES[mode];
        if mode > 15 {
            let partition = if mode < 18 { 32768u32 } else { 65536 };
            let descriptor_partition = partition & 0xffff;
            let data = descriptor_partition + partition;
            return Ok(Self { kind: FatKind::ExFat, partition: Sector(partition), volume_sectors: SectorCount(v.sectors.0.wrapping_sub(partition)), sector_bytes: 512, cluster_sectors: pages, reserved_sectors: 0, fat_sectors: SectorCount(partition >> 1), fat_start: Sector(descriptor_partition + (partition >> 1)), data_start: Sector(data), root: Cluster(4), root_entries: 0, clusters: v.sectors.0.wrapping_sub(data) / u32::from(pages), partition_type: 7, formatter: Formatter::Capacity(mode as u8) });
        }
        let kind = if mode >= 12 { FatKind::Fat32 } else if v.sectors.0 / u32::from(pages) >= 0xff5 { FatKind::Fat16 } else { FatKind::Fat12 };
        let (partition,reserved,fat) = solve_capacity(v.sectors.0, u32::from(pages), ALIGN_PAGES[mode], kind)?;
        let fat_start = if kind == FatKind::Fat32 { (partition & 0xffff) + u32::from(reserved) } else { (partition + u32::from(reserved)) & 0xffff };
        let data = fat_start + fat * 2 + if kind == FatKind::Fat32 { 0 } else { 32 };
        Ok(Self { kind, partition: Sector(partition), volume_sectors: SectorCount(v.sectors.0.wrapping_sub(partition)), sector_bytes: 512, cluster_sectors: pages, reserved_sectors: reserved, fat_sectors: SectorCount(fat), fat_start: Sector(fat_start), data_start: Sector(data), root: Cluster(if kind == FatKind::Fat32 { 2 } else { fat_start + fat * 2 }), root_entries: if kind == FatKind::Fat32 { 0 } else { 512 }, clusters: (partition & 0xffff).wrapping_sub(data).wrapping_add(v.sectors.0.wrapping_sub(partition)) / u32::from(pages), partition_type: if kind == FatKind::Fat32 { if v.sectors.0 < 0xfb0400 { 11 } else { 12 } } else if v.sectors.0 < 0x7fa8 { 1 } else if v.sectors.0 < 0x10000 { 4 } else { 6 }, formatter: Formatter::Capacity(mode as u8) })
    }
}

fn solve_capacity(total: u32, pages: u32, alignment: u32, kind: FatKind) -> Result<(u32,u16,u32),FormatError> {
    let bits = match kind { FatKind::Fat12 => 12, FatKind::Fat16 => 16, _ => 32 };
    let initial = ceil((total / pages).wrapping_mul(bits), 4096);
    let mut fat = initial;
    for iteration in 0..256u32 {
        if kind == FatKind::Fat32 { fat = initial.wrapping_sub(iteration); }
        let overhead = fat.wrapping_mul(2).wrapping_add(if kind == FatKind::Fat32 { 0 } else { 33 });
        let mut aligned = alignment;
        let mut increments = 1;
        while aligned < overhead {
            increments += 1;
            if increments == 256 { return Err(FormatError::Layout); }
            aligned = aligned.wrapping_add(alignment);
        }
        let mut extra = aligned.wrapping_sub(overhead);
        if if kind == FatKind::Fat32 { extra < 9 } else { extra != alignment } { extra = extra.wrapping_add(alignment); }
        let mut remaining = total.wrapping_sub(if kind == FatKind::Fat32 { alignment } else { 0 }).wrapping_sub(overhead).wrapping_sub(extra);
        let mut needed = 0;
        for attempt in 0..256 {
            needed = ceil((remaining / pages).wrapping_add(2).wrapping_mul(bits),4096);
            if needed <= fat { break; }
            if attempt == 255 { return Err(FormatError::Layout); }
            extra = extra.wrapping_add(alignment);
            remaining = remaining.wrapping_sub(alignment);
        }
        if needed == fat {
            return if kind == FatKind::Fat32 { Ok((alignment,extra as u16,fat)) } else { Ok((extra,1,fat)) };
        }
        if kind != FatKind::Fat32 { fat = needed; }
    }
    Err(FormatError::Layout)
}

fn put16(out: &mut [u8], at: usize, n: u16) { out[at..at+2].copy_from_slice(&n.to_le_bytes()); }
fn put32(out: &mut [u8], at: usize, n: u32) { out[at..at+4].copy_from_slice(&n.to_le_bytes()); }
fn put64(out: &mut [u8], at: usize, n: u64) { out[at..at+8].copy_from_slice(&n.to_le_bytes()); }
fn signature(out: &mut [u8]) { out[510] = 0x55; out[511] = 0xaa; }
fn chs(sector: u32, geometry: Geometry, saturated: bool) -> [u8;3] {
    if saturated { return [254,255,255]; }
    let tracks = u32::from(geometry.sectors_per_track.get());
    let cylinder = sector / (tracks * u32::from(geometry.heads.get()));
    [(sector % (tracks * u32::from(geometry.heads.get())) / tracks) as u8, (((cylinder >> 2) & 0xc0) | (sector % tracks + 1)) as u8, cylinder as u8]
}
impl Layout {
    pub fn partition_sector(&self, v: &VolumeDescriptor, out: &mut [u8;2048]) {
        out[..usize::from(self.sector_bytes)].fill(0);
        out[446] = 0x80;
        out[450] = self.partition_type;
        match self.formatter {
            Formatter::Tables(i) => {
                let i = usize::from(i);
                out[447] = TABLE_START_HEAD[i]; out[448] = TABLE_START_TRACK[i].wrapping_add(1);
                out[451] = TABLE_END_HEAD[i];
                out[452] = TABLE_TRACKS[i] | ((TABLE_END_CYL[i] & 0xf00) >> 2) as u8;
                out[453] = TABLE_END_CYL[i] as u8;
            }
            Formatter::Small | Formatter::Type6Fat32 => {
                let g = v.geometry;
                let tracks = u32::from(g.sectors_per_track.get());
                let start = tracks + 1;
                out[447] = ((start % (tracks * u32::from(g.heads.get()))) / tracks) as u8;
                out[448] = (start % tracks) as u8;
                out[449] = (start / (tracks * u32::from(g.heads.get()))) as u8;
                let mut heads = u32::from(g.heads.get()); let mut cyl = u32::from(g.cylinders);
                while cyl > 1024 { heads = (heads * 2) & 0xffff; cyl >>= 1; if heads >= 256 { break; } }
                if self.partition_type == 12 { out[451..454].copy_from_slice(&[254,255,255]); }
                else { out[451] = heads.wrapping_sub(1) as u8; out[452] = (((cyl.wrapping_sub(1) >> 2) & 0xc0) | tracks) as u8; out[453] = cyl.wrapping_sub(1) as u8; }
            }
            Formatter::Capacity(_) => {
                out[447..450].copy_from_slice(&chs(self.partition.0,v.geometry,self.kind != FatKind::Fat12 && self.kind != FatKind::Fat16 && self.partition.0 > 0xfb0400));
                out[451..454].copy_from_slice(&chs(v.sectors.0.wrapping_sub(1),v.geometry,self.kind != FatKind::Fat12 && self.kind != FatKind::Fat16 && v.sectors.0 > 0xfb0400));
            }
        }
        put32(out,454,self.partition.0); put32(out,458,self.volume_sectors.0); signature(out);
    }
    pub fn boot_sector(&self, v: &VolumeDescriptor, out: &mut [u8;2048]) {
        out[..usize::from(self.sector_bytes)].fill(0);
        if self.kind == FatKind::ExFat {
            out[..3].copy_from_slice(&[0xeb,0x76,0x90]); out[3..11].copy_from_slice(b"EXFAT   ");
            put64(out,64,u64::from(self.partition.0)); put64(out,72,u64::from(self.volume_sectors.0));
            put32(out,80,self.partition.0 >> 1); put32(out,84,self.fat_sectors.0); put32(out,88,self.partition.0);
            put32(out,92,v.sectors.0.wrapping_sub(self.partition.0 * 2) / u32::from(self.cluster_sectors)); put32(out,96,4);
            out[105] = 1; out[108] = 9; out[109] = self.cluster_sectors.trailing_zeros() as u8;
            out[110] = 1; out[111] = 0x80; out[112] = 0xff; out[120..510].fill(0xf4); signature(out); return;
        }
        out[0] = if matches!(self.formatter,Formatter::Tables(_) | Formatter::Small) { 0xe9 } else { 0xeb };
        if out[0] == 0xeb { out[2] = 0x90; }
        if self.formatter == Formatter::Type6Fat32 { out[1] = 0x5a; }
        out[3..11].copy_from_slice(b"FF-DSC01");
        put16(out,11,self.sector_bytes); out[13] = self.cluster_sectors as u8;
        put16(out,14,self.reserved_sectors); out[16] = 2; put16(out,17,self.root_entries); out[21] = 0xf8;
        put16(out,24,v.geometry.sectors_per_track.get()); put16(out,26,v.geometry.heads.get()); put32(out,28,self.partition.0);
        let short_total = match self.formatter { Formatter::Tables(i) => i < 6, _ => self.kind != FatKind::Fat32 && self.volume_sectors.0 < 0x10000 };
        if short_total { put16(out,19,self.volume_sectors.0 as u16); } else { put32(out,32,self.volume_sectors.0); }
        if self.kind == FatKind::Fat32 {
            put32(out,36,self.fat_sectors.0); put32(out,44,2); put16(out,48,1); put16(out,50,6);
            out[64] = 0x80; out[66] = 0x29; out[82..90].copy_from_slice(b"FAT32   ");
            if self.formatter == Formatter::Type6Fat32 { out[71..82].copy_from_slice(b"FinePix    "); }
        } else {
            put16(out,22,self.fat_sectors.0 as u16);
            out[54..62].copy_from_slice(if self.kind == FatKind::Fat12 { b"FAT12   " } else { b"FAT16   " });
            if !matches!(self.formatter,Formatter::Small) { out[38] = 0x29; }
            if matches!(self.formatter,Formatter::Capacity(_)) { out[36] = 0x80; }
            if let Formatter::Tables(i) = self.formatter { out[24] = TABLE_TRACKS[usize::from(i)]; out[26] = TABLE_END_HEAD[usize::from(i)].wrapping_add(1); }
        }
        signature(out);
    }
}
