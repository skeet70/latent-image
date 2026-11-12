use core::ptr::{read_volatile, write_volatile};

use super::engine::{self, Bus, Destination, EngineError, Session, Source};
use super::media::{self, MediaError};
use super::format::{
    word, FormatError, ImageId, MetadataSection, Segment, SegmentTable,
    TABLE_ADDRESS as TABLE, TABLE_BYTES as TABLE_LEN, UNIT_BYTES as UNIT,
};

const SCRATCH: u32 = 0x42e4_0000;
/// Stock 0x1dac reads the handoff argument at the fixed 0x61c40c, not at the record's load address.
const HANDOFF_WORD: u32 = 0x0061_c40c;
/// The stock image-4 load address, whose header word 3 is `HANDOFF_WORD`.
#[cfg(any(target_arch = "arm", test))]
const IMAGE: u32 = 0x0061_c400;

#[cfg(not(target_arch = "arm"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    ZeroToken,
    InvalidMatch,
    OutputLimit,
    Truncated,
    OutputLength,
}

#[derive(Debug)]
pub enum ImageError {
    Media(MediaError),
    Truncated,
    InvalidTable,
    Missing(ImageId),
    MissingMetadata(MetadataSection),
    InvalidHeader,
    InvalidTrailer,
    AddressRange,
    Overlap,
    Engine(EngineError),
    Fill(media::FillError),
}

impl From<media::FillError> for ImageError {
    fn from(error: media::FillError) -> Self {
        Self::Fill(error)
    }
}

impl From<EngineError> for ImageError {
    fn from(error: EngineError) -> Self {
        Self::Engine(error)
    }
}

impl From<MediaError> for ImageError {
    fn from(error: MediaError) -> Self {
        Self::Media(error)
    }
}

impl From<FormatError> for ImageError {
    fn from(error: FormatError) -> Self {
        match error {
            FormatError::Truncated => Self::Truncated,
            FormatError::InvalidTable => Self::InvalidTable,
            FormatError::Missing(id) => Self::Missing(id),
            FormatError::MissingMetadata(section) => Self::MissingMetadata(section),
            FormatError::InvalidHeader => Self::InvalidHeader,
            FormatError::AddressRange => Self::AddressRange,
        }
    }
}

fn physical(address: u32) -> u32 {
    address & !0x4000_0000
}

#[derive(Clone, Copy, Debug)]
pub struct RamRange {
    start: u32,
    end: u32,
}

impl RamRange {
    pub fn new(address: u32, length: u32) -> Result<Self, ImageError> {
        let start = physical(address);
        let end = start.checked_add(length).ok_or(ImageError::AddressRange)?;
        if start < 0x40000 || end > 0x2000_0000 {
            return Err(ImageError::AddressRange);
        }
        Ok(Self { start, end })
    }

    fn refusing(self, reserved: &[Self]) -> Result<Self, ImageError> {
        if reserved.iter().any(|other| self.overlaps(*other)) {
            return Err(ImageError::Overlap);
        }
        Ok(self)
    }

    fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    fn alias(self) -> u32 {
        self.start | 0x4000_0000
    }
}

/// The table bytes the stage and image 4's loaders read, from the record's own table.
fn table_range(table: &SegmentTable<'_>) -> Result<RamRange, ImageError> {
    let length = u32::try_from(table.read_length()?).map_err(|_| ImageError::AddressRange)?;
    let end = TABLE.checked_add(length).ok_or(ImageError::AddressRange)?;
    Ok(RamRange { start: TABLE, end })
}

/// The whole extent the staged record occupies while jobs run.
fn staged_range(record_length: u32) -> Result<RamRange, ImageError> {
    let bytes = unit_count(record_length)?.checked_mul(UNIT).ok_or(ImageError::AddressRange)?;
    RamRange::new(SCRATCH, bytes)
}

#[derive(Clone, Copy, Debug)]
pub struct CompressedHeader {
    pub decoded_length: u32,
    pub record_length: u32,
}

impl CompressedHeader {
    /// Stock never reads the record's segment word 2, so the header's decoded length is not
    /// bounded by it; `handoff_entry` bounds the destination against the handoff word instead.
    pub fn parse(bytes: &[u8], stored_length: u32) -> Result<Self, ImageError> {
        if bytes.len() < 20 {
            return Err(ImageError::Truncated);
        }
        let decoded_length = word(bytes, 0)?;
        let record_length = word(bytes, 4)?;
        if decoded_length < 16 || record_length <= 20 || record_length > stored_length
            || word(bytes, 12)? != 0x4000 || bytes[16] != 4
        {
            return Err(ImageError::InvalidHeader);
        }
        Ok(Self { decoded_length, record_length })
    }
}

// Token laws match lz/fflzd-codec.c and FujiMod compression::method4::Stream.
#[cfg(not(target_arch = "arm"))]
#[derive(Default)]
pub struct Method4 {
    produced: usize,
    literal: usize,
    match_token: Option<u8>,
}

#[cfg(not(target_arch = "arm"))]
impl Method4 {
    pub fn feed(&mut self, input: &[u8], output: &mut [u8]) -> Result<(), DecodeError> {
        for &byte in input {
            if self.literal != 0 {
                let slot = output.get_mut(self.produced).ok_or(DecodeError::OutputLimit)?;
                *slot = byte;
                self.produced += 1;
                self.literal -= 1;
            } else if let Some(token) = self.match_token.take() {
                let distance = ((token as usize & 127) << 4) | (byte as usize >> 4);
                let length = byte as usize & 15;
                if distance == 0 || distance > self.produced || length == 0 {
                    return Err(DecodeError::InvalidMatch);
                }
                if length > output.len().saturating_sub(self.produced) {
                    return Err(DecodeError::OutputLimit);
                }
                for _ in 0..length {
                    output[self.produced] = output[self.produced - distance];
                    self.produced += 1;
                }
            } else if byte == 0 {
                return Err(DecodeError::ZeroToken);
            } else if byte < 128 {
                if byte as usize > output.len().saturating_sub(self.produced) {
                    return Err(DecodeError::OutputLimit);
                }
                self.literal = byte as usize;
            } else {
                self.match_token = Some(byte);
            }
        }
        Ok(())
    }

    pub fn finish(self, expected: usize) -> Result<(), DecodeError> {
        if self.literal != 0 || self.match_token.is_some() {
            return Err(DecodeError::Truncated);
        }
        if self.produced != expected {
            return Err(DecodeError::OutputLength);
        }
        Ok(())
    }

    pub fn produced(&self) -> usize {
        self.produced
    }
}

pub fn check_trailer(bytes: &[u8]) -> Result<(), ImageError> {
    let marker = bytes.get(30..32).ok_or(ImageError::Truncated)?;
    if marker != [0xaa, 0xaa] {
        return Err(ImageError::InvalidTrailer);
    }
    Ok(())
}

unsafe fn table_bytes() -> &'static [u8] {
    core::slice::from_raw_parts((TABLE | 0x4000_0000) as *const u8, TABLE_LEN)
}

/// The media reads the stage performs.
pub trait Media {
    fn read_units(&mut self, source: u32, destination: u32, count: u32) -> Result<(), MediaError>;
    /// The bytes a completed read staged at `destination`.
    unsafe fn staged(&self, destination: u32, length: usize) -> &[u8];
}

/// The first stage's NAND controller and the RAM it stages into.
pub struct VolatileMedia;

