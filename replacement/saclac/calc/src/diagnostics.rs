//! The library's diagnostics: the adjustment layer's joint tables, the correction with its
//! settings and the sample history it writes into the client's dump frame, the sections it clears
//! there, and the rate status on the debug overlay.

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::exposure::Correction;

use crate::adjust::motion::search::signed_byte;
use crate::exposure::ports::{Properties, StockMemory};
use crate::exposure::settings;
use crate::scene_recognition::ports::Overlay;
use crate::stock::diagnostics::{self as stock, Term, Width};
use crate::stock::property::ADJUSTMENT_FLAGS;
use crate::stock::shot_dump::at;

/// Image 4's cache maintenance (0x740e74).
pub trait CacheMaintenance {
    /// Invalidates the data cache lines covering `bytes` bytes from `at`.
    fn invalidate(&mut self, at: SharedAddress, bytes: usize);
}

fn dump_enabled(properties: &mut impl Properties) -> bool { properties.property(ADJUSTMENT_FLAGS) & stock::DUMP_ENABLED != 0 }

fn widened(memory: &mut impl StockMemory, at: SharedAddress, width: Width) -> u32 {
    match width {
        Width::Byte => u32::from(memory.read_u8(at)),
        Width::SignedByte => memory.read_u8(at) as i8 as u32,
        Width::Half => u32::from(memory.read_u16(at)),
        Width::SignedHalf => memory.read_u16(at) as i16 as u32,
        Width::Word => memory.read_u32(at),
    }
}

/// Writes each field from `source` as a word from `out`, returning the next word's address.
fn export_fields(memory: &mut impl StockMemory, source: SharedAddress, fields: impl Iterator<Item = (u32, Width)>, mut out: SharedAddress) -> SharedAddress {
    for (offset, width) in fields {
        let value = widened(memory, at(source, offset), width);
        memory.write_u32(out, value);
        out = at(out, 4);
    }
    out
}

/// Copies the adjustment layer's two joint mapping blocks into the dump frame, every field
/// widened to a word between the frame's marks (0xf4ed28).
pub fn export_joint_tables(memory: &mut impl StockMemory, frame: SharedAddress) {
    memory.fill(at(frame, stock::JOINT_EXPORT), stock::JOINT_EXPORT_BYTES, 0);
    memory.write_u32(at(frame, stock::JOINT_EXPORT), stock::JOINT_MARK);
    let mut out = at(frame, stock::JOINT_EXPORT + 4);
    for block in 0..2 {
        let source = at(stock::JOINT_TABLES, block * stock::JOINT_BLOCK_BYTES);
        let bytes = |from: u32, count: u32| (0..count).map(move |index| (from + index, Width::Byte));
        out = export_fields(memory, source, bytes(stock::JOINT_SCALARS, stock::JOINT_SCALAR_COUNT), out);
        let mappings = (0..stock::JOINT_ENTRIES).flat_map(|entry| {
            let base = stock::JOINT_MAPPINGS + entry * stock::JOINT_MAPPING_BYTES;
            stock::JOINT_MAPPING_FIELDS.into_iter().map(move |(offset, width)| (base + offset, width))
        });
        out = export_fields(memory, source, mappings, out);
        out = export_fields(memory, source, bytes(stock::JOINT_TRIPLES, 3 * stock::JOINT_ENTRIES), out);
        out = export_fields(memory, source, bytes(stock::JOINT_EXTRAS, stock::JOINT_EXTRA_COUNT), out);
        let words = (0..stock::JOINT_WORD_COUNT).map(|index| (stock::JOINT_WORDS + 4 * index, Width::Word));
        out = export_fields(memory, source, words, out);
    }
    memory.write_u32(at(frame, stock::JOINT_END), stock::JOINT_END_MARK);
}

/// Zeroes the dump frame's selection sections of a primary exposure or of another one (0xf6035c).
pub fn clear_selection(ports: &mut (impl Properties + StockMemory), frame: Option<SharedAddress>, primary: bool) {
    let Some(frame) = frame else { return };
    if !dump_enabled(ports) {
        return;
    }
    let sections: &[(u32, usize)] = if primary { &stock::PRIMARY_SELECTION } else { &stock::SECONDARY_SELECTION };
    for &(offset, bytes) in sections {
        ports.fill(at(frame, offset), bytes, 0);
    }
}

