use super::{add, be32, bytes, jpeg, MediaError, MediaFormat, MediaMetadata, Source, ThumbRange};

pub(super) fn parse(source: &mut impl Source, format: MediaFormat) -> Result<MediaMetadata, MediaError> {
    if source.len() <= 0x88 { return Err(MediaError::Truncated); }
    let signature = bytes::<16>(source, 0)?;
    if &signature[..8] != b"FUJIFILM" || &signature[8..15] != b"CCD-RAW" {
        return Err(MediaError::InvalidSignature);
    }
    let jpeg_offset = be32(source, 0x54)?;
    let jpeg_length = be32(source, 0x58)?;
    add(jpeg_offset, jpeg_length)?;
    let available = source.len().checked_sub(jpeg_offset).ok_or(MediaError::Truncated)?;
    let readable = available.min(jpeg_length);
    let mut metadata = jpeg::parse(source, jpeg_offset, readable, format)?;
    metadata.embedded_image = Some(ThumbRange { offset: jpeg_offset, bytes: jpeg_length });
    let preview = jpeg::dimensions(source, jpeg_offset, readable, false)?;
    metadata.secondary_width = preview.width;
    metadata.secondary_height = preview.height;
    metadata.components = u32::from(preview.components);
    Ok(metadata)
}