impl Media for VolatileMedia {
    fn read_units(&mut self, source: u32, destination: u32, count: u32) -> Result<(), MediaError> {
        unsafe { media::read_units(source, destination, count) }
    }

    unsafe fn staged(&self, destination: u32, length: usize) -> &[u8] {
        core::slice::from_raw_parts(destination as *const u8, length)
    }
}

/// The three reads whose nonzero status stock discards: the table read at `0x1f5c`, loader A's
/// at `0x1d4c` and loader B's at `0x1d58`. A timeout ends stock's unit loop with status 6
/// (`0x3738`..`0x3794`), which those callers drop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiscardedRead {
    Table,
    PrimaryLoader,
    CompanionLoader,
}

impl DiscardedRead {
    /// The workspace word holding this site's last discarded status.
    fn status_word(self) -> u32 {
        match self {
            Self::Table => 0x4808,
            Self::PrimaryLoader => 0x480c,
            Self::CompanionLoader => 0x4810,
        }
    }
}

/// Stock's policy at those sites: every status, including a timeout, is discarded.
fn discard_status(status: Result<(), MediaError>) -> Option<MediaError> {
    status.err()
}

fn read_units_discarding<M: Media>(media: &mut M, _site: DiscardedRead, source: u32, destination: u32, count: u32) -> Option<MediaError> {
    let error = discard_status(media.read_units(source, destination, count));
    #[cfg(target_arch = "arm")]
    if let Some(error) = error {
        unsafe { write_volatile(_site.status_word() as *mut u32, error.status()) };
    }
    error
}

pub unsafe fn segment_table<M: Media>(media: &mut M) -> Result<(), ImageError> {
    read_units_discarding(media, DiscardedRead::Table, 0x140000, TABLE, 1);
    SegmentTable::parse(media.staged(TABLE | 0x4000_0000, TABLE_LEN)).map(|_| ()).map_err(ImageError::from)
}

#[no_mangle]
#[inline(never)]
pub unsafe fn xm1_first_stage_trailer_check() -> Result<(), ImageError> {
    media::read_units(0x24a0000, SCRATCH, 1)?;
    check_trailer(core::slice::from_raw_parts(SCRATCH as *const u8, 32))
}

fn unit_count(length: u32) -> Result<u32, ImageError> {
    length.checked_add(UNIT - 1).map(|value| value / UNIT).ok_or(ImageError::AddressRange)
}

unsafe fn copy_loader<M: Media>(media: &mut M, read: DiscardedRead, segment: Segment, staged: RamRange, table: RamRange) -> Result<(), ImageError> {
    let reserved = [staged, table];
    let first = RamRange::new(segment.destination, UNIT)?.refusing(&reserved)?;
    let first_status = read_units_discarding(media, read, segment.source, first.alias(), 1);
    let header = media.staged(first.alias(), 32);
    let length = word(header, 8)?;
    let entry = physical(word(header, 12)?);
    if physical(word(header, 4)?) != first.start || length < 32 || length > segment.length || length > segment.stored_length {
        return Err(ImageError::InvalidHeader);
    }
    let count = unit_count(length)?;
    let rounded = count.checked_mul(UNIT).ok_or(ImageError::AddressRange)?;
    RamRange::new(segment.destination, rounded)?.refusing(&reserved)?;
    let local_entry = entry >= first.start && entry < first.start + length;
    let owned_companion_entry = read == DiscardedRead::CompanionLoader && (0x10400..0x11800).contains(&entry);
    if (!local_entry && !owned_companion_entry) || entry & 3 != 0 {
        return Err(ImageError::InvalidHeader);
    }
    // Stock 0x2360/0x2364 returns after a nonzero first-unit status without reading the rest.
    if first_status.is_none() && count > 1 {
        read_units_discarding(media, read, segment.source.checked_add(UNIT).ok_or(ImageError::AddressRange)?, first.alias() + UNIT, count - 1);
    }
    Ok(())
}

/// Stock 0x22e0 passes the record's segment word 1 to setup 0x3d3c as the decode destination.
pub fn application_ranges(segment: Segment, header: CompressedHeader, table: RamRange) -> Result<(RamRange, Option<RamRange>), ImageError> {
    let reserved = [staged_range(header.record_length)?, table];
    let destination = RamRange::new(segment.destination, header.decoded_length)?.refusing(&reserved)?;
    let bss_length = segment.bss_length & !3;
    if bss_length == 0 {
        return Ok((destination, None));
    }
    if segment.bss & 3 != 0 {
        return Err(ImageError::AddressRange);
    }
    let bss = RamRange::new(segment.bss, bss_length)?.refusing(&reserved)?;
    if destination.overlaps(bss) {
        return Err(ImageError::Overlap);
    }
    Ok((destination, Some(bss)))
}

/// One 128 KiB unit of the compressed record, staged at `destination`.
pub trait UnitSource {
    fn stage(&mut self, offset: u32, destination: u32) -> Result<(), MediaError>;
}

#[derive(Debug)]
pub enum StageFailure {
    /// Stock 0x3d08 spins at 0x3d28 with the engine still open.
    EngineFault(u32),
    /// A non-timeout read error does not stop stock's unit loop (0x3740-0x3778), and finish
    /// 0x3dc0 submits and waits for the unconsumed tail (0x3e14-0x3e98) before closing.
    Failed(ImageError),
}

impl From<media::FillError> for StageFailure {
    fn from(error: media::FillError) -> Self {
        Self::Failed(ImageError::Fill(error))
    }
}

/// Stock 0x3ea0's feed: a job per staged unit when no job is deferred (0x3f04-0x3f20), bounded by the feed end.
fn drive<B: Bus, U: UnitSource>(
    session: &mut Session<B>,
    units: &mut U,
    header: &CompressedHeader,
    destination: u32,
) -> Result<u32, ImageError> {
    let staged = unit_count(header.record_length)?;
    let mut consumed = 0u32;
    let mut produced = 0u32;
    for index in 0..staged {
        let offset = index.checked_mul(UNIT).ok_or(ImageError::AddressRange)?;
        if index != 0 {
            let address = SCRATCH.checked_add(offset).ok_or(ImageError::AddressRange)?;
            units.stage(offset, address)?;
        }
        let available = (index + 1).checked_mul(UNIT).ok_or(ImageError::AddressRange)?.min(header.record_length);
        let end = engine::feed_end(available, header.record_length, engine::PAGE_BYTES)?;
        if consumed < end {
            let count = session.submit(
                Source(SCRATCH.checked_add(0x14 + consumed).ok_or(ImageError::AddressRange)?),
                Destination(destination.checked_add(produced).ok_or(ImageError::AddressRange)?),
                end - consumed,
            )?;
            produced = produced.checked_add(count).ok_or(ImageError::AddressRange)?;
            consumed = end;
        }
    }
    Ok(produced)
}

/// Stages the record's remaining units and drives stock's per-unit feed.
pub fn stage_record<B: Bus, U: UnitSource>(
    mut session: Session<B>,
    units: &mut U,
    header: &CompressedHeader,
    destination: u32,
) -> Result<u32, StageFailure> {
    match drive(&mut session, units, header, destination) {
        Ok(produced) => {
            session.close();
            Ok(produced)
        }
        Err(ImageError::Engine(EngineError::Fault(status))) => Err(StageFailure::EngineFault(status)),
        Err(error) => {
            session.close();
            Err(StageFailure::Failed(error))
        }
    }
}

