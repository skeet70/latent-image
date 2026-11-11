use super::{add, bytes, range, ImageDimensions, MediaError, MediaFormat, MediaMetadata, Source, ThumbRange};

pub(super) fn dimensions(source: &mut impl Source, start: u32, length: u32,
    require_application: bool) -> Result<ImageDimensions, MediaError> {
    scan(source, start, length, require_application, None)
}
fn scan(source: &mut impl Source, start: u32, length: u32, require_application: bool,
    mut metadata: Option<&mut TiffMetadata>) -> Result<ImageDimensions, MediaError> {
    let end = range(source, start, length)?;
    if bytes::<2>(source, start)? != [0xff, 0xd8] { return Err(MediaError::InvalidSignature); }
    let mut cursor = add(start, 2)?;
    let mut application = false;
    while cursor < end {
        if end - cursor < 4 { return Err(MediaError::Truncated); }
        let marker = bytes::<4>(source, cursor)?;
        if marker[0] != 0xff { return Err(MediaError::Malformed); }
        let size = u32::from(u16::from_be_bytes([marker[2], marker[3]]));
        if size < 2 { return Err(MediaError::Malformed); }
        let next = add(cursor, size + 2)?;
        if next > end { return Err(MediaError::Truncated); }
        match marker[1] {
            0xe0 => application = true,
            0xe1 if size >= 8 => {
                if bytes::<6>(source, add(cursor, 4)?)? == *b"Exif\0\0" {
                    application = true;
                    if let Some(output) = metadata.as_deref_mut() {
                        *output = tiff(source, add(cursor, 10)?, next)?;
                    }
                }
            }
            0xc0 => {
                if require_application && !application { return Err(MediaError::InvalidSignature); }
                if size < 8 { return Err(MediaError::Malformed); }
                let header = bytes::<6>(source, add(cursor, 4)?)?;
                let count = header[5];
                if count == 0 || size != 8 + 3 * u32::from(count) { return Err(MediaError::Malformed); }
                let width = u32::from(u16::from_be_bytes([header[3], header[4]]));
                let height = u32::from(u16::from_be_bytes([header[1], header[2]]));
                if width == 0 || height == 0 || header[0] == 0 { return Err(MediaError::MissingDimensions); }
                let component = bytes::<3>(source, add(cursor, 10)?)?;
                let sampling = match component[1] { 0x21 => 2, 0x22 => 3, _ => 1 };
                return Ok(ImageDimensions { width, height, precision: header[0], components: count, sampling });
            }
            0xc1..=0xcf if !matches!(marker[1], 0xc4 | 0xc8 | 0xcc) => return Err(MediaError::UnsupportedFormat),
            0xda | 0xd9 => return Err(MediaError::MissingDimensions),
            _ => {}
        }
        cursor = next;
    }
    Err(MediaError::Truncated)
}
#[derive(Clone, Copy)]
enum Order { Little, Big }
impl Order {
    fn half(self, input: [u8; 2]) -> u16 {
        match self { Self::Little => u16::from_le_bytes(input), Self::Big => u16::from_be_bytes(input) }
    }
    fn word(self, input: [u8; 4]) -> u32 {
        match self { Self::Little => u32::from_le_bytes(input), Self::Big => u32::from_be_bytes(input) }
    }
}
#[derive(Default)]
pub(super) struct TiffMetadata {
    width: Option<u32>, height: Option<u32>, precision: Option<u8>, components: Option<u8>,
    thumbnail_offset: Option<u32>, thumbnail_bytes: Option<u32>, thumbnail_width: Option<u32>,
    thumbnail_height: Option<u32>, thumbnail_compression: Option<u32>,
    orientation: Option<u16>,
}
impl TiffMetadata {
    pub(super) fn image_size(&self) -> Option<(u32, u32)> {
        self.width.zip(self.height).filter(|(width, height)| *width != 0 && *height != 0)
    }
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum Directory { Primary, Exif, Gps, Interop, Maker, Thumbnail }
struct Tiff<'a, S> { source: &'a mut S, base: u32, end: u32, order: Order }
impl<S: Source> Tiff<'_, S> {
    fn read<const N: usize>(&mut self, at: u32) -> Result<[u8; N], MediaError> {
        if add(at, N as u32)? > self.end { return Err(MediaError::Truncated); }
        bytes(self.source, at)
    }
    fn directory(&mut self, offset: u32, kind: Directory, output: &mut TiffMetadata,
        ancestors: &mut [u32; 8], depth: usize) -> Result<(), MediaError> {
        if offset == 0 && kind != Directory::Primary { return Ok(()); }
        if depth == ancestors.len() || ancestors[..depth].contains(&offset) { return Err(MediaError::Malformed); }
        ancestors[depth] = offset;
        let at = add(self.base, offset)?;
        let encoded_count = self.read(at)?;
        let count = u32::from(self.order.half(encoded_count));
        let records = add(at, 2)?;
        let next_at = add(records, count * 12)?;
        let encoded_next = self.read(next_at)?;
        let next = self.order.word(encoded_next);
        for index in 0..count {
            let record_at = add(records, index * 12)?;
            let record = self.read::<12>(record_at)?;
            let tag = self.order.half([record[0], record[1]]);
            let value_type = self.order.half([record[2], record[3]]);
            let count = self.order.word([record[4], record[5], record[6], record[7]]);
            let raw = [record[8], record[9], record[10], record[11]];
            let word = self.order.word(raw);
            let nested = match (kind, tag) {
                (Directory::Primary, 0x8769) => Some(Directory::Exif),
                (Directory::Primary, 0x8825) => Some(Directory::Gps),
                (Directory::Exif, 0xa005) => Some(Directory::Interop),
                _ => None,
            };
            if let Some(nested) = nested {
                if value_type != 4 || count != 1 { return Err(MediaError::Malformed); }
                self.directory(word, nested, output, ancestors, depth + 1)?;
                continue;
            }
            if kind == Directory::Exif && tag == 0x927c && count >= 12 {
                let maker_base = add(self.base, word)?;
                let signature = self.read::<12>(maker_base)?;
                if signature[..8] == *b"FUJIFILM" {
                    let maker_offset = u32::from_le_bytes(signature[8..12].try_into().map_err(|_| MediaError::Malformed)?);
                    let mut maker = Tiff { source: self.source, base: maker_base, end: self.end, order: Order::Little };
                    maker.directory(maker_offset, Directory::Maker, output, &mut [0; 8], 0)?;
                }
                continue;
            }
            if count == 0 { continue; }
            let value = match value_type {
                1 | 7 if count == 1 => Some(u32::from(raw[0])),
                3 => {
                    let encoded = if count <= 2 { [raw[0], raw[1]] }
                        else { self.read::<2>(add(self.base, word)?)? };
                    Some(u32::from(self.order.half(encoded)))
                }
                4 if count == 1 => Some(word),
                _ => None,
            };
            if let Some(value) = value {
                match (kind, tag) {
                    (Directory::Primary, 0x100) | (Directory::Exif, 0xa002) => output.width = Some(value),
                    (Directory::Primary, 0x101) | (Directory::Exif, 0xa003) => output.height = Some(value),
                    (Directory::Primary, 0x102) => output.precision = Some(u8::try_from(value).map_err(|_| MediaError::Malformed)?),
                    (Directory::Primary, 0x115) => output.components = Some(u8::try_from(value).map_err(|_| MediaError::Malformed)?),
                    (Directory::Primary, 0x112) => output.orientation = Some(u16::try_from(value).map_err(|_| MediaError::Malformed)?),
                    (Directory::Thumbnail, 0x100) => output.thumbnail_width = Some(value),
                    (Directory::Thumbnail, 0x101) => output.thumbnail_height = Some(value),
                    (Directory::Thumbnail, 0x103) => output.thumbnail_compression = Some(value),
                    (Directory::Thumbnail, 0x201) => output.thumbnail_offset = Some(add(self.base, value)?),
                    (Directory::Thumbnail, 0x202) => output.thumbnail_bytes = Some(value),
                    _ => {}
                }
            }
        }
        if kind == Directory::Primary {
            self.directory(next, Directory::Thumbnail, output, ancestors, depth + 1)?;
        }
        Ok(())
    }
}
pub(super) fn tiff(source: &mut impl Source, base: u32, end: u32) -> Result<TiffMetadata, MediaError> {
    if add(base, 8)? > end { return Err(MediaError::Truncated); }
    let header = bytes::<8>(source, base)?;
    let order = match &header[..4] { b"II*\0" => Order::Little, b"MM\0*" => Order::Big,
        _ => return Err(MediaError::InvalidSignature) };
    let offset = order.word(header[4..8].try_into().map_err(|_| MediaError::Malformed)?);
    let mut output = TiffMetadata::default();
    Tiff { source, base, end, order }.directory(offset, Directory::Primary, &mut output, &mut [0; 8], 0)?;
    Ok(output)
}
pub(super) fn movie_tiff(source: &mut impl Source, base: u32, end: u32) -> Result<TiffMetadata, MediaError> {
    let mut output = TiffMetadata::default();
    Tiff { source, base, end, order: Order::Little }.directory(0, Directory::Primary, &mut output, &mut [0; 8], 0)?;
    Ok(output)
}
pub(super) fn apply_thumbnail(source: &mut impl Source, metadata: &mut MediaMetadata,
    tiff: &TiffMetadata) -> Result<(), MediaError> {
    metadata.secondary_width = tiff.width.unwrap_or(0);
    metadata.secondary_height = tiff.height.unwrap_or(0);
    metadata.orientation = tiff.orientation.unwrap_or(0);
    match (tiff.thumbnail_offset, tiff.thumbnail_bytes) {
        (None, None) | (Some(0), Some(0)) => Ok(()),
        (Some(offset), Some(length)) if length != 0 => {
            range(source, offset, length)?;
            if tiff.thumbnail_compression.is_some_and(|compression| compression != 6) {
                return Err(MediaError::UnsupportedFormat);
            }
            let dimensions = dimensions(source, offset, length, false)?;
            metadata.thumbnail = Some(ThumbRange { offset, bytes: length });
            metadata.data_size = offset;
            metadata.secondary_size = length;
            metadata.primary_width = dimensions.width;
            metadata.primary_height = dimensions.height;
            Ok(())
        }
        _ => Err(MediaError::Malformed),
    }
}
pub(super) fn parse(source: &mut impl Source, start: u32, length: u32,
    format: MediaFormat) -> Result<MediaMetadata, MediaError> {
    let end = range(source, start, length)?;
    let signature = bytes::<2>(source, start)?;
    let mut tags = TiffMetadata::default();
    let dimensions = if signature == *b"II" || signature == *b"MM" {
        tags = tiff(source, start, end)?;
        let width = tags.width.filter(|value| *value != 0).ok_or(MediaError::MissingDimensions)?;
        let height = tags.height.filter(|value| *value != 0).ok_or(MediaError::MissingDimensions)?;
        ImageDimensions { width, height, precision: tags.precision.ok_or(MediaError::MissingDimensions)?,
            components: tags.components.ok_or(MediaError::MissingDimensions)?, sampling: 1 }
    } else { scan(source, start, length, true, Some(&mut tags))? };
    let mut metadata = MediaMetadata::image(format, dimensions);
    apply_thumbnail(source, &mut metadata, &tags)?;
    Ok(metadata)
}
