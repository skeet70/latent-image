//! Allocation group scan at X-M1 1.50 0x007012c0, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AllocationEncoding { Packed12, Words16 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SectorsPerCluster(u16);
impl SectorsPerCluster {
    pub const fn parse(value: u16) -> Option<Self> { if value != 0 { Some(Self(value)) } else { None } }
    pub const fn get(self) -> u16 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AllocationGeometry {
    pub sectors_per_cluster: SectorsPerCluster,
    pub data_page: u32,
    pub data_clusters: u32,
    pub encoding: AllocationEncoding,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AllocationGroups {
    pub skipped_clusters: u32,
    pub clusters_per_group: u8,
    pub groups: u32,
    pub free_groups: u32,
    pub free_clusters: u32,
    pub first_free_group: Option<u32>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AllocationError { TruncatedMetadata, InvalidGeometry }

pub fn scan_allocation_groups(geometry: AllocationGeometry, metadata: &[u8]) -> Result<AllocationGroups, AllocationError> {
    let cluster_sectors = u32::from(geometry.sectors_per_cluster.get());
    let width = if cluster_sectors == 32 { 16u8 } else { 8u8 };
    let skipped = if geometry.data_page < 1024 {
        cluster_sectors.wrapping_sub(geometry.data_page).wrapping_add(1023) / cluster_sectors
    } else if geometry.data_page & 511 != 0 {
        ((geometry.data_page & 511) + cluster_sectors - 1) / cluster_sectors
    } else { 0 };
    let available = geometry.data_clusters.checked_sub(skipped).ok_or(AllocationError::InvalidGeometry)?;
    let groups = available / u32::from(width);
    let entries = groups.checked_mul(u32::from(width)).and_then(|count| count.checked_add(skipped)).and_then(|count| count.checked_add(2)).ok_or(AllocationError::InvalidGeometry)?;
    let required = match geometry.encoding {
        AllocationEncoding::Packed12 => u64::from(entries).checked_mul(3).map(|bytes| (bytes + 1) / 2),
        AllocationEncoding::Words16 => u64::from(entries).checked_mul(2),
    }.ok_or(AllocationError::InvalidGeometry)?;
    if groups != 0 && required > metadata.len() as u64 { return Err(AllocationError::TruncatedMetadata); }
    let mut result = AllocationGroups { skipped_clusters: skipped, clusters_per_group: width, groups, free_groups: 0, free_clusters: 0, first_free_group: None };
    for group in 0..groups {
        let start = skipped + 2 + group * u32::from(width);
        let free = (start..start+u32::from(width)).all(|cluster| {
            match geometry.encoding {
                AllocationEncoding::Packed12 => {
                    let index = (u64::from(cluster) * 3 / 2) as usize;
                    if cluster & 1 == 0 { metadata[index] == 0 && metadata[index+1] & 15 == 0 }
                    else { metadata[index] & 0xf0 == 0 && metadata[index+1] == 0 }
                }
                AllocationEncoding::Words16 => {
                    let index = cluster as usize * 2;
                    metadata[index] == 0 && metadata[index+1] == 0
                }
            }
        });
        if free {
            result.free_groups += 1;
            if result.first_free_group.is_none() { result.first_free_group = Some(group); }
        }
    }
    result.free_clusters = u32::from(width) * result.free_groups;
    Ok(result)
}

pub const fn capacity_bytes(groups: u32) -> u32 { if groups < 0x4000 { groups << 18 } else { u32::MAX } }

#[cfg(test)]
mod tests {
    use super::{scan_allocation_groups, capacity_bytes, AllocationEncoding, AllocationError, AllocationGeometry, SectorsPerCluster};
    fn geometry(encoding: AllocationEncoding) -> AllocationGeometry {
        AllocationGeometry { sectors_per_cluster: SectorsPerCluster::parse(64).unwrap(), data_page: 1024, data_clusters: 16, encoding }
    }
    #[test]
    fn counts_only_wholly_free_groups() {
        let mut metadata = [0u8; 36];
        metadata[4] = 0xff;
        let result = scan_allocation_groups(geometry(AllocationEncoding::Words16),&metadata).unwrap();
        assert_eq!(result.free_groups,1);
        assert_eq!(result.free_clusters,8);
        assert_eq!(result.first_free_group,Some(1));
    }
    #[test]
    fn packed_nibbles_do_not_leak_between_groups() {
        let mut metadata = [0u8; 27];
        metadata[3] = 1;
        let result = scan_allocation_groups(geometry(AllocationEncoding::Packed12),&metadata).unwrap();
        assert_eq!(result.free_groups,1);
        assert_eq!(result.first_free_group,Some(1));
    }
    #[test]
    fn rejects_short_metadata_without_partial_count() {
        assert_eq!(scan_allocation_groups(geometry(AllocationEncoding::Words16),&[0;35]),Err(AllocationError::TruncatedMetadata));
    }
    #[test]
    fn saturates_capacity_at_sixteen_thousand_groups() {
        assert_eq!(capacity_bytes(0x3fff),0xfffc0000);
        assert_eq!(capacity_bytes(0x4000),u32::MAX);
    }
}