struct MediaUnits {
    source: u32,
}

impl UnitSource for MediaUnits {
    fn stage(&mut self, offset: u32, destination: u32) -> Result<(), MediaError> {
        let source = self.source.checked_add(offset).ok_or(MediaError::AddressRange)?;
        unsafe { media::read_units(source, destination, 1) }
    }
}

fn halt_in_place() -> ! {
    loop {
        core::hint::spin_loop()
    }
}

unsafe fn decompress(segment: Segment, table: RamRange) -> Result<(), ImageError> {
    media::read_units(segment.source, SCRATCH, 1)?;
    let input = core::slice::from_raw_parts(SCRATCH as *const u8, UNIT as usize);
    let header = CompressedHeader::parse(input, segment.stored_length)?;
    let (destination, bss) = application_ranges(segment, header, table)?;
    let mut units = MediaUnits { source: segment.source };
    let decoded = decode_and_hand_off(
        engine::VolatileBus,
        &mut units,
        &header,
        destination,
        bss,
        &mut media::VolatilePeripherals,
        || read_volatile((HANDOFF_WORD | 0x4000_0000) as *const u32),
    );
    match decoded {
        Ok(_) => Ok(()),
        Err(StageFailure::EngineFault(_)) => halt_in_place(),
        Err(StageFailure::Failed(error)) => Err(error),
    }
}

/// Stock's 0x22a0-0x2318 decode sequence and 0x1dac handoff check. `handoff_word` is the
/// fixed word stock reads at 0x61c40c.
fn decode_and_hand_off<B: Bus, U: UnitSource, P: media::PeripheralBus>(
    bus: B,
    units: &mut U,
    header: &CompressedHeader,
    destination: RamRange,
    bss: Option<RamRange>,
    fill: &mut P,
    handoff_word: impl FnOnce() -> u32,
) -> Result<u32, StageFailure> {
    let filling = match bss {
        Some(bss) => {
            media::fill_start(fill, bss.start, media::Words::from_bytes(bss.end - bss.start))?;
            true
        }
        None => false,
    };
    let session = Session::open(bus);
    let staged = stage_record(session, units, header, destination.alias());
    if filling {
        media::fill_wait(fill);
    }
    match staged {
        Ok(_) => (),
        Err(StageFailure::EngineFault(status)) => return Err(StageFailure::EngineFault(status)),
        Err(error) => return Err(error),
    }
    handoff_entry(handoff_word(), destination).map_err(StageFailure::Failed)
}

/// The handoff word names a word inside the decoded image; stock jumps to it unchecked.
/// The decode must have written the fixed word stock reads, `[0x61c40c,0x61c410)`.
fn handoff_entry(word: u32, destination: RamRange) -> Result<u32, ImageError> {
    if HANDOFF_WORD < destination.start || HANDOFF_WORD.checked_add(4).ok_or(ImageError::AddressRange)? > destination.end {
        return Err(ImageError::InvalidHeader);
    }
    let entry = physical(word);
    if entry < destination.start || entry >= destination.end || entry & 3 != 0 {
        return Err(ImageError::InvalidHeader);
    }
    Ok(entry)
}

pub fn staging_ranges(primary: Segment, companion: Segment, application: Segment, header: CompressedHeader, table: RamRange) -> Result<RamRange, ImageError> {
    let staged = staged_range(header.record_length)?;
    let reserved = [staged, table];
    let primary = RamRange::new(primary.destination, unit_count(primary.length)?.checked_mul(UNIT).ok_or(ImageError::AddressRange)?)?
        .refusing(&reserved)?;
    let companion = RamRange::new(companion.destination, unit_count(companion.length)?.checked_mul(UNIT).ok_or(ImageError::AddressRange)?)?
        .refusing(&reserved)?;
    let (application, bss) = application_ranges(application, header, table)?;
    if primary.overlaps(companion) || primary.overlaps(application) || companion.overlaps(application) {
        return Err(ImageError::Overlap);
    }
    if let Some(bss) = bss {
        if primary.overlaps(bss) || companion.overlaps(bss) {
            return Err(ImageError::Overlap);
        }
    }
    Ok(staged)
}

