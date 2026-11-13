//! The stock layouts of the argument blocks and words the ISP units' X-M1 1.50 entries take, read
//! into the typed values of `raid0`, `mvrec` and `auto`. A word an entry refuses at run time is a
//! `ParameterFault`, refused through `driver::refuse_parameter`.

use core::ptr::read_volatile;

use crate::driver::{Operation, ParameterFault, Unit};

/// Memory an argument block lives in, by guest address.
pub trait GuestMemory {
    fn byte(&self, address: u32) -> u8;
    fn halfword(&self, address: u32) -> u16;
    fn word(&self, address: u32) -> u32;
    fn words(&self, address: u32, count: usize) -> &[u32];
    fn table<const N: usize>(&self, address: u32) -> &[u32; N];
}

/// Guest memory at its own addresses.
#[derive(Clone, Copy, Debug, Default)]
pub struct StockMemory;

impl GuestMemory for StockMemory {
    fn byte(&self, address: u32) -> u8 { unsafe { read_volatile(address as usize as *const u8) } }

    fn halfword(&self, address: u32) -> u16 { unsafe { read_volatile(address as usize as *const u16) } }

    fn word(&self, address: u32) -> u32 { unsafe { read_volatile(address as usize as *const u32) } }

    fn words(&self, address: u32, count: usize) -> &[u32] { unsafe { core::slice::from_raw_parts(address as usize as *const u32, count) } }

    fn table<const N: usize>(&self, address: u32) -> &[u32; N] { unsafe { &*(address as usize as *const [u32; N]) } }
}

fn halfwords<const N: usize>(memory: &impl GuestMemory, at: u32) -> [u16; N] {
    core::array::from_fn(|index| memory.halfword(at + 2 * index as u32))
}

fn tables<'m, const N: usize, const M: usize>(memory: &'m impl GuestMemory, at: u32) -> [&'m [u32; N]; M] {
    core::array::from_fn(|index| memory.table(memory.word(at + 4 * index as u32)))
}

const fn switch(word: u32, fault: ParameterFault) -> Result<bool, ParameterFault> {
    match word {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(fault),
    }
}

/// RAID0's entries 0x685208..0x688c44.
pub mod raid0 {
    use super::{halfwords, switch, tables, GuestMemory, Operation, ParameterFault, Unit};
    use crate::raid0::{self as unit, Block, Division, DivisionMode, Format, Gates, Mode, Path, PathSource, SecondExtent, SecondWindow, Shift, Stage, StageMode, Window};

    const fn fault(operation: Operation) -> ParameterFault { ParameterFault { unit: Unit::Raid0, operation } }

    pub fn stage_mode(stage: Stage, word: u32) -> Result<StageMode, ParameterFault> {
        match word {
            0 => Ok(StageMode::Off),
            1 => Ok(StageMode::On),
            2 => Ok(StageMode::Alternate),
            _ => Err(fault(stage.operation())),
        }
    }

    /// The first and second gate words (0x685598).
    pub fn gates(words: [u32; 2]) -> Result<Gates, ParameterFault> {
        let refused = fault(unit::GATES);
        Ok(Gates { first: switch(words[0], refused)?, second: switch(words[1], refused)? })
    }

    /// The source and enable words (0x68577c).
    pub fn path(words: [u32; 2]) -> Result<Path, ParameterFault> {
        let refused = fault(unit::PATH);
        let source = match words[0] {
            0 => PathSource::First,
            1 => PathSource::Second,
            2 => PathSource::Third,
            3 => PathSource::Fourth,
            _ => return Err(refused),
        };
        Ok(Path { source, enabled: switch(words[1], refused)? })
    }

    /// 0x686560.
    pub fn gate(word: u32) -> Result<bool, ParameterFault> { switch(word, fault(unit::SET_GATE)) }

    /// 0x6873b8.
    pub fn output(word: u32) -> Result<bool, ParameterFault> { switch(word, fault(unit::SET_OUTPUT)) }

    /// 0x687ef0.
    pub fn mode(enabled: u32, level: u8) -> Result<Mode, ParameterFault> {
        Ok(Mode { enabled: switch(enabled, fault(unit::SET_MODE))?, level })
    }

    /// 0x685b94.
    pub fn origin(memory: &impl GuestMemory, at: u32) -> [u16; 3] { halfwords(memory, at) }

    /// 0x685dd0.
    pub fn format(memory: &impl GuestMemory, at: u32) -> Format {
        Format { first: memory.byte(at + 4), second: memory.byte(at + 5), third: memory.halfword(at), fourth: memory.halfword(at + 2) }
    }

    /// 0x68604c.
    pub fn extent(memory: &impl GuestMemory, at: u32) -> [u16; 4] { halfwords(memory, at) }

