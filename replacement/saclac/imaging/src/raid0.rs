#![forbid(unsafe_code)]

//! RAID0, the ISP unit whose registers the image pipeline programs for capture and live view.

use crate::driver::{admit, Client, Isp, Operation, Refusal, Register, Unit};

const UNIT: Unit = Unit::Raid0;

const CONTROL: u16 = 0x00;
const ORIGIN: u16 = 0x04;
const ORIGIN_DEPTH: u16 = 0x08;
const FORMAT: u16 = 0x0c;
const EXTENT: u16 = 0x10;
const EXTENT_DEPTH: u16 = 0x14;
const DIVISION: u16 = 0x18;
const GATE: u16 = 0x1c;
const PAIRS: u16 = 0x20;
const AREA: u16 = 0x40;
const BLOCK_BASE: u16 = 0x48;
const OUTPUT: u16 = 0x60;
const WINDOW: u16 = 0x64;
const MODE: u16 = 0x74;
const SECOND_WINDOW: u16 = 0x78;
const SECOND_EXTENT: u16 = 0x7c;
const SECOND_SPAN: u16 = 0x80;
const SECOND_STEPS: u16 = 0x84;
const ROTATED: [u16; 2] = [0x88, 0xc0];
const POINTS: u16 = 0x100;
const CURVES: u16 = 0x1100;
const FIRST_TABLES: u16 = 0x1c00;
const SECOND_TABLES: u16 = 0x2800;

const COORDINATE: u16 = 0x2000;
const SPAN: u16 = 0x800;
const STEP: u8 = 0x40;
const LONG: u16 = 0x4000;

fn write<I: Isp>(isp: &mut I, offset: u16, value: u32) { isp.write(Register::Raid0(offset), value) }

fn read<I: Isp>(isp: &mut I, offset: u16) -> u32 { isp.read(Register::Raid0(offset)) }

fn pair(high: u16, low: u16, bits: u32) -> u32 {
    let mask = (1 << bits) - 1;
    (u32::from(high) & mask) << 16 | u32::from(low) & mask
}

fn table<I: Isp>(isp: &mut I, start: u16, words: impl IntoIterator<Item = u32>) {
    for (index, word) in (0u16..).zip(words) {
        write(isp, start + 4 * index, word);
    }
}

/// One of the control register's four two-bit stage selectors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage { First, Second, Third, Fourth }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageMode { Off, On, Alternate }

impl Stage {
    const fn shift(self) -> u32 {
        match self {
            Self::First => 0,
            Self::Second => 2,
            Self::Third => 4,
            Self::Fourth => 8,
        }
    }

    pub(crate) const fn operation(self) -> Operation {
        match self {
            Self::First => Operation(4),
            Self::Second => Operation(5),
            Self::Third => Operation(6),
            Self::Fourth => Operation(7),
        }
    }
}

pub fn set_stage<I: Isp>(isp: &mut I, stage: Stage, mode: StageMode) {
    isp.note(UNIT, stage.operation());
    let field = match mode {
        StageMode::Off => 0,
        StageMode::On => 1,
        StageMode::Alternate => 2,
    };
    let control = read(isp, CONTROL) & !(3 << stage.shift()) | field << stage.shift();
    write(isp, CONTROL, control);
}

/// The control register's bits 12 and 13.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Gates { pub first: bool, pub second: bool }

pub(crate) const GATES: Operation = Operation(8);

pub fn set_gates<I: Isp>(isp: &mut I, gates: Gates) {
    isp.note(UNIT, GATES);
    let control = read(isp, CONTROL) & !(3 << 12) | u32::from(gates.first) << 12 | u32::from(gates.second) << 13;
    write(isp, CONTROL, control);
}

pub fn gates<I: Isp>(isp: &mut I) -> Gates {
    let control = read(isp, CONTROL);
    isp.note(UNIT, GATES);
    Gates { first: control >> 12 & 1 == 1, second: control >> 13 & 1 == 1 }
}

/// The control register's bits 16..=18: a four-way source and its enable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Path { pub source: PathSource, pub enabled: bool }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathSource { First, Second, Third, Fourth }

pub(crate) const PATH: Operation = Operation(9);

pub fn set_path<I: Isp>(isp: &mut I, path: Path) {
    isp.note(UNIT, PATH);
    let source = match path.source {
        PathSource::First => 0,
        PathSource::Second => 1,
        PathSource::Third => 2,
        PathSource::Fourth => 3,
    };
    let control = read(isp, CONTROL) & !(7 << 16) | source << 16 | u32::from(path.enabled) << 18;
    write(isp, CONTROL, control);
}