#[cfg(target_arch = "arm")]
#[no_mangle]
#[inline(never)]
pub unsafe fn xm1_first_stage_stage_images(core_zero: bool) -> Result<(), ImageError> {
    let segments = SegmentTable::parse(table_bytes())?;
    let table = table_range(&segments)?;
    let primary = segments.find(ImageId::PrimaryLoader)?;
    let companion = segments.find(ImageId::CompanionLoader)?;
    let mut nand = VolatileMedia;
    if core_zero {
        let application = segments.find(ImageId::Application)?;
        media::read_units(application.source, SCRATCH, 1)?;
        let header = CompressedHeader::parse(
            core::slice::from_raw_parts(SCRATCH as *const u8, 20),
            application.stored_length,
        )?;
        let staged = staging_ranges(primary, companion, application, header, table)?;
        super::log_event(0x0100_0200, super::LogChannel::Zero);
        copy_loader(&mut nand, DiscardedRead::PrimaryLoader, primary, staged, table)?;
        copy_loader(&mut nand, DiscardedRead::CompanionLoader, companion, staged, table)?;
        decompress(application, table)?;
        super::log_event(0x1100_0200, super::LogChannel::Zero);
    }
    let loader = if core_zero { primary } else { companion };
    let loader = RamRange::new(loader.destination, 16)?.refusing(&[table])?;
    write_volatile(0x4300 as *mut u32, read_volatile((loader.alias() + 12) as *const u32));
    write_volatile(0x4304 as *mut u32, read_volatile((HANDOFF_WORD | 0x4000_0000) as *const u32));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn segment() -> Segment {
        Segment {
            destination: IMAGE,
            length: 0x282828,
            bss: 0x89ec00,
            bss_length: 0x352e00,
            source: 0x160000,
            stored_length: 0x160000,
        }
    }

    /// The stock table shape: eight segment entries at 0x61c040 and three sub-list entries.
    fn stock_table() -> Vec<u8> {
        let pointer = |offset: u32| (TABLE + offset) | 0x4000_0000;
        let mut bytes = vec![0u8; TABLE_LEN];
        bytes[0..4].copy_from_slice(&pointer(0x40).to_le_bytes());
        bytes[4..8].copy_from_slice(&pointer(0x1a4).to_le_bytes());
        bytes[12..16].copy_from_slice(&pointer(0x184).to_le_bytes());
        for id in 0..8u32 {
            let at = 0x40 + id as usize * 36;
            bytes[at..at + 4].copy_from_slice(&id.to_le_bytes());
        }
        bytes[0x160..0x164].copy_from_slice(&u32::MAX.to_le_bytes());
        bytes[0x1a4..0x1a8].copy_from_slice(&pointer(0x1b0).to_le_bytes());
        bytes[0x1a8..0x1ac].copy_from_slice(&0x0110_0000u32.to_le_bytes());
        bytes[0x1ac..0x1b0].copy_from_slice(&0x0036_0000u32.to_le_bytes());
        for index in 0..3u32 {
            let at = 0x1b0 + index as usize * 24;
            bytes[at..at + 4].copy_from_slice(&index.to_le_bytes());
            bytes[at + 4..at + 8].copy_from_slice(&(0x0110_0000u32 + index * 0x10000).to_le_bytes());
        }
        bytes[0x1f8..0x1fc].copy_from_slice(&u32::MAX.to_le_bytes());
        bytes
    }

    fn stock_table_range() -> RamRange {
        let bytes = stock_table();
        let table = SegmentTable::parse(&bytes).unwrap();
        RamRange { start: TABLE, end: TABLE + table.read_length().unwrap() as u32 }
    }

    #[test]
    fn the_table_reserves_the_bytes_the_stage_and_loaders_read() {
        let bytes = stock_table();
        let table = SegmentTable::parse(&bytes).unwrap();
        assert_eq!(table.read_length().unwrap(), 0x1fc);
    }

    /// The publication cell is the last object read when header word 3 points at the unit's end.
    #[test]
    fn a_publication_cell_at_the_table_end_is_reserved() {
        let mut bytes = stock_table();
        let pointer = |offset: u32| (TABLE + offset) | 0x4000_0000;
        bytes[12..16].copy_from_slice(&pointer(0x3f8).to_le_bytes());
        bytes[0x3f8..0x3fc].copy_from_slice(&pointer(0x0c).to_le_bytes());
        bytes[0x3fc..0x400].copy_from_slice(&0x0000_4310u32.to_le_bytes());
        let table = SegmentTable::parse(&bytes).unwrap();
        assert_eq!(table.read_length().unwrap(), 0x400);
        let range = RamRange { start: TABLE, end: TABLE + 0x400 };
        let header = CompressedHeader { decoded_length: 0x282800, record_length: 0x146567 };
        let mut application = segment();
        application.destination = 0x0061_c340;
        assert!(matches!(application_ranges(application, header, range), Err(ImageError::Overlap)));
    }

    /// A sub-list header above its entries extends the read extent past the terminator.
    #[test]
    fn a_sub_list_header_above_its_entries_is_reserved() {
        let mut bytes = stock_table();
        let pointer = |offset: u32| (TABLE + offset) | 0x4000_0000;
        bytes[4..8].copy_from_slice(&pointer(0x3f0).to_le_bytes());
        bytes[0x3f0..0x3f4].copy_from_slice(&pointer(0x300).to_le_bytes());
        bytes[0x300..0x304].copy_from_slice(&0u32.to_le_bytes());
        bytes[0x318..0x31c].copy_from_slice(&u32::MAX.to_le_bytes());
        let table = SegmentTable::parse(&bytes).unwrap();
        assert_eq!(table.read_length().unwrap(), 0x3fc);
        let range = RamRange { start: TABLE, end: TABLE + 0x3fc };
        let header = CompressedHeader { decoded_length: 0x282800, record_length: 0x146567 };
        let mut application = segment();
        application.destination = 0x0061_c340;
        assert!(matches!(application_ranges(application, header, range), Err(ImageError::Overlap)));
    }

    /// Stock drops every status at the table read (0x1f5c) and both loader reads (0x1d4c, 0x1d58),
    /// including the timeout its unit loop ends with (0x3738..0x3794).
    #[test]
    fn the_three_reads_discard_every_status() {
        assert_eq!(DiscardedRead::Table.status_word(), 0x4808);
        assert_eq!(DiscardedRead::PrimaryLoader.status_word(), 0x480c);
        assert_eq!(DiscardedRead::CompanionLoader.status_word(), 0x4810);
        assert_eq!(discard_status(Ok(())), None);
        for error in [MediaError::ReadFault, MediaError::SkipBudget, MediaError::Timeout] {
            assert_eq!(discard_status(Err(error)), Some(error));
        }
    }

    struct DiscardingMedia {
        statuses: Vec<Result<(), MediaError>>,
        header: Vec<u8>,
        table: Vec<u8>,
        /// Withhold the table until a read has completed.
        table_after_read: bool,
        reads: Vec<(u32, u32, u32)>,
    }

    impl DiscardingMedia {
        fn new(header: Vec<u8>, statuses: Vec<Result<(), MediaError>>) -> Self {
            Self { statuses, header, table: stock_table(), table_after_read: false, reads: Vec::new() }
        }
    }

    impl Media for DiscardingMedia {
        fn read_units(&mut self, source: u32, destination: u32, count: u32) -> Result<(), MediaError> {
            self.reads.push((source, destination, count));
            self.statuses.get(self.reads.len() - 1).copied().unwrap_or(Ok(()))
        }

        unsafe fn staged(&self, destination: u32, length: usize) -> &[u8] {
            if destination & !0x4000_0000 == TABLE {
                if self.table_after_read && self.reads.is_empty() {
                    return &self.table[..0];
                }
                &self.table[..length]
            } else {
                &self.header[..length]
            }
        }
    }

    fn loader_header(destination: u32, length: u32, entry: u32) -> Vec<u8> {
        let mut bytes = vec![0u8; 32];
        bytes[4..8].copy_from_slice(&destination.to_le_bytes());
        bytes[8..12].copy_from_slice(&length.to_le_bytes());
        bytes[12..16].copy_from_slice(&entry.to_le_bytes());
        bytes
    }

    /// Stock's table read continues after a timeout and parses the unit it staged.
    #[test]
    fn the_table_read_continues_after_a_timeout() {
        let mut media = DiscardingMedia::new(Vec::new(), vec![Err(MediaError::Timeout)]);
        assert!(unsafe { segment_table(&mut media) }.is_ok());
        assert_eq!(media.reads, vec![(0x140000, TABLE, 1)]);
    }

    /// The table bytes exist only once the controller's read has staged them.
    #[test]
    fn the_table_is_parsed_from_the_bytes_the_read_staged() {
        let mut media = DiscardingMedia::new(Vec::new(), vec![Err(MediaError::Timeout)]);
        media.table_after_read = true;
        assert!(unsafe { segment_table(&mut media) }.is_ok());
        assert_eq!(media.reads, vec![(0x140000, TABLE, 1)]);
    }

    /// Stock's loader read continues after a timeout, and a nonzero first-unit status of a loader
    /// longer than one unit returns without reading the remaining units (0x2360, 0x2364).
    #[test]
    fn the_loader_reads_continue_after_a_timeout_and_skip_a_failed_first_unit() {
        for status in [MediaError::Timeout, MediaError::ReadFault] {
            let segment = loader(0x40000, 0x30000);
            let mut media = DiscardingMedia::new(loader_header(0x40000, 0x30000, 0x40020), vec![Err(status)]);
            let staged = RamRange::new(0x42e4_0000, 0x40000).unwrap();
            let table = stock_table_range();
            unsafe { copy_loader(&mut media, DiscardedRead::PrimaryLoader, segment, staged, table) }.unwrap();
            assert_eq!(media.reads, vec![(segment.source, 0x4004_0000, 1)]);
        }
    }

    /// After a zero first-unit status and a header length above one unit, stock reads the rest
    /// one unit on in both the source and the staged destination (0x2384-0x23b0).
    #[test]
    fn the_loader_reads_its_remaining_units_after_a_successful_first_unit() {
        let segment = loader(0x40000, 0x30000);
        let mut media = DiscardingMedia::new(loader_header(0x40000, 0x30000, 0x40020), vec![Ok(()), Ok(())]);
        let staged = RamRange::new(0x42e4_0000, 0x40000).unwrap();
        let table = stock_table_range();
        unsafe { copy_loader(&mut media, DiscardedRead::PrimaryLoader, segment, staged, table) }.unwrap();
        assert_eq!(media.reads, vec![(0x400000, 0x4004_0000, 1), (0x420000, 0x4006_0000, 1)]);
    }

    /// The decoded range must contain the fixed handoff word stock reads at 0x61c40c.
    #[test]
    fn the_entry_check_requires_the_handoff_word_in_the_decoded_range() {
        assert_eq!(handoff_entry(0x0061_c440, RamRange::new(0x0061_c400, 0x100).unwrap()).unwrap(), 0x0061_c440);
        assert_eq!(handoff_entry(0x0069_cdf8, RamRange::new(0x0061_c340, 0x2828c0).unwrap()).unwrap(), 0x0069_cdf8);
        assert!(matches!(handoff_entry(0x0061_c440, RamRange::new(0x0061_c410, 0x100).unwrap()), Err(ImageError::InvalidHeader)));
        assert!(matches!(handoff_entry(0x0061_0000, RamRange::new(0x0060_0000, 0x1c40c).unwrap()), Err(ImageError::InvalidHeader)));
    }

    #[test]
    fn overlapping_matches_survive_every_input_split() {
        let encoded = [3, b'a', b'b', b'c', 0x80, 0x3f, 2, b'd', b'e'];
        for split in 0..=encoded.len() {
            let mut decoder = Method4::default();
            let mut output = [0; 20];
            decoder.feed(&encoded[..split], &mut output).unwrap();
            decoder.feed(&encoded[split..], &mut output).unwrap();
            decoder.finish(20).unwrap();
            assert_eq!(&output, b"abcabcabcabcabcabcde");
        }
    }

    #[test]
    fn malformed_and_truncated_streams_refuse() {
        for input in [&[0][..], &[0x80, 0x11], &[1, b'a', 0x80, 0x10]] {
            assert!(Method4::default().feed(input, &mut [0; 32]).is_err());
        }
        for input in [&[2, b'a'][..], &[0x80]] {
            let mut decoder = Method4::default();
            decoder.feed(input, &mut [0; 32]).unwrap();
            assert_eq!(decoder.finish(1), Err(DecodeError::Truncated));
        }
        assert_eq!(Method4::default().feed(&[2, 1, 2], &mut [0; 1]), Err(DecodeError::OutputLimit));
        assert_eq!(Method4::default().finish(1), Err(DecodeError::OutputLength));
    }

    #[test]
    fn record_and_trailer_validation_reject_short_or_invalid_data() {
        let mut bytes = [0; 32];
        bytes[0..4].copy_from_slice(&0x282800u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x146567u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&0x4000u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&4u32.to_le_bytes());
        assert!(CompressedHeader::parse(&bytes, 0x160000).is_ok());
        assert!(matches!(CompressedHeader::parse(&bytes[..19], 0x160000), Err(ImageError::Truncated)));
        assert!(matches!(CompressedHeader::parse(&bytes, 0x100000), Err(ImageError::InvalidHeader)));
        assert!(matches!(check_trailer(&bytes[..31]), Err(ImageError::Truncated)));
        assert!(matches!(check_trailer(&bytes), Err(ImageError::InvalidTrailer)));
        bytes[30..32].copy_from_slice(&[0xaa, 0xaa]);
        assert!(check_trailer(&bytes).is_ok());
    }

    /// Stock never reads the segment's word 2, so a decoded length above it is not refused.
    #[test]
    fn a_decoded_length_above_the_segment_word_two_is_accepted() {
        let mut bytes = [0; 32];
        bytes[0..4].copy_from_slice(&0x300000u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x146567u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&0x4000u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&4u32.to_le_bytes());
        let header = CompressedHeader::parse(&bytes, 0x160000).unwrap();
        assert_eq!(header.decoded_length, 0x300000);
        let mut application = segment();
        application.length = 0x282828;
        application.bss = 0;
        application.bss_length = 0;
        assert!(application_ranges(application, header, stock_table_range()).is_ok());
    }

    #[test]
    fn bss_owns_only_disjoint_whole_words() {
        let header = CompressedHeader { decoded_length: 0x282800, record_length: 0x146567 };
        let (_, bss) = application_ranges(segment(), header, stock_table_range()).unwrap();
        let bss = bss.unwrap();
        assert_eq!((bss.start, bss.end), (0x89ec00, 0xbf1a00));
        let mut invalid = segment();
        invalid.bss -= 4;
        assert!(matches!(application_ranges(invalid, header, stock_table_range()), Err(ImageError::Overlap)));
        invalid.bss = 0xffff_fffc;
        invalid.bss_length = 8;
        assert!(matches!(application_ranges(invalid, header, stock_table_range()), Err(ImageError::AddressRange)));
        assert!(matches!(RamRange::new(0x1fff_ffff, 2), Err(ImageError::AddressRange)));
        assert!(matches!(unit_count(u32::MAX), Err(ImageError::AddressRange)));
    }

    const DESTINATION: u32 = IMAGE | 0x4000_0000;

    struct PlanBus {
        registers: [u32; 0x90 / 4],
        jobs: Rc<RefCell<Vec<(u32, u32, u32)>>>,
    }

    impl Bus for PlanBus {
        fn read(&mut self, register: engine::Register) -> u32 {
            match register {
                engine::Register::Engine(engine::EngineRegister::Reset) => 1,
                engine::Register::Engine(engine::EngineRegister::Status) => 1,
                engine::Register::Engine(register) => self.registers[register as usize / 4],
                engine::Register::Clock(_) => 0,
            }
        }

        fn write(&mut self, register: engine::Register, value: u32) {
            if let engine::Register::Engine(engine::EngineRegister::Start) = register {
                let at = |register: engine::EngineRegister| self.registers[register as usize / 4];
                self.jobs.borrow_mut().push((
                    at(engine::EngineRegister::Source),
                    at(engine::EngineRegister::Length),
                    at(engine::EngineRegister::Destination),
                ));
                self.registers[engine::EngineRegister::OutputCount as usize / 4] =
                    at(engine::EngineRegister::Length);
            }
            if let engine::Register::Engine(register) = register {
                self.registers[register as usize / 4] = value;
            }
        }
    }

    struct PlanUnits {
        offsets: Vec<u32>,
    }

    impl UnitSource for PlanUnits {
        fn stage(&mut self, offset: u32, destination: u32) -> Result<(), MediaError> {
            assert_eq!(destination, SCRATCH + offset);
            self.offsets.push(offset);
            Ok(())
        }
    }

    #[test]
    fn stock_record_length_plans_eleven_feed_jobs() {
        let header = CompressedHeader { decoded_length: 0x282800, record_length: 0x146567 };
        let jobs = Rc::new(RefCell::new(Vec::new()));
        let mut units = PlanUnits { offsets: Vec::new() };
        let session = Session::open(PlanBus { registers: [0; 0x90 / 4], jobs: jobs.clone() });
        let produced = stage_record(session, &mut units, &header, DESTINATION).unwrap();
        let lengths = [0x1ffec, 0x20000, 0x20000, 0x20000, 0x20000, 0x20000, 0x20000, 0x20000, 0x20000, 0x20000, 0x6567];
        let mut source = SCRATCH + 0x14;
        let mut destination = DESTINATION;
        let mut expected = Vec::new();
        for length in lengths {
            expected.push((source, length, destination));
            source += length;
            destination += length;
        }
        assert_eq!(produced, 0x146553);
        assert_eq!(*jobs.borrow(), expected);
        assert_eq!(units.offsets, (1..11).map(|index| index * UNIT).collect::<Vec<_>>());
    }

    struct Decoder {
        source: Rc<RefCell<Vec<u8>>>,
        output: Rc<RefCell<Vec<u8>>>,
        stream: Method4,
        registers: [u32; 0x90 / 4],
        clock: [u32; 0x40 / 4],
        jobs: Rc<RefCell<Vec<(u32, u32, u32)>>>,
        writes: Rc<RefCell<Vec<(engine::Register, u32)>>>,
        fault: Option<u32>,
        /// Bytes each job under-reports in the output count.
        shortfall: u32,
    }

    impl Decoder {
        fn start(&mut self) {
            let at = |register: engine::EngineRegister| self.registers[register as usize / 4];
            let source = (at(engine::EngineRegister::Source) - SCRATCH) as usize;
            let length = at(engine::EngineRegister::Length) as usize;
            let destination = at(engine::EngineRegister::Destination);
            let before = self.stream.produced();
            self.stream
                .feed(&self.source.borrow()[source..source + length], &mut self.output.borrow_mut()[..])
                .unwrap();
            let count = (self.stream.produced() - before) as u32;
            self.registers[engine::EngineRegister::OutputCount as usize / 4] = count - self.shortfall;
            self.jobs.borrow_mut().push((source as u32, length as u32, destination));
            self.registers[engine::EngineRegister::Status as usize / 4] = self.fault.unwrap_or(1);
        }
    }

    impl Bus for Decoder {
        fn read(&mut self, register: engine::Register) -> u32 {
            match register {
                engine::Register::Engine(engine::EngineRegister::Reset) => 1,
                engine::Register::Engine(register) => self.registers[register as usize / 4],
                engine::Register::Clock(register) => self.clock[register as usize / 4],
            }
        }

        fn write(&mut self, register: engine::Register, value: u32) {
            self.writes.borrow_mut().push((register, value));
            match register {
                engine::Register::Engine(engine::EngineRegister::Start) => self.start(),
                engine::Register::Engine(register) => self.registers[register as usize / 4] = value,
                engine::Register::Clock(register) => self.clock[register as usize / 4] = value,
            }
        }
    }

    struct StreamUnits {
        record: Vec<u8>,
        source: Rc<RefCell<Vec<u8>>>,
        staged: Rc<RefCell<Vec<u32>>>,
        fail_at: Option<u32>,
    }

    impl UnitSource for StreamUnits {
        fn stage(&mut self, offset: u32, destination: u32) -> Result<(), MediaError> {
            assert_eq!(destination, SCRATCH + offset);
            if self.fail_at == Some(offset) {
                return Err(MediaError::ReadFault);
            }
            let start = offset as usize;
            let end = (start + UNIT as usize).min(self.record.len());
            self.source.borrow_mut()[start..end].copy_from_slice(&self.record[start..end]);
            self.staged.borrow_mut().push(offset);
            Ok(())
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Event {
        FillRead(u32),
        FillWrite(u32),
        EngineRead(engine::Register),
        EngineWrite(engine::Register),
    }

    /// The 0xfff02000 register file, completing immediately and optionally logging to `log`.
    #[derive(Default)]
    struct Fill {
        words: std::collections::BTreeMap<u32, u32>,
        log: Option<Rc<RefCell<Vec<Event>>>>,
    }

    impl media::PeripheralBus for Fill {
        fn read(&mut self, register: media::PeripheralRegister) -> u32 {
            let address = register.address();
            if let Some(log) = &self.log {
                log.borrow_mut().push(Event::FillRead(address));
            }
            if address == media::FILL_BASE + 0x24 {
                0x40
            } else {
                self.words.get(&address).copied().unwrap_or(0)
            }
        }

        fn write(&mut self, register: media::PeripheralRegister, value: u32) {
            let address = register.address();
            if let Some(log) = &self.log {
                log.borrow_mut().push(Event::FillWrite(address));
            }
            self.words.insert(address, value);
        }
    }

    struct Logged<B> {
        bus: B,
        log: Rc<RefCell<Vec<Event>>>,
    }

    impl<B: Bus> Bus for Logged<B> {
        fn read(&mut self, register: engine::Register) -> u32 {
            self.log.borrow_mut().push(Event::EngineRead(register));
            self.bus.read(register)
        }

        fn write(&mut self, register: engine::Register, value: u32) {
            self.log.borrow_mut().push(Event::EngineWrite(register));
            self.bus.write(register, value);
        }
    }

    /// One-byte literal runs over three units: every second input byte is a literal.
    fn spanning_record() -> (Vec<u8>, u32, Vec<u8>) {
        const RECORD_LENGTH: u32 = 0x50000;
        const DECODED: usize = ((RECORD_LENGTH - 20) / 2) as usize;
        let mut record = vec![0u8; RECORD_LENGTH as usize];
        record[0..4].copy_from_slice(&(DECODED as u32).to_le_bytes());
        record[4..8].copy_from_slice(&RECORD_LENGTH.to_le_bytes());
        record[12..16].copy_from_slice(&engine::PAGE_BYTES.to_le_bytes());
        record[16..20].copy_from_slice(&4u32.to_le_bytes());
        let mut decoded = Vec::with_capacity(DECODED);
        for index in 0..DECODED {
            let byte = (index % 251) as u8;
            record[20 + 2 * index] = 1;
            record[21 + 2 * index] = byte;
            decoded.push(byte);
        }
        (record, RECORD_LENGTH, decoded)
    }

    #[test]
    fn a_three_unit_record_decodes_across_units() {
        let (record, record_length, decoded) = spanning_record();
        let header = CompressedHeader { decoded_length: decoded.len() as u32, record_length };
        let source = Rc::new(RefCell::new(vec![0u8; record_length as usize]));
        source.borrow_mut()[..UNIT as usize].copy_from_slice(&record[..UNIT as usize]);
        let output = Rc::new(RefCell::new(vec![0u8; decoded.len()]));
        let jobs = Rc::new(RefCell::new(Vec::new()));
        let mut units = StreamUnits {
            record,
            source: source.clone(),
            staged: Rc::new(RefCell::new(Vec::new())),
            fail_at: None,
        };
        let decoder = Decoder {
            source,
            output: output.clone(),
            stream: Method4::default(),
            registers: [0; 0x90 / 4],
            clock: [0; 0x40 / 4],
            jobs: jobs.clone(),
            writes: Rc::new(RefCell::new(Vec::new())),
            fault: None,
            shortfall: 0,
        };
        let produced = stage_record(Session::open(decoder), &mut units, &header, DESTINATION).unwrap();
        assert_eq!(produced as usize, decoded.len());
        assert_eq!(*units.staged.borrow(), vec![UNIT, 2 * UNIT]);
        assert_eq!(
            *jobs.borrow(),
            vec![
                (0x14, 0x1ffec, DESTINATION),
                (0x14 + 0x1ffec, 0x20000, DESTINATION + 0xfff6),
                (0x14 + 0x3ffec, 0x10000, DESTINATION + 0x1fff6),
            ]
        );
        assert_eq!(*output.borrow(), decoded);
    }

    /// Stock reads the output count at 0x3ce4 only to advance the next destination (0x3c7c); nothing compares the 0x400042e0 total.
    #[test]
    fn a_decode_that_reports_a_different_final_count_still_hands_off() {
        let (record, record_length, decoded) = spanning_record();
        let header = CompressedHeader { decoded_length: decoded.len() as u32, record_length };
        let source = Rc::new(RefCell::new(vec![0u8; record_length as usize]));
        source.borrow_mut()[..UNIT as usize].copy_from_slice(&record[..UNIT as usize]);
        let jobs = Rc::new(RefCell::new(Vec::new()));
        let decoder = Decoder {
            source: source.clone(),
            output: Rc::new(RefCell::new(vec![0u8; decoded.len()])),
            stream: Method4::default(),
            registers: [0; 0x90 / 4],
            clock: [0; 0x40 / 4],
            jobs: jobs.clone(),
            writes: Rc::new(RefCell::new(Vec::new())),
            fault: None,
            shortfall: 1,
        };
        let mut units = StreamUnits {
            record,
            source,
            staged: Rc::new(RefCell::new(Vec::new())),
            fail_at: None,
        };
        let destination = RamRange::new(0x0061_c400, decoded.len() as u32).unwrap();
        let bss = Some(RamRange::new(0x0089_ec00, 0x0035_2e00).unwrap());
        let mut fill = Fill::default();
        let entry = decode_and_hand_off(
            decoder, &mut units, &header, destination, bss, &mut fill, || 0x0061_c410,
        )
        .unwrap();
        assert_eq!(entry, 0x0061_c410);
        assert_eq!(
            *jobs.borrow(),
            vec![
                (0x14, 0x1ffec, DESTINATION),
                (0x14 + 0x1ffec, 0x20000, DESTINATION + 0xfff5),
                (0x14 + 0x3ffec, 0x10000, DESTINATION + 0x1fff4),
            ]
        );
    }

    #[test]
    fn the_bss_fill_starts_before_the_session_and_is_waited_on_after_finish() {
        let (record, record_length, decoded) = spanning_record();
        let header = CompressedHeader { decoded_length: decoded.len() as u32, record_length };
        let source = Rc::new(RefCell::new(vec![0u8; record_length as usize]));
        source.borrow_mut()[..UNIT as usize].copy_from_slice(&record[..UNIT as usize]);
        let log = Rc::new(RefCell::new(Vec::new()));
        let decoder = Logged {
            bus: Decoder {
                source: source.clone(),
                output: Rc::new(RefCell::new(vec![0u8; decoded.len()])),
                stream: Method4::default(),
                registers: [0; 0x90 / 4],
                clock: [0; 0x40 / 4],
                jobs: Rc::new(RefCell::new(Vec::new())),
                writes: Rc::new(RefCell::new(Vec::new())),
                fault: None,
                shortfall: 0,
            },
            log: log.clone(),
        };
        let mut units = StreamUnits {
            record,
            source,
            staged: Rc::new(RefCell::new(Vec::new())),
            fail_at: None,
        };
        let destination = RamRange::new(0x0061_c400, decoded.len() as u32).unwrap();
        let bss = RamRange::new(0x0089_ec00, 0x0035_2e00).unwrap();
        let mut fill = Fill { words: Default::default(), log: Some(log.clone()) };
        decode_and_hand_off(
            decoder, &mut units, &header, destination, Some(bss), &mut fill, || 0x0061_c410,
        )
        .unwrap();
        let events = log.borrow();
        let engine = |event: &Event| matches!(event, Event::EngineRead(_) | Event::EngineWrite(_));
        let first_engine = events.iter().position(engine).unwrap();
        let last_start = events.iter().rposition(|event| *event == Event::FillWrite(media::FILL_BASE + 0x28)).unwrap();
        let last_engine = events.iter().rposition(engine).unwrap();
        let first_status = events.iter().position(|event| *event == Event::FillRead(media::FILL_BASE + 0x24)).unwrap();
        assert!(last_start < first_engine);
        assert!(last_engine < first_status);
    }

    #[test]
    fn the_record_supplies_the_image_four_load_address() {
        let table = stock_table_range();
        let (record, record_length, decoded) = spanning_record();
        let header = CompressedHeader { decoded_length: decoded.len() as u32, record_length };
        let mut application = segment();
        application.destination = 0x0061_c1f0;
        assert!(matches!(application_ranges(application, header, table), Err(ImageError::Overlap)));
        application.destination = 0x0061_c340;
        let (destination, _) = application_ranges(application, header, table).unwrap();
        assert_eq!(destination.alias(), 0x4061_c340);
        let source = Rc::new(RefCell::new(vec![0u8; record_length as usize]));
        source.borrow_mut()[..UNIT as usize].copy_from_slice(&record[..UNIT as usize]);
        let output = Rc::new(RefCell::new(vec![0u8; decoded.len()]));
        let jobs = Rc::new(RefCell::new(Vec::new()));
        let mut units = StreamUnits {
            record,
            source: source.clone(),
            staged: Rc::new(RefCell::new(Vec::new())),
            fail_at: None,
        };
        let decoder = Decoder {
            source,
            output: output.clone(),
            stream: Method4::default(),
            registers: [0; 0x90 / 4],
            clock: [0; 0x40 / 4],
            jobs: jobs.clone(),
            writes: Rc::new(RefCell::new(Vec::new())),
            fault: None,
            shortfall: 0,
        };
        let produced = stage_record(Session::open(decoder), &mut units, &header, destination.alias()).unwrap();
        assert_eq!(produced as usize, decoded.len());
        assert_eq!(*output.borrow(), decoded);
        assert_eq!(
            *jobs.borrow(),
            vec![
                (0x14, 0x1ffec, 0x4061_c340),
                (0x14 + 0x1ffec, 0x20000, 0x4061_c340 + 0xfff6),
                (0x14 + 0x3ffec, 0x10000, 0x4061_c340 + 0x1fff6),
            ]
        );
    }

    /// The RC0 record's image-4 entry and compressed header, read from
    /// FWUP0002-prefix.DAT (sha256 acb0fbbb9507421f48a6254920cafb5ce20e1fd6bf821bc80c912c1f99c29ca8):
    /// image 4 loads at 0x61c340 and its entry word sits at the fixed 0x61c40c.
    fn rc0_segment() -> Segment {
        Segment {
            destination: 0x0061_c340,
            length: 0x2828e8,
            bss: 0x89ec00,
            bss_length: 0x352e00,
            source: 0x160000,
            stored_length: 0x160000,
        }
    }

    fn rc0_header_bytes() -> [u8; 20] {
        let mut bytes = [0; 20];
        bytes[0..4].copy_from_slice(&0x2828c0u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x146602u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&0x52u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&0x4000u32.to_le_bytes());
        bytes[16] = 4;
        bytes
    }

    fn rc0_table() -> Vec<u8> {
        let mut bytes = stock_table();
        let at = 0x40 + 4 * 36;
        let segment = rc0_segment();
        for (index, value) in [
            segment.destination, segment.length, segment.bss,
            segment.bss_length, segment.source, segment.stored_length,
        ].into_iter().enumerate() {
            let offset = at + 4 + index * 4;
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn the_rc0_record_header_passes_the_stage_refusals() {
        let bytes = rc0_table();
        let table = SegmentTable::parse(&bytes).unwrap();
        let range = RamRange { start: TABLE, end: TABLE + table.read_length().unwrap() as u32 };
        let application = table.find(ImageId::Application).unwrap();
        let header = CompressedHeader::parse(&rc0_header_bytes(), application.stored_length).unwrap();
        assert_eq!((header.decoded_length, header.record_length), (0x2828c0, 0x146602));
        let (destination, bss) = application_ranges(application, header, range).unwrap();
        assert_eq!(destination.alias(), 0x4061_c340);
        assert_eq!(bss.map(|bss| (bss.start, bss.end)), Some((0x89ec00, 0xbf1a00)));
        assert!(staging_ranges(loader(0x40000, 0xdae0), loader(0xd0000, 0xdaa0), application, header, range).is_ok());
    }

    /// A record that decodes `body` through one literal run per byte pair.
    fn literal_record(body: &[u8]) -> Vec<u8> {
        let mut record = vec![0u8; 20];
        record[0..4].copy_from_slice(&(body.len() as u32).to_le_bytes());
        record[4..8].copy_from_slice(&((20 + 2 * body.len()) as u32).to_le_bytes());
        record[12..16].copy_from_slice(&engine::PAGE_BYTES.to_le_bytes());
        record[16..20].copy_from_slice(&4u32.to_le_bytes());
        for &byte in body {
            record.push(1);
            record.push(byte);
        }
        record
    }

    #[test]
    fn the_rc0_layout_stages_and_hands_off_through_the_fixed_word() {
        let table = RamRange { start: TABLE, end: TABLE + 0x1fc };
        let real = CompressedHeader::parse(&rc0_header_bytes(), rc0_segment().stored_length).unwrap();
        let (destination, _) = application_ranges(rc0_segment(), real, table).unwrap();
        assert_eq!(destination.alias(), 0x4061_c340);
        let mut body = vec![0u8; 0x100];
        body[0xcc..0xd0].copy_from_slice(&0x0069_cdf8u32.to_le_bytes());
        let record = literal_record(&body);
        let header = CompressedHeader::parse(&record, rc0_segment().stored_length).unwrap();
        let source = Rc::new(RefCell::new(record.clone()));
        let output = Rc::new(RefCell::new(vec![0u8; body.len()]));
        let decoder = Decoder {
            source: source.clone(),
            output: output.clone(),
            stream: Method4::default(),
            registers: [0; 0x90 / 4],
            clock: [0; 0x40 / 4],
            jobs: Rc::new(RefCell::new(Vec::new())),
            writes: Rc::new(RefCell::new(Vec::new())),
            fault: None,
            shortfall: 0,
        };
        let mut units = StreamUnits {
            record,
            source,
            staged: Rc::new(RefCell::new(Vec::new())),
            fail_at: None,
        };
        let produced = stage_record(Session::open(decoder), &mut units, &header, destination.alias()).unwrap();
        assert_eq!(produced as usize, body.len());
        let staged = output.borrow();
        let word = u32::from_le_bytes(staged[0xcc..0xd0].try_into().unwrap());
        assert_eq!(word, 0x0069_cdf8);
        assert_eq!(handoff_entry(word, destination).unwrap(), 0x0069_cdf8);
        assert_eq!(u32::from_le_bytes(staged[0xc..0x10].try_into().unwrap()), 0);
        assert!(matches!(handoff_entry(0, destination), Err(ImageError::InvalidHeader)));
    }

    #[test]
    fn engine_fault_leaves_the_session_open() {
        let header = CompressedHeader { decoded_length: 1, record_length: 22 };
        let mut record = vec![0u8; 22];
        record[20] = 1;
        record[21] = b'a';
        let source = Rc::new(RefCell::new(record));
        let writes = Rc::new(RefCell::new(Vec::new()));
        let decoder = Decoder {
            source,
            output: Rc::new(RefCell::new(vec![0u8; 1])),
            stream: Method4::default(),
            registers: [0; 0x90 / 4],
            clock: [0; 0x40 / 4],
            jobs: Rc::new(RefCell::new(Vec::new())),
            writes: writes.clone(),
            fault: Some(0x101),
            shortfall: 0,
        };
        let mut units = StreamUnits {
            record: Vec::new(),
            source: Rc::new(RefCell::new(Vec::new())),
            staged: Rc::new(RefCell::new(Vec::new())),
            fail_at: None,
        };
        match stage_record(Session::open(decoder), &mut units, &header, DESTINATION) {
            Err(StageFailure::EngineFault(status)) => assert_eq!(status, 0x101),
            other => panic!("{:?}", other),
        }
        assert_eq!(
            writes.borrow().last(),
            Some(&(engine::Register::Engine(engine::EngineRegister::Start), 2))
        );
    }

    #[test]
    fn media_error_closes_the_engine_and_gates_the_clock() {
        let (record, record_length, decoded) = spanning_record();
        let header = CompressedHeader { decoded_length: decoded.len() as u32, record_length };
        let source = Rc::new(RefCell::new(vec![0u8; record_length as usize]));
        source.borrow_mut()[..UNIT as usize].copy_from_slice(&record[..UNIT as usize]);
        let writes = Rc::new(RefCell::new(Vec::new()));
        let decoder = Decoder {
            source,
            output: Rc::new(RefCell::new(vec![0u8; decoded.len()])),
            stream: Method4::default(),
            registers: [0; 0x90 / 4],
            clock: [0; 0x40 / 4],
            jobs: Rc::new(RefCell::new(Vec::new())),
            writes: writes.clone(),
            fault: None,
            shortfall: 0,
        };
        let mut units = StreamUnits {
            record,
            source: Rc::new(RefCell::new(Vec::new())),
            staged: Rc::new(RefCell::new(Vec::new())),
            fail_at: Some(UNIT),
        };
        match stage_record(Session::open(decoder), &mut units, &header, DESTINATION) {
            Err(StageFailure::Failed(ImageError::Media(MediaError::ReadFault))) => {}
            other => panic!("{:?}", other),
        }
        let writes = writes.borrow();
        assert_eq!(
            &writes[writes.len() - 4..],
            &[
                (engine::Register::Engine(engine::EngineRegister::Enable), 0),
                (engine::Register::Engine(engine::EngineRegister::Status), engine::STATUS_CLEAR),
                (engine::Register::Clock(engine::ClockRegister::Enable), 0),
                (engine::Register::Clock(engine::ClockRegister::Gate), 7),
            ]
        );
    }

    fn loader(destination: u32, length: u32) -> Segment {
        Segment { destination, length, bss: 0, bss_length: 0, source: 0x400000, stored_length: length }
    }

    #[test]
    fn staging_reserves_every_staged_unit() {
        let header = CompressedHeader { decoded_length: 0x282800, record_length: 0x146567 };
        let table = stock_table_range();
        let companion = loader(0xd0000, 0x40000);
        assert!(matches!(
            staging_ranges(loader(0x02e6_0000, 0x20000), companion, segment(), header, table),
            Err(ImageError::Overlap)
        ));
        assert!(staging_ranges(loader(0x02fa_0000, 0x20000), companion, segment(), header, table).is_ok());
        let mut staged_bss = segment();
        staged_bss.bss = 0x02e4_0000;
        assert!(matches!(
            staging_ranges(loader(0x400000, 0x20000), companion, staged_bss, header, table),
            Err(ImageError::Overlap)
        ));
        staged_bss.bss = 0x02e6_0000;
        assert!(matches!(
            staging_ranges(loader(0x400000, 0x20000), companion, staged_bss, header, table),
            Err(ImageError::Overlap)
        ));
    }
}