    /// 0x686308.
    pub fn division(memory: &impl GuestMemory, at: u32) -> Result<Division, ParameterFault> {
        let mode = match memory.word(at) {
            0 => DivisionMode::First,
            1 => DivisionMode::Second,
            2 => DivisionMode::Third,
            3 => DivisionMode::Fourth,
            _ => return Err(fault(unit::SET_DIVISION)),
        };
        Ok(Division { mode, count: memory.halfword(at + 4) })
    }

    /// The point list and its shift (0x6866d4); a count above 1024 is refused before the list is
    /// read.
    pub fn points<'m>(memory: &'m impl GuestMemory, at: u32) -> Result<(&'m [u32], Shift), ParameterFault> {
        let count = usize::from(memory.halfword(at + 4));
        if count > unit::POINT_LIMIT {
            return Err(fault(unit::POINTS_LOAD));
        }
        let shift = Shift { low: memory.halfword(at + 6), high: memory.halfword(at + 8) };
        Ok((memory.words(memory.word(at), count), shift))
    }

    /// The pair entries and their shift (0x686904); a count above 16 is refused before the entries
    /// are read.
    pub fn pairs<'m>(memory: &'m impl GuestMemory, at: u32) -> Result<(&'m [u32], u16), ParameterFault> {
        let count = usize::from(memory.halfword(at + 4));
        if count > unit::PAIR_LIMIT {
            return Err(fault(unit::PAIRS_LOAD));
        }
        Ok((memory.words(memory.word(at), count), memory.halfword(at + 6)))
    }

    /// 0x686b54.
    pub fn area(memory: &impl GuestMemory, at: u32) -> [u16; 4] { halfwords(memory, at) }

    /// 0x686e10.
    pub fn block(memory: &impl GuestMemory, at: u32) -> Block {
        Block { base: memory.word(at), stride: memory.halfword(at + 4), corners: halfwords(memory, at + 6), limits: halfwords(memory, at + 0xe) }
    }

    /// 0x68752c.
    pub fn window(memory: &impl GuestMemory, at: u32) -> Window {
        Window {
            position: halfwords(memory, at),
            steps: [memory.byte(at + 4), memory.byte(at + 5)],
            size: halfwords(memory, at + 6),
            length: memory.halfword(at + 0xa),
        }
    }

    /// Nine pointers to 64-word curves (0x6879a8).
    pub fn curves<'m>(memory: &'m impl GuestMemory, at: u32) -> [&'m [u32; 64]; 9] { tables(memory, at) }

    /// The block 0x688098 shares with `second_extent`'s 0x6882f4.
    pub fn second_window(memory: &impl GuestMemory, at: u32) -> SecondWindow {
        SecondWindow { position: halfwords(memory, at), steps: [memory.byte(at + 0xc), memory.byte(at + 0xd)], length: memory.halfword(at + 0xe) }
    }

    pub fn second_extent(memory: &impl GuestMemory, at: u32) -> SecondExtent {
        SecondExtent { extent: halfwords(memory, at + 4), span: halfwords(memory, at + 8), steps: [memory.byte(at + 0xc), memory.byte(at + 0xd)] }
    }

    /// Two pointers to 14-word tables (0x688730).
    pub fn rotated<'m>(memory: &'m impl GuestMemory, at: u32) -> [&'m [u32; 14]; 2] { tables(memory, at) }

    /// Three pointers to 256-word tables (0x6889f0, 0x688c44).
    pub fn table_set<'m>(memory: &'m impl GuestMemory, at: u32) -> [&'m [u32; 256]; 3] { tables(memory, at) }
}

/// MVREC's entries 0x671eec..0x67395c.
pub mod mvrec {
    use super::{halfwords, switch, GuestMemory, Operation, ParameterFault, Unit};
    use crate::mvrec::{self as unit, Address, Geometry, Offset, Readback, ReadbackMode, ReadbackSource, SecondaryGeometry, Window};

    const fn fault(operation: Operation) -> ParameterFault { ParameterFault { unit: Unit::Mvrec, operation } }

    /// 0x671eec.
    pub fn dimensions(memory: &impl GuestMemory, at: u32) -> [u16; 2] { halfwords(memory, at) }

    /// 0x6720d8.
    pub fn geometry(memory: &impl GuestMemory, at: u32) -> Result<Geometry, ParameterFault> {
        let enabled = switch(memory.word(at), fault(unit::SET_GEOMETRY))?;
        Ok(Geometry { enabled, first: memory.halfword(at + 4), second: memory.halfword(at + 6) })
    }

    /// Two halfwords for the first offset (0x6722ec), two words for the others (0x6724a8,
    /// 0x67266c).
    pub fn offset(memory: &impl GuestMemory, at: u32, offset: Offset) -> [u32; 2] {
        match offset {
            Offset::First => halfwords(memory, at).map(u32::from),
            Offset::Second | Offset::Third => [memory.word(at), memory.word(at + 4)],
        }
    }

