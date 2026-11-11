use super::{add, be32, bytes, jpeg, range, ImageDimensions, MediaError, MediaFormat, MediaMetadata, Source};

#[derive(Default)]
struct Movie {
    width: Option<u32>, height: Option<u32>, precision: Option<u16>,
    timescale: u32, ticks: u32, audio_channels: u16, audio_rate: u32,
    sample_bytes: u64, avc: bool, tags: Option<jpeg::TiffMetadata>,
}
#[derive(Clone, Copy)]
struct Atom { start: u32, payload: u32, end: u32, tag: [u8; 4] }
fn atom(source: &mut impl Source, at: u32, end: u32) -> Result<Atom, MediaError> {
    if end - at < 8 { return Err(MediaError::Truncated); }
    let header = bytes::<8>(source, at)?;
    let size = u32::from_be_bytes(header[..4].try_into().map_err(|_| MediaError::Malformed)?);
    let (size, header_bytes) = match size {
        0 => (end - at, 8),
        1 => {
            if end - at < 16 { return Err(MediaError::Truncated); }
            let extended = u64::from_be_bytes(bytes(source, add(at, 8)?)?);
            (u32::try_from(extended).map_err(|_| MediaError::SizeOverflow)?, 16)
        }
        size => (size, 8),
    };
    if size < header_bytes { return Err(MediaError::Malformed); }
    let atom_end = add(at, size)?;
    if atom_end > end { return Err(MediaError::Truncated); }
    Ok(Atom { start: at, payload: add(at, header_bytes)?, end: atom_end,
        tag: header[4..8].try_into().map_err(|_| MediaError::Malformed)? })
}
fn field(source: &mut impl Source, atom: Atom, offset: u32) -> Result<u32, MediaError> {
    let at = add(atom.payload, offset)?;
    if add(at, 4)? > atom.end { return Err(MediaError::Truncated); }
    be32(source, at)
}
fn sample_description(source: &mut impl Source, parent: Atom, movie: &mut Movie) -> Result<(), MediaError> {
    let count = field(source, parent, 4)?;
    let mut at = add(parent.payload, 8)?;
    for _ in 0..count {
        let entry = atom(source, at, parent.end)?;
        match &entry.tag {
            b"avc1" | b"jpeg" | b"mjpa" | b"mjpb" => {
                if entry.end - entry.start < 86 { return Err(MediaError::Truncated); }
                let dimensions = bytes::<4>(source, add(entry.start, 32)?)?;
                movie.width = Some(u32::from(u16::from_be_bytes([dimensions[0], dimensions[1]])));
                movie.height = Some(u32::from(u16::from_be_bytes([dimensions[2], dimensions[3]])));
                movie.precision = Some(u16::from_be_bytes(bytes(source, add(entry.start, 82)?)?));
                movie.avc = entry.tag == *b"avc1";
            }
            b"sowt" | b"twos" | b"raw " | b"lpcm" => {
                if entry.end - entry.start < 36 { return Err(MediaError::Truncated); }
                movie.audio_channels = u16::from_be_bytes(bytes(source, add(entry.start, 24)?)?);
                movie.audio_rate = u32::from(u16::from_be_bytes(bytes(source, add(entry.start, 32)?)?));
            }
            _ => {}
        }
        at = entry.end;
    }
    Ok(())
}
fn walk(source: &mut impl Source, start: u32, end: u32, depth: u8, movie: &mut Movie) -> Result<(), MediaError> {
    if depth > 12 { return Err(MediaError::Malformed); }
    let mut at = start;
    while at < end {
        let item = atom(source, at, end)?;
        match &item.tag {
            b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" | b"udta" | b"edts" =>
                walk(source, item.payload, item.end, depth + 1, movie)?,
            b"mvhd" => {
                let version = bytes::<1>(source, item.payload)?[0];
                match version {
                    0 => { movie.timescale = field(source, item, 12)?; movie.ticks = field(source, item, 16)?; }
                    1 => {
                        movie.timescale = field(source, item, 20)?;
                        if field(source, item, 24)? != 0 { return Err(MediaError::SizeOverflow); }
                        movie.ticks = field(source, item, 28)?;
                    }
                    _ => return Err(MediaError::UnsupportedFormat),
                }
            }
            b"stsd" => sample_description(source, item, movie)?,
            b"stsz" => {
                let uniform = field(source, item, 4)?;
                let count = field(source, item, 8)?;
                let total = if uniform == 0 {
                    let records = add(item.payload, 12)?;
                    if add(records, count.checked_mul(4).ok_or(MediaError::SizeOverflow)?)? > item.end {
                        return Err(MediaError::Truncated);
                    }
                    let mut total = 0u64;
                    for index in 0..count { total += u64::from(be32(source, add(records, index * 4)?)?); }
                    total
                } else { u64::from(uniform) * u64::from(count) };
                movie.sample_bytes = movie.sample_bytes.checked_add(total).ok_or(MediaError::SizeOverflow)?;
            }
            b"MVTG" => {
                if item.end - item.start < 24 { return Err(MediaError::Truncated); }
                let declared = u32::from_le_bytes(bytes(source, add(item.start, 16)?)?);
                if declared < 4 { return Err(MediaError::Malformed); }
                let tiff_base = add(item.start, 24)?;
                let tiff_end = add(add(item.start, 20)?, declared)?;
                if tiff_end > item.end { return Err(MediaError::Truncated); }
                movie.tags = Some(jpeg::movie_tiff(source, tiff_base, tiff_end)?);
            }
            _ => {}
        }
        at = item.end;
    }
    Ok(())
}
pub(super) fn parse(source: &mut impl Source) -> Result<MediaMetadata, MediaError> {
    let length = source.len();
    if length < 8 { return Err(MediaError::Truncated); }
    let mut movie = Movie::default();
    walk(source, 0, length, 0, &mut movie)?;
    let tagged_size = movie.tags.as_ref().and_then(jpeg::TiffMetadata::image_size);
    let (width, height) = match movie.width.zip(movie.height).or(tagged_size) {
        Some((width, height)) if width != 0 && height != 0 => (width, height),
        _ => return Err(MediaError::MissingDimensions),
    };
    let precision = movie.precision.unwrap_or(0);
    let mut metadata = MediaMetadata::image(MediaFormat::Movie, ImageDimensions {
        width, height, precision: 0, components: 0, sampling: 0,
    });
    metadata.bit_depth = u32::from(precision);
    metadata.secondary_width = movie.width.unwrap_or(0);
    metadata.secondary_height = movie.height.unwrap_or(0);
    metadata.duration = movie.timescale;
    metadata.average_size = movie.ticks.checked_div(movie.timescale).unwrap_or(0);
    metadata.sample_count = if movie.avc { 2 } else { 1 };
    metadata.sample_format = if movie.audio_channels <= 2 { movie.audio_channels } else { 0 };
    metadata.sample_rate = movie.audio_rate;
    let seconds = movie.ticks.checked_div(movie.timescale).unwrap_or(0);
    metadata.stream_flags = u32::try_from(movie.sample_bytes.checked_div(u64::from(seconds)).unwrap_or(0)
        .checked_mul(8).ok_or(MediaError::SizeOverflow)?).map_err(|_| MediaError::SizeOverflow)?;
    if let Some(tags) = movie.tags {
        jpeg::apply_thumbnail(source, &mut metadata, &tags)?;
        metadata.secondary_width = movie.width.unwrap_or(0);
        metadata.secondary_height = movie.height.unwrap_or(0);
    }
    range(source, 0, length)?;
    Ok(metadata)
}