/// What the correction export takes besides the correction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CorrectionExport {
    /// The exposure's first treatment (0xfa66d4); with property 0x657d set the export carries
    /// the correction's rate.
    pub first_treatment: bool,
    /// The low half of the snapshot's subject count; at most ten subject levels are exported.
    pub subjects: u16,
    /// Clears the sample records instead of exporting the history.
    pub clear_samples: bool,
}

fn term(properties: &mut impl Properties, correction: &Correction, base: i32, term: Term) -> u32 {
    match term {
        Term::Signed(half) => correction.half(half) as i16 as u32,
        Term::Unsigned(half) => u32::from(correction.half(half)),
        Term::Relative(half) => base.wrapping_add(i32::from(correction.half(half) as i16)) as u32,
        Term::Word(half) => correction.word(half),
        Term::Level(id) => settings::level(properties, id) as u32,
        Term::SignedByte(id) => signed_byte(properties, id) as u32,
        Term::Setting(id) => properties.property(id),
    }
}

/// Exports the sample history and the correction with the settings it was made under into the
/// dump frame (0xf60740).
pub fn export_correction(ports: &mut (impl Properties + StockMemory + CacheMaintenance), frame: Option<SharedAddress>, correction: &Correction, export: &CorrectionExport) {
    let Some(frame) = frame else { return };
    export_samples(ports, frame, export.clear_samples);
    let (base_at, base_half) = stock::CORRECTION_BASE;
    let base_level = correction.half(base_half) as i16;
    ports.write_u32(at(frame, base_at), base_level as u32);
    let base = i32::from((stock::RELATIVE_BASE as i16).wrapping_sub(base_level));
    for (offset, item) in stock::CORRECTION_TERMS {
        let value = term(ports, correction, base, item);
        ports.write_u32(at(frame, offset), value);
    }
    let rated = export.first_treatment && ports.property(crate::stock::property::CORRECTION_RATE) != 0;
    let (setting, rate) = if rated {
        (stock::RATED_SETTING, correction.half(stock::RATE_HALF) as i16 as u32)
    } else {
        (stock::UNRATED_SETTING, stock::NO_RATE)
    };
    let setting = ports.property(setting);
    ports.write_u32(at(frame, stock::RATE_SETTING_AT), setting);
    ports.write_u32(at(frame, stock::RATE_AT), rate);
    for (offset, item) in stock::CORRECTION_TAIL {
        let value = term(ports, correction, base, item);
        ports.write_u32(at(frame, offset), value);
    }
    let [low, high] = stock::CORRECTION_SPAN.map(|(_, half)| base.wrapping_add(i32::from(correction.half(half) as i16)));
    for ((offset, _), value) in stock::CORRECTION_SPAN.into_iter().zip([low, high]) {
        ports.write_u32(at(frame, offset), value as u32);
    }
    ports.write_u32(at(frame, stock::CORRECTION_SPAN_WIDTH), high.wrapping_sub(low) as u32);
    for (offset, item) in stock::CORRECTION_SETTINGS {
        let value = term(ports, correction, base, item);
        ports.write_u32(at(frame, offset), value);
    }
    let subjects = usize::from(export.subjects).min(stock::SUBJECT_LEVEL_COUNT);
    for index in 0..stock::SUBJECT_LEVEL_COUNT {
        let level = if index < subjects { term(ports, correction, base, Term::Relative(stock::SUBJECT_LEVEL_HALVES + index)) } else { 0 };
        ports.write_u32(at(frame, stock::SUBJECT_LEVELS + 4 * index as u32), level);
    }
}

