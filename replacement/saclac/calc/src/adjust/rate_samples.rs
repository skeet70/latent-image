//! The rate sample recorder the shot sets up, records columns into and scans for extremes. Its
//! state and buffers stay in stock memory: calc's rate functions read them.

use xm1_saclac_interface::calc::SharedAddress;

use crate::exposure::ports::StockMemory;
use crate::stock::rate_samples::{self as layout, Array};

/// A request the recorder refuses (stock's 0x84).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Refused;

/// What the shot names when it sets the recorder up.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RecorderSetup {
    pub buffer: Option<SharedAddress>,
    pub results: Option<SharedAddress>,
    /// The seven configuration tables the recorder keeps, in `layout::TABLES` order.
    pub tables: [Option<SharedAddress>; 7],
}

const fn offset(base: SharedAddress, bytes: u32) -> SharedAddress { SharedAddress(base.0.wrapping_add(bytes)) }

fn copy(memory: &mut impl StockMemory, to: SharedAddress, from: SharedAddress, bytes: u32) {
    for at in (0..bytes).step_by(4) {
        let word = memory.read_u32(offset(from, at));
        memory.write_u32(offset(to, at), word);
    }
}

/// Fills `bytes` from `at` with `value`, by words where aligned.
fn fill(memory: &mut impl StockMemory, at: SharedAddress, value: u8, bytes: u32) {
    let mut done = 0;
    while done < bytes {
        let here = offset(at, done);
        if here.0 % 4 == 0 && bytes - done >= 4 {
            memory.write_u32(here, u32::from_ne_bytes([value; 4]));
            done += 4;
        } else {
            memory.write_u8(here, value);
            done += 1;
        }
    }
}

fn array(memory: &mut impl StockMemory, which: Array) -> SharedAddress { SharedAddress(memory.read_u32(layout::array_word(which))) }

fn mark_columns(memory: &mut impl StockMemory, base: SharedAddress, start: u32, columns: u32) {
    for column in 0..columns {
        memory.write_u32(offset(base, start + layout::COLUMN * column), layout::UNFILLED);
    }
}

/// Copies the configuration tables, points the state at the buffer's arrays, marks every column
/// unfilled and sets the results area to its starting values (0xf8cc9c).
pub fn set_up(memory: &mut impl StockMemory, setup: &RecorderSetup) -> Result<(), Refused> {
    let (Some(buffer), Some(results)) = (setup.buffer, setup.results) else { return Err(Refused) };
    let mut tables = [SharedAddress(0); 7];
    for (table, named) in tables.iter_mut().zip(setup.tables) {
        *table = named.ok_or(Refused)?;
    }
    memory.write_u32(layout::MARK, 0);
    for ((to, bytes), from) in layout::TABLES.into_iter().zip(tables) {
        copy(memory, to, from, bytes);
    }
    fill(memory, layout::CLEARED.0, 0, layout::CLEARED.1);
    memory.write_u8(layout::RECORDED, 0);
    memory.write_u8(layout::FEWEST, 0);
    memory.write_u32(layout::AT_130, 3);
    memory.write_u8(layout::AT_06, 0);
    memory.write_u32(layout::BUFFER, buffer.0);
    let results = SharedAddress(results.0.wrapping_add(3) & !3);
    memory.write_u32(layout::RESULTS, results.0);
    for (word, at) in layout::ARRAYS {
        memory.write_u32(word, buffer.0.wrapping_add(at));
    }
    mark_unfilled(memory, buffer);
    for record in 0..layout::RATE_RECORD_COUNT {
        for (index, word) in layout::DEFAULT_RATE_RECORD.into_iter().enumerate() {
            memory.write_u32(offset(results, layout::RATE_RECORDS + 0x18 * record + 4 * index as u32), word);
        }
    }
    for (at, value, bytes) in layout::RESULT_FILLS[..8].iter().copied() {
        fill(memory, offset(results, at), value, bytes);
    }
    let (at, bytes) = layout::BUFFER_CLEARED[0];
    fill(memory, offset(buffer, at), 0, bytes);
    let cleared = array(memory, Array::Cleared);
    fill(memory, cleared, 0, 0xa04);
    for band in 0..3 {
        for (index, value) in layout::BAND_PATTERN.into_iter().enumerate() {
            memory.write_u8(offset(results, layout::BAND_BYTES + 4 * band + index as u32), value);
        }
        for word in 0..4 {
            memory.write_u32(offset(results, layout::BAND_WORDS + 0x10 * band + 4 * word), 0);
        }
    }
    let (at, value, bytes) = layout::RESULT_FILLS[8];
    fill(memory, offset(results, at), value, bytes);
    let (at, bytes) = layout::BUFFER_CLEARED[1];
    fill(memory, offset(buffer, at), 0, bytes);
    let flags: [u8; layout::CHANNEL_FLAGS] = core::array::from_fn(|index| memory.read_u8(offset(layout::CHANNELS, index as u32)));
    let first_flagged = flags.iter().position(|flag| flag & 4 != 0).unwrap_or(layout::CHANNEL_FLAGS);
    memory.write_u8(layout::FIRST_GROUP, (first_flagged / 8) as u8);
    memory.write_u8(layout::LEADING, leading_channels(&flags));
    Ok(())
}