pub fn path<I: Isp>(isp: &mut I) -> Path {
    let control = read(isp, CONTROL);
    isp.note(UNIT, PATH);
    let source = match control >> 16 & 3 {
        0 => PathSource::First,
        1 => PathSource::Second,
        2 => PathSource::Third,
        _ => PathSource::Fourth,
    };
    Path { source, enabled: control >> 18 & 1 == 1 }
}

/// Three coordinates below 0x2000: the first two share one register, the third the next one's
/// high half.
pub fn set_origin<I: Isp>(isp: &mut I, client: Client, origin: [u16; 3]) -> Result<(), Refusal> {
    admit(isp, UNIT, client, Operation(0x0c), &origin.map(|value| value < COORDINATE))?;
    write(isp, ORIGIN, pair(origin[0], origin[1], 13));
    write(isp, ORIGIN_DEPTH, u32::from(origin[2]) << 16);
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Format {
    /// 0..=5.
    pub first: u8,
    /// 0..=11.
    pub second: u8,
    /// 0..=15.
    pub third: u16,
    /// 0..=15.
    pub fourth: u16,
}

pub fn set_format<I: Isp>(isp: &mut I, client: Client, format: Format) -> Result<(), Refusal> {
    let checks = [format.first <= 5, format.second <= 11, format.third <= 15, format.fourth <= 15];
    admit(isp, UNIT, client, Operation(0x0e), &checks)?;
    let word = u32::from(format.first) | u32::from(format.second) << 3 | u32::from(format.third) << 16 | u32::from(format.fourth) << 20;
    write(isp, FORMAT, word);
    Ok(())
}

/// Four coordinates below 0x2000, two per register, the first of each pair in the high half.
pub fn set_extent<I: Isp>(isp: &mut I, client: Client, extent: [u16; 4]) -> Result<(), Refusal> {
    admit(isp, UNIT, client, Operation(0x10), &extent.map(|value| value < COORDINATE))?;
    write(isp, EXTENT, pair(extent[0], extent[1], 13));
    write(isp, EXTENT_DEPTH, pair(extent[2], extent[3], 13));
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DivisionMode { First, Second, Third, Fourth }

/// A division mode and a count of 1..=15.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Division { pub mode: DivisionMode, pub count: u16 }

pub(crate) const SET_DIVISION: Operation = Operation(0x12);

pub fn set_division<I: Isp>(isp: &mut I, client: Client, division: Division) -> Result<(), Refusal> {
    admit(isp, UNIT, client, SET_DIVISION, &[(1..=15).contains(&division.count)])?;
    let mode = match division.mode {
        DivisionMode::First => 0,
        DivisionMode::Second => 1,
        DivisionMode::Third => 2,
        DivisionMode::Fourth => 3,
    };
    write(isp, DIVISION, mode << 4 | u32::from(division.count));
    Ok(())
}

pub(crate) const SET_GATE: Operation = Operation(0x14);

pub fn set_gate<I: Isp>(isp: &mut I, enabled: bool) {
    isp.note(UNIT, SET_GATE);
    write(isp, GATE, u32::from(enabled));
}

pub fn gate<I: Isp>(isp: &mut I) -> bool {
    isp.note(UNIT, Operation(0x15));
    read(isp, GATE) & 1 == 1
}

/// Added to each point's low and high 13-bit coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Shift { pub low: u16, pub high: u16 }

pub(crate) const POINT_LIMIT: usize = 1024;
const EMPTY_POINT: u32 = 0x9fff_1fff;
const LAST_POINT: u32 = 1 << 31;

fn shifted(entry: u32, shift: Shift) -> u32 {
    let low = (entry & 0x1fff).wrapping_add(u32::from(shift.low)) & 0x1fff;
    let high = (entry >> 16 & 0x1fff).wrapping_add(u32::from(shift.high)) & 0x1fff;
    high << 16 | entry & 0xc000 | low
}

pub(crate) const POINTS_LOAD: Operation = Operation(0x16);

/// Loads up to 1024 points (13-bit coordinates in each half, flags at 14..=15) and marks the last
/// with bit 31; an empty list writes the empty entry to the first two slots.
pub fn set_points<I: Isp>(isp: &mut I, client: Client, points: &[u32], shift: Shift) -> Result<(), Refusal> {
    admit(isp, UNIT, client, POINTS_LOAD, &[points.len() <= POINT_LIMIT])?;
    if points.is_empty() {
        write(isp, POINTS, EMPTY_POINT);
        write(isp, POINTS + 4, EMPTY_POINT);
        return Ok(());
    }
    table(isp, POINTS, points.iter().map(|point| shifted(*point, shift)));
    let last = POINTS + 4 * (points.len() as u16 - 1);
    let marked = read(isp, last) | LAST_POINT;
    write(isp, last, marked);
    Ok(())
}

pub(crate) const PAIR_LIMIT: usize = 16;
const UNUSED: u32 = 0x1fff;

pub(crate) const PAIRS_LOAD: Operation = Operation(0x17);

/// Up to sixteen entries two per register, flags at 14..=15 (low) and 30..=31 (high); slots past
/// the list read 0x1fff. An empty list writes only the first register.
pub fn set_pairs<I: Isp>(isp: &mut I, client: Client, entries: &[u32], shift: u16) -> Result<(), Refusal> {
    admit(isp, UNIT, client, PAIRS_LOAD, &[entries.len() <= PAIR_LIMIT])?;
    if entries.is_empty() {
        write(isp, PAIRS, UNUSED << 16 | UNUSED);
        return Ok(());
    }
    let field = |entry: Option<&u32>| match entry {
        Some(entry) => (entry & 0x1fff).wrapping_add(u32::from(shift)) & 0x1fff | (entry >> 14 & 3) << 14,
        None => UNUSED,
    };
    for index in 0..PAIR_LIMIT / 2 {
        let word = field(entries.get(2 * index)) | field(entries.get(2 * index + 1)) << 16;
        write(isp, PAIRS + 4 * index as u16, word);
    }
    Ok(())
}

/// Four coordinates below 0x2000, two per register, the first of each pair in the low half.
pub fn set_area<I: Isp>(isp: &mut I, client: Client, area: [u16; 4]) -> Result<(), Refusal> {
    admit(isp, UNIT, client, Operation(0x18), &area.map(|value| value < COORDINATE))?;
    write(isp, AREA, pair(area[1], area[0], 13));
    write(isp, AREA + 4, pair(area[3], area[2], 13));
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Block {
    /// Below 0x100000.
    pub base: u32,
    /// Below 0x4000.
    pub stride: u16,
    /// Each below 0x2000, programmed two per register, low half first.
    pub corners: [u16; 4],
    /// Each below 0x4000; checked and not programmed.
    pub limits: [u16; 4],
}

pub fn set_block<I: Isp>(isp: &mut I, client: Client, block: Block) -> Result<(), Refusal> {
    let corners = block.corners.map(|value| value < COORDINATE);
    let limits = block.limits.map(|value| value < LONG);
    let checks = [block.base < 0x10_0000, block.stride < LONG, corners[0], corners[1], corners[2], corners[3], limits[0], limits[1], limits[2], limits[3]];
    admit(isp, UNIT, client, Operation(0x1a), &checks)?;
    write(isp, BLOCK_BASE, block.base);
    write(isp, BLOCK_BASE + 4, u32::from(block.stride));
    write(isp, BLOCK_BASE + 8, pair(block.corners[1], block.corners[0], 13));
    write(isp, BLOCK_BASE + 12, pair(block.corners[3], block.corners[2], 13));
    Ok(())
}

pub(crate) const SET_OUTPUT: Operation = Operation(0x1c);

pub fn set_output<I: Isp>(isp: &mut I, enabled: bool) {
    isp.note(UNIT, SET_OUTPUT);
    write(isp, OUTPUT, u32::from(enabled));
}

pub fn output<I: Isp>(isp: &mut I) -> bool {
    isp.note(UNIT, Operation(0x1d));
    read(isp, OUTPUT) & 1 == 1
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Window {
    /// Each below 0x2000; the first is the high half.
    pub position: [u16; 2],
    /// Each below 0x40; the first is the high half.
    pub steps: [u8; 2],
    /// Each below 0x800; the first is the high half.
    pub size: [u16; 2],
    /// Below 0x4000.
    pub length: u16,
}

pub fn set_window<I: Isp>(isp: &mut I, client: Client, window: Window) -> Result<(), Refusal> {
    let checks = [
        window.position[0] < COORDINATE,
        window.position[1] < COORDINATE,
        window.steps[0] < STEP,
        window.steps[1] < STEP,
        window.size[0] < SPAN,
        window.size[1] < SPAN,
        window.length < LONG,
    ];
    admit(isp, UNIT, client, Operation(0x1e), &checks)?;
    write(isp, WINDOW, pair(window.position[0], window.position[1], 13));
    write(isp, WINDOW + 4, pair(window.steps[0].into(), window.steps[1].into(), 6));
    write(isp, WINDOW + 8, pair(window.size[0], window.size[1], 11));
    write(isp, WINDOW + 12, u32::from(window.length));
    Ok(())
}

/// Nine curves of 64 words each, loaded unchanged.
pub fn load_curves<I: Isp>(isp: &mut I, curves: [&[u32; 64]; 9]) {
    isp.note(UNIT, Operation(0x20));
    for (index, curve) in (0u16..).zip(curves) {
        table(isp, CURVES + 0x100 * index, curve.iter().copied());
    }
}

/// The mode register: an enable bit and a three-bit level at 16.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mode { pub enabled: bool, pub level: u8 }

pub(crate) const SET_MODE: Operation = Operation(0x21);

pub fn set_mode<I: Isp>(isp: &mut I, mode: Mode) {
    isp.note(UNIT, SET_MODE);
    write(isp, MODE, u32::from(mode.enabled) | (u32::from(mode.level) & 7) << 16);
}

pub fn mode<I: Isp>(isp: &mut I) -> Mode {
    let word = read(isp, MODE);
    isp.note(UNIT, Operation(0x22));
    Mode { enabled: word & 1 == 1, level: (word >> 16 & 7) as u8 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecondWindow {
    /// Each below 0x2000; the first is the high half.
    pub position: [u16; 2],
    /// Each below 0x40, at bits 24 and 16 of the step register.
    pub steps: [u8; 2],
    /// Below 0x2000, the step register's low bits.
    pub length: u16,
}

/// Programs the window and replaces the whole step register.
pub fn set_second_window<I: Isp>(isp: &mut I, client: Client, window: SecondWindow) -> Result<(), Refusal> {
    let checks = [
        window.position[0] < COORDINATE,
        window.position[1] < COORDINATE,
        window.steps[0] < STEP,
        window.steps[1] < STEP,
        window.length < COORDINATE,
    ];
    admit(isp, UNIT, client, Operation(0x23), &checks)?;
    write(isp, SECOND_WINDOW, pair(window.position[0], window.position[1], 13));
    let steps = u32::from(window.steps[0]) << 24 | u32::from(window.steps[1]) << 16 | u32::from(window.length);
    write(isp, SECOND_STEPS, steps);
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecondExtent {
    /// Each below 0x2000; the first is the high half.
    pub extent: [u16; 2],
    /// Each below 0x800; the first is the high half.
    pub span: [u16; 2],
    /// Each below 0x40, at bits 24 and 16 of the step register.
    pub steps: [u8; 2],
}

/// Programs the extent and span and replaces only the step fields of the step register.
pub fn set_second_extent<I: Isp>(isp: &mut I, client: Client, extent: SecondExtent) -> Result<(), Refusal> {
    let checks = [
        extent.extent[0] < COORDINATE,
        extent.extent[1] < COORDINATE,
        extent.span[0] < SPAN,
        extent.span[1] < SPAN,
        extent.steps[0] < STEP,
        extent.steps[1] < STEP,
    ];
    admit(isp, UNIT, client, Operation(0x5a), &checks)?;
    write(isp, SECOND_EXTENT, pair(extent.extent[0], extent.extent[1], 13));
    write(isp, SECOND_SPAN, pair(extent.span[0], extent.span[1], 11));
    let steps = read(isp, SECOND_STEPS) & 0xc0c0_ffff | u32::from(extent.steps[0]) << 24 | u32::from(extent.steps[1]) << 16;
    write(isp, SECOND_STEPS, steps);
    Ok(())
}

/// Two tables of fourteen entries, each entry's 14-bit halves exchanged.
pub fn load_rotated<I: Isp>(isp: &mut I, tables: [&[u32; 14]; 2]) {
    isp.note(UNIT, Operation(0x25));
    for (start, entries) in ROTATED.into_iter().zip(tables) {
        table(isp, start, entries.iter().map(|entry| (entry & 0x3fff) << 16 | entry >> 16 & 0x3fff));
    }
}

/// Three tables of 256 entries of three 10-bit fields.
pub fn load_first_tables<I: Isp>(isp: &mut I, tables: [&[u32; 256]; 3]) {
    isp.note(UNIT, Operation(0x27));
    for (index, entries) in (0u16..).zip(tables) {
        table(isp, FIRST_TABLES + 0x400 * index, entries.iter().map(|entry| entry & 0x3fff_ffff));
    }
}

/// Three tables of 256 entries; the first two keep three 10-bit fields, the third two.
pub fn load_second_tables<I: Isp>(isp: &mut I, tables: [&[u32; 256]; 3]) {
    isp.note(UNIT, Operation(0x28));
    for (index, entries) in (0u16..).zip(tables) {
        let mask = if index == 2 { 0x000f_ffff } else { 0x3fff_ffff };
        table(isp, SECOND_TABLES + 0x400 * index, entries.iter().map(|entry| entry & mask));
    }
}