    /// 0x672830.
    pub fn window(memory: &impl GuestMemory, at: u32) -> Window {
        Window { position: halfwords(memory, at), repeat: memory.byte(at + 4), count: memory.byte(at + 5) }
    }

    /// The 31 words in place (0x672b2c).
    pub fn table<'m>(memory: &'m impl GuestMemory, at: u32) -> &'m [u32; 31] { memory.table(at) }

    /// 0x672c84.
    pub fn secondary_geometry(memory: &impl GuestMemory, at: u32) -> SecondaryGeometry {
        let [width, height, offset, stride] = halfwords(memory, at);
        SecondaryGeometry { width, height, offset, stride }
    }

    /// 0x6730b4: an enable word other than 1 disables.
    pub fn readback(memory: &impl GuestMemory, at: u32) -> Result<Readback, ParameterFault> {
        let refused = fault(unit::SET_READBACK);
        let source = match memory.word(at + 8) {
            0 => ReadbackSource::First,
            1 => ReadbackSource::Second,
            2 => ReadbackSource::Third,
            3 => ReadbackSource::Fourth,
            _ => return Err(refused),
        };
        let mode = match memory.word(at + 0xc) {
            0 => ReadbackMode::First,
            1 => ReadbackMode::Second,
            2 => ReadbackMode::Third,
            3 => ReadbackMode::Fourth,
            4 => ReadbackMode::Fifth,
            _ => return Err(refused),
        };
        Ok(Readback { enabled: memory.word(at) == 1, region: memory.halfword(at + 4), source, mode })
    }

    /// 0x6734d4.
    pub fn address(memory: &impl GuestMemory, at: u32) -> Address { Address { address: memory.word(at), stride: memory.halfword(at + 4) } }

    /// The programmed and the checked nibble (0x67395c).
    pub fn nibbles(memory: &impl GuestMemory, at: u32) -> [u8; 2] { [memory.byte(at), memory.byte(at + 1)] }
}

/// AUTO's entries 0x6768f8..0x67b194.
pub mod auto {
    use super::{halfwords, GuestMemory, ParameterFault, Unit};
    use crate::auto::{self as unit, Grid, Segment, Selectors, Source, Span, Window};

    const SEGMENTS: usize = 10;

    fn span(memory: &impl GuestMemory, at: u32) -> Span { Span { start: memory.halfword(at), blocks: memory.byte(at + 2), size: memory.halfword(at + 4) } }

    fn segment(memory: &impl GuestMemory, at: u32) -> Segment { Segment { position: memory.halfword(at), length: memory.halfword(at + 2) } }

    /// 0x6768f8: the third and fourth fields are words; an enable word other than 0 enables.
    pub fn selectors(memory: &impl GuestMemory, at: u32) -> Result<Selectors, ParameterFault> {
        let source = match memory.word(at + 0xc) {
            0 => Source::First,
            1 => Source::Second,
            2 => Source::Third,
            _ => return Err(ParameterFault { unit: Unit::Auto, operation: unit::SET_SELECTORS }),
        };
        let fields = [memory.byte(at), memory.byte(at + 1), memory.word(at + 4) as u8, memory.word(at + 8) as u8];
        Ok(Selectors { fields, source, source_enabled: memory.word(at + 0x10) != 0 })
    }

    /// The block every grid entry takes (0x676c18, 0x6789b4, 0x679490).
    pub fn grid(memory: &impl GuestMemory, at: u32) -> Grid {
        Grid { spans: [span(memory, at), span(memory, at + 6)], gaps: halfwords(memory, at + 0xc) }
    }

    /// Ten first segments, then their ten second segments (0x6771b0, 0x678f60).
    pub fn bank(memory: &impl GuestMemory, at: u32) -> [[Segment; 2]; SEGMENTS] {
        core::array::from_fn(|index| {
            let entry = at + 4 * index as u32;
            [segment(memory, entry), segment(memory, entry + 4 * SEGMENTS as u32)]
        })
    }

    /// 0x679c60.
    pub fn first_window(memory: &impl GuestMemory, at: u32) -> Window { Window::First([segment(memory, at), segment(memory, at + 4)]) }

    /// 0x679fb0.
    pub fn second_window(memory: &impl GuestMemory, at: u32) -> Window { Window::Second(span(memory, at), segment(memory, at + 6)) }

    /// 0x67a384.
    pub fn third_window(memory: &impl GuestMemory, at: u32) -> Window { Window::Third(segment(memory, at), span(memory, at + 4)) }

    /// 0x67b194.
    pub fn fourth_window(memory: &impl GuestMemory, at: u32) -> Window { Window::Fourth([segment(memory, at), segment(memory, at + 4)]) }
}