/// Channels with bit 7 counted up to the first that also carries bit 2, at most nine.
fn leading_channels(flags: &[u8]) -> u8 {
    let mut count = 0u8;
    for flag in flags {
        if count <= 8 && flag & 0x80 != 0 {
            if flag & 4 != 0 {
                break;
            }
            count += 1;
        }
    }
    count
}

fn mark_unfilled(memory: &mut impl StockMemory, buffer: SharedAddress) {
    let primary = array(memory, Array::Primary);
    for channel in 0..2 {
        mark_columns(memory, primary, layout::PRIMARY_CHANNEL * channel, 49);
    }
    let wide = array(memory, Array::Wide);
    for column in 0..10 {
        memory.write_u32(offset(wide, layout::COLUMN * column), layout::UNFILLED);
        memory.write_u32(offset(wide, layout::COLUMN * column + layout::PAIR_CHANNEL), layout::UNFILLED);
    }
    for channel in 0..2 {
        for group in 0..2 {
            for column in 0..5 {
                let at = layout::PAIR_CHANNEL * channel + layout::PAIR_GROUP * group + layout::COLUMN * column;
                for pair in [Array::PairFirst, Array::PairSecond] {
                    let base = array(memory, pair);
                    memory.write_u32(offset(base, at), layout::UNFILLED);
                }
            }
        }
    }
    let ring = array(memory, Array::Ring);
    mark_columns(memory, ring, 0, 10);
    let grid = array(memory, Array::Grid);
    for row in 0..3 {
        mark_columns(memory, grid, layout::GRID_ROW * row, 4);
    }
    for at in layout::BUFFER_MARKS {
        memory.write_u8(offset(buffer, at), layout::UNFILLED_BYTE);
    }
    for channel in 0..2 {
        let half = array(memory, Array::HalfFirst);
        for column in 0..9 {
            memory.write_u16(offset(half, layout::HALF_FIRST_CHANNEL * channel + layout::HALF_COLUMN * column), layout::UNFILLED_HALF);
        }
    }
    for channel in 0..2 {
        let half = array(memory, Array::HalfSecond);
        mark_columns(memory, half, layout::HALF_SECOND_CHANNEL * channel, 9);
    }
    let third = array(memory, Array::HalfThird);
    for column in 0..9 {
        memory.write_u16(offset(third, layout::HALF_COLUMN * column), layout::UNFILLED_HALF);
    }
    let fourth = array(memory, Array::HalfFourth);
    memory.write_u16(fourth, layout::UNFILLED_HALF);
    for column in 0..4 {
        for strip in [Array::StripFirst, Array::StripSecond] {
            let base = array(memory, strip);
            memory.write_u32(offset(base, layout::COLUMN * column), layout::UNFILLED);
        }
    }
    let records = array(memory, Array::Records);
    mark_columns(memory, records, layout::COLUMN, 7);
    mark_columns(memory, records, layout::RECORD_EXTRAS, 10);
    memory.write_u32(offset(records, layout::RECORD_MARKER), layout::UNFILLED);
    for channel in 0..2 {
        let tracks = array(memory, Array::Tracks);
        for column in 0..14 {
            memory.write_u32(offset(tracks, layout::TRACK_CHANNEL * channel + layout::COLUMN * column), layout::UNFILLED);
            memory.write_u8(offset(buffer, layout::TRACK_FLAGS + 0x10 * channel + column), 0);
        }
        memory.write_u8(offset(buffer, layout::TRACK_END + 0x10 * channel), layout::UNFILLED_BYTE);
    }
    memory.write_u32(offset(buffer, layout::BUFFER_WORD), 0);
}

