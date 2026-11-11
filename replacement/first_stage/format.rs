pub const UNIT_BYTES: u32 = 0x20000;
pub const TABLE_ADDRESS: u32 = 0x0061_c000;
pub const TABLE_BYTES: usize = 0x400;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageId {
    PrimaryLoader,
    CompanionLoader,
    Application,
}

impl ImageId {
    fn word(self) -> u32 {
        match self {
            Self::PrimaryLoader => 0,
            Self::CompanionLoader => 1,
            Self::Application => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataSection {
    LensDefaults,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalMediaOffset(u32);

impl LogicalMediaOffset {
    pub fn bytes(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SectionMetadata {
    pub source_offset: LogicalMediaOffset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormatError {
    Truncated,
    InvalidTable,
    Missing(ImageId),
    MissingMetadata(MetadataSection),
    InvalidHeader,
    AddressRange,
}

#[derive(Clone, Copy)]
enum SectionId {
    Image(ImageId),
    Metadata(MetadataSection),
}

impl SectionId {
    fn word(self) -> u32 {
        match self {
            Self::Image(id) => id.word(),
            Self::Metadata(MetadataSection::LensDefaults) => 7,
        }
    }

    fn missing(self) -> FormatError {
        match self {
            Self::Image(id) => FormatError::Missing(id),
            Self::Metadata(section) => FormatError::MissingMetadata(section),
        }
    }
}

pub fn word(bytes: &[u8], offset: usize) -> Result<u32, FormatError> {
    let end = offset.checked_add(4).ok_or(FormatError::Truncated)?;
    let value = bytes.get(offset..end).ok_or(FormatError::Truncated)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn table_offset(pointer: u32, length: usize) -> Result<usize, FormatError> {
    let offset = (pointer & !0x4000_0000).checked_sub(TABLE_ADDRESS).ok_or(FormatError::InvalidTable)? as usize;
    if pointer & 3 != 0 || offset.checked_add(length).ok_or(FormatError::InvalidTable)? > TABLE_BYTES {
        return Err(FormatError::InvalidTable);
    }
    Ok(offset)
}

#[derive(Clone, Copy, Debug)]
pub struct Segment {
    pub(crate) destination: u32,
    pub(crate) length: u32,
    pub(crate) bss: u32,
    pub(crate) bss_length: u32,
    pub(crate) source: u32,
    pub(crate) stored_length: u32,
}

pub struct SegmentTable<'a> {
    bytes: &'a [u8],
    entries: usize,
}

impl<'a> SegmentTable<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, FormatError> {
        let bytes = bytes.get(..TABLE_BYTES).ok_or(FormatError::Truncated)?;
        let entries = table_offset(word(bytes, 0)?, 4)?;
        table_offset(word(bytes, 12)?, 8)?;
        let table = Self { bytes, entries };
        let mut cursor = entries;
        loop {
            if word(bytes, cursor)? == u32::MAX {
                return Ok(table);
            }
            if cursor.checked_add(36).ok_or(FormatError::InvalidTable)? > TABLE_BYTES {
                return Err(FormatError::InvalidTable);
            }
            cursor += 36;
        }
    }

    pub fn find(&self, id: ImageId) -> Result<Segment, FormatError> {
        let segment = self.lookup(SectionId::Image(id))?;
        if segment.source & (UNIT_BYTES - 1) != 0 || segment.length < 16 || segment.stored_length == 0 {
            return Err(FormatError::InvalidHeader);
        }
        segment.source.checked_add(segment.stored_length).ok_or(FormatError::AddressRange)?;
        Ok(segment)
    }

    pub fn metadata(&self, section: MetadataSection) -> Result<SectionMetadata, FormatError> {
        let segment = self.lookup(SectionId::Metadata(section))?;
        segment.source.checked_add(segment.stored_length).ok_or(FormatError::AddressRange)?;
        Ok(SectionMetadata { source_offset: LogicalMediaOffset(segment.source) })
    }

    fn lookup(&self, id: SectionId) -> Result<Segment, FormatError> {
        let mut cursor = self.entries;
        loop {
            let found = word(self.bytes, cursor)?;
            if found == u32::MAX {
                return Err(id.missing());
            }
            if found == id.word() {
                return Ok(Segment {
                    destination: word(self.bytes, cursor + 4)?,
                    length: word(self.bytes, cursor + 8)?,
                    bss: word(self.bytes, cursor + 12)?,
                    bss_length: word(self.bytes, cursor + 16)?,
                    source: word(self.bytes, cursor + 20)?,
                    stored_length: word(self.bytes, cursor + 24)?,
                });
            }
            cursor += 36;
        }
    }
}

pub unsafe fn section_metadata(section: MetadataSection) -> Result<SectionMetadata, FormatError> {
    let bytes = core::slice::from_raw_parts((TABLE_ADDRESS | 0x4000_0000) as *const u8, TABLE_BYTES);
    SegmentTable::parse(bytes)?.metadata(section)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_lookup_requires_bounded_terminated_entries() {
        let mut bytes = [0; TABLE_BYTES];
        bytes[0..4].copy_from_slice(&0x4061c040u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&0x4061c184u32.to_le_bytes());
        for (index, value) in [4u32, 0x61c400, 0x282828, 0x89ec00, 0x352e00, 0x160000, 0x160000, 32, 1, u32::MAX].iter().enumerate() {
            bytes[0x40 + index * 4..0x44 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        let table = SegmentTable::parse(&bytes).unwrap();
        assert_eq!(table.find(ImageId::Application).unwrap().source, 0x160000);
        assert!(matches!(table.find(ImageId::PrimaryLoader), Err(FormatError::Missing(ImageId::PrimaryLoader))));
        bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(SegmentTable::parse(&bytes), Err(FormatError::InvalidTable)));
        assert!(matches!(SegmentTable::parse(&bytes[..32]), Err(FormatError::Truncated)));
    }

    #[test]
    fn metadata_uses_live_source_without_image_destination_constraints() {
        let mut bytes = [0; TABLE_BYTES];
        bytes[0..4].copy_from_slice(&0x4061c040u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&0x4061c184u32.to_le_bytes());
        bytes[0x40..0x44].copy_from_slice(&7u32.to_le_bytes());
        bytes[0x58..0x5c].copy_from_slice(&0x20000u32.to_le_bytes());
        bytes[0x64..0x68].copy_from_slice(&u32::MAX.to_le_bytes());
        for source in [0x00b40000u32, 0x01800000] {
            bytes[0x54..0x58].copy_from_slice(&source.to_le_bytes());
            let metadata = SegmentTable::parse(&bytes).unwrap().metadata(MetadataSection::LensDefaults).unwrap();
            assert_eq!(metadata.source_offset.bytes(), source);
        }
        bytes[0x54..0x58].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(SegmentTable::parse(&bytes).unwrap().metadata(MetadataSection::LensDefaults), Err(FormatError::AddressRange)));
        bytes[0x40..0x44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(SegmentTable::parse(&bytes).unwrap().metadata(MetadataSection::LensDefaults), Err(FormatError::MissingMetadata(MetadataSection::LensDefaults))));
    }
}
