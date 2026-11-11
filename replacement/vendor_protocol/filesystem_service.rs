//! X-M1 1.50 filesystem service snapshot and status translation.
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, 0x007101e4 and 0x0071d450.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotStatus { Ready, Unavailable }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SectorCount(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClusterBytes(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FreeBytes(pub u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MediaWriteState(u8);
impl MediaWriteState {
    pub const fn from_device(value: u8) -> Self { Self(value) }
    pub const fn bits(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VolumeFormat { Unformatted, Fat12, Fat16, Fat32, Type6, Type7, Type8, Other(u8) }
impl VolumeFormat {
    pub const fn from_device(value: u8) -> Self {
        match value { 0 => Self::Unformatted, 3 => Self::Fat12, 4 => Self::Fat16, 5 => Self::Fat32, 6 => Self::Type6, 7 => Self::Type7, 8 => Self::Type8, other => Self::Other(other) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VolumeIdentifier(pub [u8; 16]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Geometry { pub cylinders: u16, pub heads: u16, pub sectors_per_track: u16, pub sector_bytes: u16 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceSummary {
    pub sectors: SectorCount,
    pub write_state: MediaWriteState,
    pub format: VolumeFormat,
    pub metadata_status: bool,
    pub rounded_megabytes: u32,
    pub free_bytes: FreeBytes,
    pub free_clusters: u32,
    pub cluster_bytes: ClusterBytes,
    pub geometry: Geometry,
    pub identifier: VolumeIdentifier,
    pub metadata_present: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NumberPair { pub directory: u16, pub file: u16 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NumberingCounters {
    pub total: u32,
    pub current: NumberPair,
    pub first: u32,
    pub second: u32,
    pub third: u32,
    pub fourth: u32,
    pub fifth: u32,
    pub sixth: u32,
    pub seventh: u32,
    pub eighth: u32,
    pub ninth: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogSummary {
    pub present: bool,
    pub secondary_present: bool,
    pub counters: NumberingCounters,
    pub last: NumberPair,
    pub remaining: u32,
    pub selected: u32,
    pub auxiliary_count: u32,
    pub directory_count: u16,
    pub sequence: u16,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DimensionCount(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DimensionMode { pub count: DimensionCount, pub value: u32 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotMode { Filesystem, Dimension(DimensionMode) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumberingMode { Filesystem, RegionZero, RegionOne, RegionTwo }
impl NumberingMode {
    pub fn parse(value: u32) -> Result<Self, ServiceStatus> {
        match value { 0 => Ok(Self::Filesystem), 1 => Ok(Self::RegionZero), 2 => Ok(Self::RegionOne), 3 => Ok(Self::RegionTwo), _ => Err(ServiceStatus(8)) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryError { InvalidSelector, ShortOutput { required: usize }, Uninitialized, CountPending }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotSelector(u8);
impl SnapshotSelector {
    pub fn parse(value: u32) -> Result<Self, QueryError> {
        if value <= 0x28 { Ok(Self(value as u8)) } else { Err(QueryError::InvalidSelector) }
    }
}

pub struct FilesystemService {
    initialized: bool,
    mode: SnapshotMode,
    record: [u8; 0x90],
    valid: [bool; 4],
    metadata_present: bool,
    use_current_pair: bool,
    current_pair: Option<NumberPair>,
    selected_count: u32,
    directory_count: u16,
    count_result: Option<Result<u32, ()>>,
}
impl Default for FilesystemService { fn default() -> Self { Self::new() } }
impl FilesystemService {
    pub const fn new() -> Self {
        Self { initialized: false, mode: SnapshotMode::Filesystem, record: [0; 0x90], valid: [false; 4], metadata_present: false, use_current_pair: false, current_pair: None, selected_count: 0, directory_count: 0, count_result: None }
    }
    pub fn initialize(&mut self) { self.initialized = true; }
    pub fn set_numbering_mode(&mut self, mode: NumberingMode) {
        self.mode = match mode {
            NumberingMode::Filesystem => SnapshotMode::Filesystem,
            NumberingMode::RegionZero => SnapshotMode::Dimension(DimensionMode { count: DimensionCount(8), value: 0 }),
            NumberingMode::RegionOne | NumberingMode::RegionTwo => SnapshotMode::Dimension(DimensionMode { count: DimensionCount(15), value: 0 }),
        };
    }
    pub fn set_numbering_offset(&mut self, offset: u32) {
        if let SnapshotMode::Dimension(mode) = &mut self.mode { mode.value = offset; }
    }
    pub fn invalidate_device_summary(&mut self) { self.valid[0] = false; self.valid[3] = false; }
    pub fn invalidate_catalog(&mut self) { self.valid[1] = false; self.valid[2] = false; }
    pub fn refresh_device_summary(&mut self, summary: &DeviceSummary) {
        let record = &mut self.record;
        put32(record, 0, u32::from(summary.write_state.bits() & 0x1f));
        let format = match summary.format {
            VolumeFormat::Unformatted => Some(0), VolumeFormat::Fat12 => Some(4), VolumeFormat::Fat16 => Some(5), VolumeFormat::Fat32 => Some(6), VolumeFormat::Type6 => Some(7), VolumeFormat::Type7 => Some(8), VolumeFormat::Type8 => Some(9), VolumeFormat::Other(_) => None,
        };
        if let Some(format) = format { put32(record, 4, format); }
        put32(record, 8, u32::from(summary.metadata_status));
        let size = summary.rounded_megabytes;
        let size_mode = if size.is_power_of_two() && size <= 0x80000 { size.trailing_zeros() } else { 0x13 };
        put32(record, 0x0c, size_mode);
        put32(record, 0x10, summary.free_bytes.0 as u32);
        put32(record, 0x14, (summary.free_bytes.0 >> 32) as u32);
        put32(record, 0x18, summary.free_clusters);
        put32(record, 0x1c, if summary.free_bytes.0 >> 32 != 0 { u32::MAX } else { summary.free_bytes.0 as u32 });
        put32(record, 0x20, summary.cluster_bytes.0);
        record[0x28..0x38].copy_from_slice(&summary.identifier.0);
        record[0x38] = u8::from(summary.metadata_present);
        put16(record, 0x70, summary.geometry.cylinders);
        put16(record, 0x72, summary.geometry.heads);
        put16(record, 0x74, summary.geometry.sectors_per_track);
        put16(record, 0x76, summary.geometry.sector_bytes);
        put32(record, 0x78, summary.sectors.0);
        self.metadata_present = summary.metadata_present;
        self.valid[0] = true;
        self.valid[3] = true;
    }
    pub fn refresh_catalog(&mut self, summary: &CatalogSummary) {
        self.record[0x3c] = u8::from(summary.present);
        self.record[0x3d] = u8::from(summary.secondary_present);
        let counters = summary.counters;
        put32(&mut self.record, 0x40, counters.total);
        put16(&mut self.record, 0x44, counters.current.directory);
        put16(&mut self.record, 0x46, counters.current.file);
        for (offset, value) in [(0x48,counters.first),(0x4c,counters.second),(0x50,counters.third),(0x54,counters.fourth),(0x58,counters.fifth),(0x5c,counters.sixth),(0x60,counters.seventh),(0x64,counters.eighth),(0x68,counters.ninth)] { put32(&mut self.record, offset, value); }
        put16(&mut self.record, 0x7c, summary.last.directory);
        put16(&mut self.record, 0x7e, summary.last.file);
        put32(&mut self.record, 0x80, summary.remaining);
        put32(&mut self.record, 0x84, summary.selected);
        put32(&mut self.record, 0x88, summary.auxiliary_count);
        put16(&mut self.record, 0x8c, summary.sequence);
        self.directory_count = summary.directory_count;
        self.valid[1] = true;
        self.valid[2] = true;
    }
    pub fn set_current_pair(&mut self, enabled: bool, pair: Option<NumberPair>) { self.use_current_pair = enabled; self.current_pair = pair; }
    pub fn refresh_selected_count(&mut self, groups: &[DirectoryGroup], mask: GroupMask) {
        self.selected_count = groups.iter().filter(|group| mask.contains(group.group)).fold(0u32, |sum, group| sum.wrapping_add(u32::from(group.count)));
    }
    pub fn refresh_cached_capacity(&mut self) {
        self.count_result = Some(Ok(if get32(&self.record,0x14) != 0 { u32::MAX } else { get32(&self.record,0x10) }));
    }
    pub(crate) fn allocation_capacity_unavailable(&mut self) { self.count_result = Some(Err(())); }
    pub fn refresh_allocation_capacity(&mut self, geometry: crate::filesystem_capacity::AllocationGeometry, metadata: &[u8]) -> Result<(), crate::filesystem_capacity::AllocationError> {
        match crate::filesystem_capacity::scan_allocation_groups(geometry,metadata) {
            Ok(groups) => { self.count_result = Some(Ok(crate::filesystem_capacity::capacity_bytes(groups.free_groups))); Ok(()) }
            Err(error) => { self.count_result = Some(Err(())); Err(error) }
        }
    }
    pub fn media_status(&self, output: &mut u32) -> SnapshotStatus {
        if !self.initialized { return SnapshotStatus::Unavailable; }
        *output = if matches!(self.mode, SnapshotMode::Dimension(_)) { 0 } else { get32(&self.record, 0) };
        if self.valid[0] || matches!(self.mode, SnapshotMode::Dimension(_)) { SnapshotStatus::Ready } else { SnapshotStatus::Unavailable }
    }
    pub fn query(&self, selector: SnapshotSelector, output: &mut [u8]) -> Result<SnapshotStatus, QueryError> {
        if !self.initialized { return Err(QueryError::Uninitialized); }
        let kind = selector.0;
        let (offset, length) = match kind {
            0 => (0,0x3c), 1..=6 => ((kind as usize-1)*4,4), 7 => (0x1c,4), 8 => (0x18,4), 9 => (0x20,4), 10 => (0x26,2), 11 => (0x28,16), 12 => (0x3c,4), 13 => (0x3c,1), 14 => (0x3d,1), 15 => (0x40,0x30), 16 => (0x40,4), 17 => (0x44,2), 18 => (0x46,2), 19..=25 => (0x48+(kind as usize-19)*4,4), 26 => (0x7c,2), 27 => (0x7e,2), 28 => (0x80,4), 29 => (0x84,4), 30 => (0x70,12), 31..=34 => (0x70+(kind as usize-31)*2,2), 35 => (0x78,4), 36..=38 => (0,4), 39 => (0x8c,2), 40 => (0,0x90), _ => return Err(QueryError::InvalidSelector),
        };
        if output.len() < length { return Err(QueryError::ShortOutput { required: length }); }
        let output = &mut output[..length];
        if let SnapshotMode::Dimension(mode) = self.mode {
            if matches!(kind, 1..=9 | 13 | 15..=23 | 26..=35 | 37..=39) {
                self.dimension_query(kind,mode,output);
                return Ok(SnapshotStatus::Ready);
            }
        }
        if kind == 36 {
            match self.count_result {
                None => return Err(QueryError::CountPending),
                Some(Err(())) => return Ok(SnapshotStatus::Unavailable),
                Some(Ok(count)) => put32(output,0,count),
            }
        } else {
        output.copy_from_slice(&self.record[offset..offset+length]);
        }
        match kind {
            2 if get32(output,0) == 0 && self.metadata_present => put32(output,0,1),
            15 => { if self.use_current_pair { self.override_pair(output,4); } put32(output,0x24,get32(&self.record,0x88)); }
            17 => { if let Some(pair) = self.current_pair { put16(output,0,pair.directory); } }
            18 => { if let Some(pair) = self.current_pair { put16(output,0,pair.file); } }
            36 => (),
            37 => put32(output,0,self.selected_count),
            38 => put32(output,0,u32::from(self.directory_count)),
            40 if self.use_current_pair => self.override_pair(output,0x44),
            _ => (),
        }
        if let SnapshotMode::Dimension(mode) = self.mode { self.dimension_query(kind, mode, output); return Ok(SnapshotStatus::Ready); }
        let valid = match kind { 0..=11 => self.valid[0], 12..=14 => self.valid[1], 15..=22 | 26..=29 => self.valid[2], 30..=35 => self.valid[3], _ => self.valid.iter().all(|flag| *flag) };
        Ok(if valid { SnapshotStatus::Ready } else { SnapshotStatus::Unavailable })
    }
    fn override_pair(&self, output: &mut [u8], offset: usize) {
        if let Some(pair) = self.current_pair { put16(output,offset,pair.directory); put16(output,offset+2,pair.file); }
    }
    fn dimension_query(&self, kind: u8, mode: DimensionMode, output: &mut [u8]) {
        let count = mode.count.0;
        match kind {
            0 | 40 => {
                for offset in (0..0x20).step_by(4) { put32(output,offset,0); }
                put32(output,8,1); put32(output,0x20,0x4000);
                if kind == 40 {
                    output[0x3c] = 1;
                    dimension_counters(&mut output[0x40..],mode,false);
                    put16(output,0x7c,100); put16(output,0x7e,count as u16);
                    put32(output,0x80,count); put32(output,0x84,mode.value);
                    dimension_geometry(&mut output[0x70..]);
                }
            }
            1 | 2 | 5..=8 | 37 | 38 => put32(output,0,0),
            3 => put32(output,0,1), 4 => put32(output,0,6), 9 => put32(output,0,0x4000),
            12 | 13 => output[0] = 1,
            15 => dimension_counters(output,mode,true),
            16 | 19 | 21 | 28 => put32(output,0,count),
            17 | 26 => put16(output,0,100), 18 | 27 => put16(output,0,count as u16),
            20 | 22 | 23 | 29 => put32(output,0,mode.value),
            30 => dimension_geometry(output),
            31 => put16(output,0,500), 32 => put16(output,0,8), 33 => put16(output,0,32), 34 => put16(output,0,512), 35 => put32(output,0,128000), 39 => put16(output,0,0), _ => (),
        }
    }
}
fn dimension_counters(output: &mut [u8], mode: DimensionMode, clear_auxiliary: bool) {
    put32(output,0,mode.count.0); put16(output,4,100); put16(output,6,mode.count.0 as u16);
    for offset in [8,16] { put32(output,offset,mode.count.0); }
    for offset in [12,20,24] { put32(output,offset,mode.value); }
    if clear_auxiliary { put32(output,36,0); }
}
fn dimension_geometry(output: &mut [u8]) { put16(output,0,500); put16(output,2,8); put16(output,4,32); put16(output,6,512); put32(output,8,128000); }
fn put16(output: &mut [u8], offset: usize, value: u16) { output[offset..offset+2].copy_from_slice(&value.to_le_bytes()); }
fn put32(output: &mut [u8], offset: usize, value: u32) { output[offset..offset+4].copy_from_slice(&value.to_le_bytes()); }
fn get32(input: &[u8], offset: usize) -> u32 { u32::from_le_bytes([input[offset],input[offset+1],input[offset+2],input[offset+3]]) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GroupId(u8);
impl GroupId { pub const fn parse(value: u8) -> Option<Self> { if value < 32 { Some(Self(value)) } else { None } } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GroupMask(pub u32);
impl GroupMask { fn contains(self, group: Option<GroupId>) -> bool { group.is_some_and(|id| self.0 & (1u32 << id.0) != 0) } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectoryGroup { pub group: Option<GroupId>, pub count: u16 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilesystemStatus(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceStatus(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiagnosticCode(pub u8);
pub fn translate_status(status: FilesystemStatus) -> (ServiceStatus, Option<DiagnosticCode>) {
    let (status, diagnostic) = match status.0 {
        0 => (0,None), 1 => (1,Some(0x50)), 3 => (8,Some(0x61)), 4 => (8,Some(0x62)), 7 | 8 => (6,None), 9 | 10 => (5,Some(0x57)), 13 => (14,Some(100)), 14 => (3,Some(0x60)), 17 => (7,Some(0x66)), 18 => (4,Some(0x56)), 19 => (8,None), 20 => (9,Some(0x67)), 21 => (10,None), 22 => (11,None), 23 | 26 => (5,Some(0x58)), 32 => (3,Some(0x5f)), 33 => (13,Some(0x54)), 34 => (12,Some(0x52)), 35 | 37 => (8,None), 36 => (8,Some(0x65)), 38 => (5,Some(0x5b)), _ => (5,None),
    };
    (ServiceStatus(status), diagnostic.map(DiagnosticCode))
}

#[cfg(test)]
mod tests {
    use super::{FilesystemService, NumberingMode, QueryError, SnapshotSelector, SnapshotStatus, FilesystemStatus, ServiceStatus, DiagnosticCode, translate_status};
    #[test]
    fn uninitialized_snapshot_preserves_caller_output() {
        let service = FilesystemService::new();
        let mut output = 0x12345678;
        assert_eq!(service.media_status(&mut output),SnapshotStatus::Unavailable);
        assert_eq!(output,0x12345678);
    }
    #[test]
    fn invalidated_snapshot_writes_value_before_returning_unavailable() {
        let mut service = FilesystemService::new();
        service.initialize();
        let mut output = u32::MAX;
        assert_eq!(service.media_status(&mut output),SnapshotStatus::Unavailable);
        assert_eq!(output,0);
    }
    #[test]
    fn dimension_mode_uses_actual_region_count_and_preserves_unwritten_fields() {
        let mut service = FilesystemService::new();
        service.initialize();
        service.set_numbering_mode(NumberingMode::RegionOne);
        let mut output = [0xa5; 48];
        assert_eq!(service.query(SnapshotSelector::parse(15).unwrap(),&mut output),Ok(SnapshotStatus::Ready));
        assert_eq!(&output[..8],&[15,0,0,0,100,0,15,0]);
        assert_eq!(&output[28..36],&[0xa5;8]);
        assert_eq!(&output[36..40],&[0;4]);
    }
    #[test]
    fn rejects_short_output_without_mutating_it() {
        let mut service = FilesystemService::new();
        service.initialize();
        let mut output = [0x7e;3];
        assert_eq!(service.query(SnapshotSelector::parse(1).unwrap(),&mut output),Err(QueryError::ShortOutput { required:4 }));
        assert_eq!(output,[0x7e;3]);
    }
    #[test]
    fn filesystem_errors_keep_service_status_and_diagnostic_separate() {
        assert_eq!(translate_status(FilesystemStatus(18)),(ServiceStatus(4),Some(DiagnosticCode(0x56))));
        assert_eq!(translate_status(FilesystemStatus(22)),(ServiceStatus(11),None));
        assert_eq!(translate_status(FilesystemStatus(0x102)),(ServiceStatus(5),None));
    }
}