/// Marks a recording made (0xf8d754).
pub fn mark_recording(memory: &mut impl StockMemory) { memory.write_u32(layout::MARK, 1); }

/// Columns recorded so far (0xf8a780, 0xf8d778).
pub fn recorded(memory: &mut impl StockMemory) -> u8 { memory.read_u8(layout::RECORDED) }

/// Writes one column of samples: a sample per flagged channel of the first seven, the extra
/// samples after them, and the eighth sample again when the marker word is 1 (0xf8d7b8). Samples
/// past the end of `samples` read 0.
pub fn record_column(memory: &mut impl StockMemory, samples: &[i16], column: u8) -> Result<(), Refused> {
    if recorded(memory) <= column {
        return Err(Refused);
    }
    let sample = |index: usize| i32::from(samples.get(index).copied().unwrap_or(0)) as u32;
    let records = array(memory, Array::Records);
    let at = 4 * u32::from(column);
    let extras = u32::from(memory.read_u8(layout::EXTRA_SAMPLES));
    let marker = memory.read_u32(layout::MARKER);
    for channel in 0..7 {
        if memory.read_u8(offset(layout::CHANNELS, layout::CHANNEL_STRIDE * channel)) & 1 != 0 {
            memory.write_u32(offset(records, layout::COLUMN * (channel + 1) + at), sample(channel as usize));
        }
    }
    for extra in 0..extras {
        memory.write_u32(offset(records, layout::RECORD_EXTRAS + layout::COLUMN * extra + at), sample(7 + extra as usize));
    }
    if marker == 1 {
        memory.write_u32(offset(records, layout::RECORD_MARKER + at), sample(7));
    }
    Ok(())
}

/// The largest of `count` words from `at`, first index on ties (0xf8ca48).
fn maximum(memory: &mut impl StockMemory, at: SharedAddress, count: u8) -> (u8, u32) {
    (0..count).fold((0, 0), |(index, best), sample| {
        let value = memory.read_u32(offset(at, 4 * u32::from(sample)));
        if best < value { (sample, value) } else { (index, best) }
    })
}

/// The smallest, likewise (0xf8ca94).
fn minimum(memory: &mut impl StockMemory, at: SharedAddress, count: u8) -> (u8, u32) {
    (0..count).fold((0, u32::MAX), |(index, best), sample| {
        let value = memory.read_u32(offset(at, 4 * u32::from(sample)));
        if best > value { (sample, value) } else { (index, best) }
    })
}

/// Over the first `count` samples of every column whose threshold bytes allow it, keeps each
/// column's maximum and minimum with their indices, then records `count` as the columns
/// recorded (0xf8d8c8).
pub fn scan_extremes(memory: &mut impl StockMemory, count: u8) -> Result<(), Refused> {
    if count == 0 || count > recorded(memory) {
        return Err(Refused);
    }
    let results = SharedAddress(memory.read_u32(layout::RESULTS));
    for scan in layout::EXTREMES {
        let [(outer_stride, outers), (middle_stride, middles), (column_stride, columns)] = scan.levels;
        let mut slot = 0;
        for outer in 0..outers {
            for middle in 0..middles {
                for column in 0..columns {
                    let at = outer_stride * outer + middle_stride * middle + column_stride * column;
                    for (bytes, values, extreme) in [(scan.maxima.0, scan.maxima.1, true), (scan.minima.0, scan.minima.1, false)] {
                        let threshold = offset(results, bytes + slot);
                        if memory.read_u8(threshold) < count {
                            continue;
                        }
                        let base = array(memory, scan.array);
                        let (index, value) = if extreme { maximum(memory, offset(base, at), count) } else { minimum(memory, offset(base, at), count) };
                        memory.write_u8(threshold, index);
                        memory.write_u32(offset(results, values + 4 * slot), value);
                    }
                    slot += 1;
                }
            }
        }
    }
    memory.write_u8(layout::RECORDED, count);
    if memory.read_u8(layout::FEWEST) > count {
        memory.write_u8(layout::FEWEST, count);
    }
    Ok(())
}
