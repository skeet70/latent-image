#![forbid(unsafe_code)]

//! AUTO, the ISP unit's statistics grids and windows. Each operation names the session it was
//! handed; another session is logged and the operation proceeds.

use crate::driver::{check_session, refuse, Client, Event, Fault, Isp, Operation, Refusal, Register, SessionId, Unit};

const UNIT: Unit = Unit::Auto;

const SELECTORS: u16 = 0x00c;
const SCALING: u16 = 0x15c;
const SOURCE: u16 = 0x600;
const SOURCE_ENABLE: u16 = 0x418;
const FIRST_BANK: u16 = 0x10c;
const SECOND_BANK: u16 = 0x208;

fn write<I: Isp>(isp: &mut I, offset: u16, value: u32) { isp.write(Register::Auto(offset), value) }

fn read<I: Isp>(isp: &mut I, offset: u16) -> u32 { isp.read(Register::Auto(offset)) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Caller { pub client: Client, pub session: Option<SessionId> }

fn begin<I: Isp>(isp: &mut I, caller: Caller, operation: Operation) {
    check_session(isp, UNIT, caller.client, caller.session, operation);
}

fn admit<I: Isp>(isp: &mut I, caller: Caller, operation: Operation, checks: &[bool]) -> Result<(), Refusal> {
    if checks.iter().all(|check| *check) { Ok(()) } else { Err(refuse(isp, UNIT, caller.client, operation)) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source { First, Second, Third }

/// The selector register's four fields, kept to 3, 3, 2 and 2 bits, and the source select.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Selectors { pub fields: [u8; 4], pub source: Source, pub source_enabled: bool }

pub fn set_selectors<I: Isp>(isp: &mut I, caller: Caller, selectors: Selectors) {
    begin(isp, caller, Operation(6));
    let [first, second, third, fourth] = selectors.fields.map(u32::from);
    let fields = first & 7 | (second & 7) << 3 | (third & 3) << 8 | (fourth & 3) << 12;
    let word = read(isp, SELECTORS) & 0xffff_ccc0 | fields;
    write(isp, SELECTORS, word);
    let source = match selectors.source {
        Source::First => 0,
        Source::Second => 1,
        Source::Third => 2,
    };
    let word = read(isp, SOURCE) & !3 | source;
    write(isp, SOURCE, word);
    let word = read(isp, SOURCE_ENABLE) & !1 | u32::from(selectors.source_enabled);
    write(isp, SOURCE_ENABLE, word);
}

/// One dimension of a grid: its start, 1..=16 blocks and a block size of 1..=512.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span { pub start: u16, pub blocks: u8, pub size: u16 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Grid { pub spans: [Span; 2], pub gaps: [u16; 2] }

/// The third grid's second start is checked one higher and programmed six higher.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GridId { First, Second, Third }

fn span_word(start: u32, span: Span) -> u32 {
    (start & 0x1fff) << 16 | (u32::from(span.blocks) - 1 & 0xf) << 12 | u32::from(span.size) - 1 & 0x1ff
}

fn span_fits(span: Span) -> bool { (1..=16).contains(&span.blocks) && (1..=512).contains(&span.size) }

pub fn set_grid<I: Isp>(isp: &mut I, caller: Caller, id: GridId, grid: Grid) -> Result<(), Refusal> {
    let (operation, registers, checked, programmed) = match id {
        GridId::First => (Operation(10), [0x100, 0x104, 0x108], 0, 0),
        GridId::Second => (Operation(0x14), [0x200, 0x204, 0x258], 1, 1),
        GridId::Third => (Operation(0x18), [0x300, 0x304, 0x30c], 1, 6),
    };
    begin(isp, caller, operation);
    let [first, mut second] = grid.spans;
    let mut gaps = grid.gaps;
    if id == GridId::First {
        let contracted = contract(isp, second.start, second.size, gaps[1]);
        (second.start, second.size, gaps[1]) = contracted;
    }
    let checks = [
        first.start <= 0x1fff,
        span_fits(first),
        u32::from(second.start) + checked <= 0x1fff,
        span_fits(second),
        gaps[0] <= 0x1ff,
        gaps[1] <= 0x1ff,
    ];
    admit(isp, caller, operation, &checks)?;
    write(isp, registers[0], span_word(first.start.into(), first));
    write(isp, registers[1], span_word(u32::from(second.start) + programmed, second));
    write(isp, registers[2], u32::from(gaps[0]) << 16 | u32::from(gaps[1]));
    Ok(())
}

/// A start inside scaling mode 1's margin is logged against the unit's holder and kept.
fn contract<I: Isp>(isp: &mut I, start: u16, size: u16, gap: u16) -> (u16, u16, u16) {
    let start = start.wrapping_add(6);
    let scaling = read(isp, SCALING);
    match scaling >> 8 & 7 {
        1 => {
            let margin = (scaling >> 24 & 0x1f) as u16;
            let shift = scaling >> 16 & 7;
            let start = if start < margin {
                let record = isp.record(UNIT);
                isp.log(Event::Refused { client: record.holder, unit: UNIT, operation: record.operation, fault: Fault::Parameter });
                start
            } else {
                ((u32::from(start - margin) >> shift) as u16).wrapping_add(margin)
            };
            let size = ((u32::from(size) >> shift) as u16).max(1);
            (start, size, (u32::from(gap) >> shift) as u16)
        }
        2..=4 => (start.wrapping_add(2), size, gap),
        _ => (start, size, gap),
    }
}

/// A position below 0x2000 (0x4000 in the second bank) and a length of 1..=0x2000.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Segment { pub position: u16, pub length: u16 }

const SEGMENTS: usize = 10;

fn segment_word(position: u32, length: u16) -> u32 { position << 16 | u32::from(length) - 1 & 0x1fff }

fn length_fits(length: u16) -> bool { (1..=0x2000).contains(&length) }

pub fn set_first_bank<I: Isp>(isp: &mut I, caller: Caller, pairs: &[[Segment; 2]; SEGMENTS]) -> Result<(), Refusal> {
    let operation = Operation(12);
    begin(isp, caller, operation);
    for (index, [first, second]) in (0u16..).zip(pairs) {
        let (position, length, _) = contract(isp, second.position, second.length, 0);
        let checks = [first.position <= 0x1fff, length_fits(first.length), position <= 0x1fff, length_fits(length)];
        admit(isp, caller, operation, &checks)?;
        write(isp, FIRST_BANK + 8 * index, segment_word(first.position.into(), first.length));
        write(isp, FIRST_BANK + 8 * index + 4, segment_word(position.into(), length));
    }
    Ok(())
}

pub fn set_second_bank<I: Isp>(isp: &mut I, caller: Caller, pairs: &[[Segment; 2]; SEGMENTS]) -> Result<(), Refusal> {
    let operation = Operation(0x16);
    begin(isp, caller, operation);
    for (index, [first, second]) in (0u16..).zip(pairs) {
        let shifted = u32::from(second.position) + 1;
        let checks = [first.position <= 0x3fff, length_fits(first.length), shifted <= 0x1fff, length_fits(second.length)];
        admit(isp, caller, operation, &checks)?;
        write(isp, SECOND_BANK + 8 * index, segment_word(first.position.into(), first.length));
        write(isp, SECOND_BANK + 8 * index + 4, segment_word(shifted, second.length));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Window {
    First([Segment; 2]),
    Second(Span, Segment),
    Third(Segment, Span),
    Fourth([Segment; 2]),
}

pub fn set_window<I: Isp>(isp: &mut I, caller: Caller, window: Window) -> Result<(), Refusal> {
    let (operation, register, checks, words) = match window {
        Window::First([first, second]) => (
            Operation(0x1c),
            0x400,
            [first.position <= 0x3fff, length_fits(first.length), second.position <= 0x1fff, length_fits(second.length)],
            [segment_word(first.position.into(), first.length), segment_word(second.position.into(), second.length)],
        ),
        Window::Second(span, segment) => (
            Operation(0x1e),
            0x408,
            [span.start <= 0x3fff, span_fits(span), segment.position <= 0x1fff, length_fits(segment.length)],
            [span_word14(span), segment_word(segment.position.into(), segment.length)],
        ),
        Window::Third(segment, span) => (
            Operation(0x20),
            0x410,
            [segment.position <= 0x3fff, length_fits(segment.length), span.start <= 0x1fff, span_fits(span)],
            [segment_word(segment.position.into(), segment.length), span_word(span.start.into(), span)],
        ),
        Window::Fourth([first, second]) => {
            let shifted = u32::from(second.position) + 1;
            (
                Operation(0x31),
                0x604,
                [first.position <= 0x1fff, length_fits(first.length), shifted <= 0x1fff, length_fits(second.length)],
                [segment_word(first.position.into(), first.length), segment_word(shifted, second.length)],
            )
        }
    };
    begin(isp, caller, operation);
    admit(isp, caller, operation, &checks)?;
    write(isp, register, words[0]);
    write(isp, register + 4, words[1]);
    Ok(())
}

fn span_word14(span: Span) -> u32 {
    u32::from(span.start) << 16 | (u32::from(span.blocks) - 1 & 0xf) << 12 | u32::from(span.size) - 1 & 0x1ff
}
