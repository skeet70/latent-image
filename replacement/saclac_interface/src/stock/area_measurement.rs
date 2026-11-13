#![forbid(unsafe_code)]

//! X-M1 1.50 layouts of the secondary area measurement's payloads and of the geometry block
//! 0xf4ca60 reads at 0x16b76bc. Completion payloads start at +0x10 of the receive buffer 0x169d370.

use crate::area_measurement::{AreaEntry, CaptureSelection, Evaluation, EvaluatedList, MeasurementReady, MeasurementRequest, Refinement, TransferRequest};
use crate::calc::{AreaGeometry, SharedAddress};

fn word(bytes: &[u8], offset: usize) -> Option<u32> { Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?)) }

fn half(bytes: &[u8], offset: usize) -> Option<i16> { Some(i16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?)) }

fn put(bytes: &mut [u8], offset: usize, value: &[u8]) { bytes[offset..offset + value.len()].copy_from_slice(value); }

pub const MEASUREMENT_REQUEST_BYTES: usize = 108;

/// The 108 bytes 0xfeab6c posts to imgdc 0x3f46 after its two callbacks (0xf2ad34 for 0x5b,
/// 0xf2ad08 for 0x5c), as 0xf2b014 fills them.
pub fn measurement_request(request: &MeasurementRequest, areas: SharedAddress) -> [u8; MEASUREMENT_REQUEST_BYTES] {
    let mut bytes = [0; MEASUREMENT_REQUEST_BYTES];
    put(&mut bytes, 0x00, &request.subject_source.to_le_bytes());
    put(&mut bytes, 0x04, &request.subjects_locked.to_le_bytes());
    put(&mut bytes, 0x08, &request.layout[0].to_le_bytes());
    put(&mut bytes, 0x0a, &request.layout[1].to_le_bytes());
    put(&mut bytes, 0x0c, &request.secondary_mode.to_le_bytes());
    put(&mut bytes, 0x10, &request.secondary_option.to_le_bytes());
    put(&mut bytes, 0x14, &request.secondary_latch.to_le_bytes());
    put(&mut bytes, 0x18, &request.profile_class.to_le_bytes());
    put(&mut bytes, 0x1c, &request.capture_offset.to_le_bytes());
    put(&mut bytes, 0x20, &request.frame_words[0].to_le_bytes());
    put(&mut bytes, 0x24, &request.frame_words[1].to_le_bytes());
    put(&mut bytes, 0x2c, &1u32.to_le_bytes());
    put(&mut bytes, 0x30, &1u32.to_le_bytes());
    put(&mut bytes, 0x34, &request.selection.selection.to_le_bytes());
    put(&mut bytes, 0x38, &request.slot_offset.to_le_bytes());
    put(&mut bytes, 0x3a, &request.selection.first.to_le_bytes());
    put(&mut bytes, 0x3c, &request.selection.second.to_le_bytes());
    put(&mut bytes, 0x40, &request.selection.auxiliary.to_le_bytes());
    put(&mut bytes, 0x44, &request.slot_rate.to_le_bytes());
    put(&mut bytes, 0x48, &0x50u16.to_le_bytes());
    put(&mut bytes, 0x4a, &0x10u16.to_le_bytes());
    put(&mut bytes, 0x4c, &request.subject_area.to_le_bytes());
    put(&mut bytes, 0x50, &u32::from(request.lists[0]).to_le_bytes());
    put(&mut bytes, 0x54, &u32::from(request.lists[1]).to_le_bytes());
    put(&mut bytes, 0x58, &request.list_details[0].to_le_bytes());
    put(&mut bytes, 0x5c, &request.list_details[1].to_le_bytes());
    put(&mut bytes, 0x60, &request.area_count.to_le_bytes());
    put(&mut bytes, 0x64, &areas.0.to_le_bytes());
    put(&mut bytes, 0x68, &request.shift.to_le_bytes());
    bytes
}

/// 0xf2ad08 posts the word it is given, then the source byte; 0xf2be70 reads the byte at +4.
pub fn measurement_ready(payload: &[u8]) -> Option<MeasurementReady> { Some(MeasurementReady { source: *payload.get(4)? }) }