/// The adjustment layer's sample history in the dump frame's two sample areas (0xf6049c as
/// 0xf60740 enters it).
fn export_samples(ports: &mut (impl Properties + StockMemory + CacheMaintenance), frame: SharedAddress, clear: bool) {
    if !dump_enabled(ports) {
        return;
    }
    if clear {
        for area in stock::SAMPLE_AREAS {
            ports.fill(at(frame, area), stock::SAMPLE_AREA_BYTES, 0);
        }
        return;
    }
    ports.invalidate(stock::SAMPLE_HISTORY, stock::SAMPLE_HISTORY_BYTES);
    let mark = ports.read_u8(stock::SAMPLE_HISTORY);
    ports.write_u32(at(frame, stock::SAMPLE_MARK), u32::from(mark));
    let slots = ports.read_u8(at(stock::SAMPLE_HISTORY, stock::SAMPLE_COUNT)).min(stock::SAMPLE_SLOTS);
    for slot in 0..u32::from(slots) {
        for pass in 0..2 {
            let record = at(frame, stock::SAMPLE_AREAS[pass] + stock::SAMPLE_RECORD_BYTES * slot);
            let half = |table: u32| at(stock::SAMPLE_HISTORY, table + 2 * slot);
            let level = widened(ports, half(stock::SAMPLE_LEVELS[pass]), Width::SignedHalf).wrapping_add(stock::RELATIVE_BASE as u32);
            ports.write_u32(at(record, stock::SAMPLE_LEVEL), level);
            let reference = widened(ports, half(stock::SAMPLE_REFERENCES), Width::SignedHalf).wrapping_add(stock::RELATIVE_BASE as u32);
            ports.write_u32(at(record, stock::SAMPLE_REFERENCE), reference);
            let setting = ports.property(stock::SAMPLE_SETTING);
            ports.write_u32(at(record, stock::SAMPLE_SETTING_AT), setting);
            let sample = at(stock::SAMPLE_HISTORY, stock::SAMPLES[pass] + stock::SAMPLE_BYTES * slot);
            for (out, offset, width) in stock::SAMPLE_FIELDS {
                let value = widened(ports, at(sample, offset), width);
                ports.write_u32(at(record, out), value);
            }
        }
    }
}

/// What the focus measurement reports beside its rate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateStatus {
    Still,
    Active,
    NoFocus,
    Failed,
}

impl RateStatus {
    /// The code the focus measurement reports.
    pub const fn from_code(code: u32) -> Self {
        match code {
            0 => Self::Still,
            1 => Self::Active,
            3 => Self::NoFocus,
            _ => Self::Failed,
        }
    }

    const fn text(self) -> &'static [u8] {
        match self {
            Self::Still => stock::STATUS_STILL,
            Self::Active => stock::STATUS_ACTIVE,
            Self::NoFocus => stock::STATUS_NO_FOCUS,
            Self::Failed => stock::STATUS_FAILED,
        }
    }

    const fn colour(self) -> u8 {
        match self {
            Self::Still => stock::COLOUR_STILL,
            Self::Active => stock::COLOUR_ACTIVE,
            Self::NoFocus | Self::Failed => stock::COLOUR_ALERT,
        }
    }
}

/// The longest rate text: ten digits, the point and one digit.
const RATE_TEXT_BYTES: usize = 12;

/// `tenths` as whole and tenth digits with a point, padded with spaces to six characters.
fn rate_text(tenths: u32, text: &mut [u8; RATE_TEXT_BYTES]) -> usize {
    let mut digits = [0u8; 10];
    let mut whole = tenths / 10;
    let mut count = 0;
    loop {
        digits[count] = b'0' + (whole % 10) as u8;
        count += 1;
        whole /= 10;
        if whole == 0 {
            break;
        }
    }
    for (slot, digit) in text.iter_mut().zip(digits[..count].iter().rev()) {
        *slot = *digit;
    }
    text[count] = b'.';
    text[count + 1] = b'0' + (tenths % 10) as u8;
    let length = count + 2;
    for slot in &mut text[length..stock::RATE_WIDTH.max(length)] {
        *slot = b' ';
    }
    length.max(stock::RATE_WIDTH)
}

/// Shows the rate and its status on the overlay's rate and status rows (0xf521d8).
pub fn draw_rate_status(overlay: &mut impl Overlay, tenths: u32, status: RateStatus, second_column: bool) {
    let mut text = [0; RATE_TEXT_BYTES];
    let length = rate_text(tenths, &mut text);
    let [first, second] = stock::STATUS_COLUMNS;
    let column = if second_column { second } else { first };
    let colour = status.colour();
    overlay.text(&text[..length], column, stock::RATE_ROW, stock::STATUS_FOREGROUND, colour);
    overlay.text(status.text(), column, stock::STATUS_ROW, stock::STATUS_FOREGROUND, colour);
    if !second_column {
        overlay.text(stock::STATUS_BLANK, second, stock::RATE_ROW, stock::STATUS_FOREGROUND, stock::COLOUR_BLANK);
        overlay.text(stock::STATUS_BLANK, second, stock::STATUS_ROW, stock::STATUS_FOREGROUND, stock::COLOUR_BLANK);
    }
}