/// 0xf2acd0 posts 84 bytes; 0xf2c4c8 copies them whole.
pub fn evaluation(payload: &[u8]) -> Option<Evaluation> { Some(Evaluation(payload.get(..Evaluation::BYTES)?.try_into().ok()?)) }

/// 0xf2ac98 posts 84 bytes; 0xf2c05c reads the halfword at +0x16, 24 bytes at +0x20, the word at
/// +0x44 and the signed halfword at +0xc.
pub fn refinement(payload: &[u8]) -> Option<Refinement> {
    Some(Refinement {
        fifth: half(payload, 0x16)?,
        block: payload.get(0x20..0x38)?.try_into().ok()?,
        slot_word: word(payload, 0x44)?,
        slot_offset: half(payload, 0xc)?,
    })
}

/// The four bytes 0xfeabb8 posts to imgdc 0x3f47 after its callback (0xf2ac70 for 0x5d).
pub fn transfer_request(request: &TransferRequest) -> [u8; 4] { request.transfer.to_le_bytes() }

/// The 12 bytes 0xf4c958 writes: word +0, halfwords +4 and +6, word +8 (0xf2b0e0..0xf2b104).
pub fn capture_selection(bytes: &[u8; 12]) -> CaptureSelection {
    CaptureSelection {
        selection: word(bytes, 0).unwrap_or(0),
        first: half(bytes, 4).unwrap_or(0),
        second: half(bytes, 6).unwrap_or(0) as u16,
        auxiliary: word(bytes, 8).unwrap_or(0),
    }
}

pub const AREA_GEOMETRY_BYTES: usize = 0x1a8;
/// Where the measured areas sit in the geometry block; imgdc reads them through the request.
pub const AREAS_OFFSET: usize = 0x40;

/// The geometry block as 0xf2b014..0xf2bd2c writes it and 0xf4ca60 reads it. Each measured area
/// has a pair of words 0xf4d144 sets to all ones at +0x10. Bytes 0xf4ca60 does not read stay zero.
pub fn area_geometry(geometry: &AreaGeometry) -> [u8; AREA_GEOMETRY_BYTES] {
    let mut bytes = [0; AREA_GEOMETRY_BYTES];
    put(&mut bytes, 0x00, &(if geometry.active { 2u32 } else { 0 }).to_le_bytes());
    let list: u32 = match geometry.list {
        None => 0,
        Some(EvaluatedList::First) => 1,
        Some(EvaluatedList::Second) => 2,
    };
    put(&mut bytes, 0x04, &list.to_le_bytes());
    put(&mut bytes, 0x0a, &geometry.settings);
    bytes[0x0d] = geometry.count;
    for index in 0..(geometry.area_count as usize).min(3) { put(&mut bytes, 0x10 + 8 * index, &[0xff; 8]); }
    for (index, share) in geometry.shares.iter().enumerate() { put(&mut bytes, 0x28 + 4 * index, &share.to_le_bytes()); }
    put(&mut bytes, 0x34, &geometry.scale.to_le_bytes());
    put(&mut bytes, 0x38, &geometry.list_value.to_le_bytes());
    put(&mut bytes, 0x3c, &geometry.area_count.to_le_bytes());
    for (index, area) in geometry.areas.iter().enumerate() { put(&mut bytes, AREAS_OFFSET + AreaEntry::BYTES * index, &area.0); }
    for (index, accepted) in geometry.accepted.iter().enumerate() { put(&mut bytes, 0x178 + 4 * index, &u32::from(*accepted).to_le_bytes()); }
    put(&mut bytes, 0x184, &geometry.span.to_le_bytes());
    put(&mut bytes, 0x186, &geometry.shift.to_le_bytes());
    put(&mut bytes, 0x198, &geometry.record_value.to_le_bytes());
    put(&mut bytes, 0x1a0, &u32::from(geometry.within_limit).to_le_bytes());
    put(&mut bytes, 0x1a4, &geometry.slot_rate.to_le_bytes());
    bytes
}
