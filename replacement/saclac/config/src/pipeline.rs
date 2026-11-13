#![forbid(unsafe_code)]

//! Image-pipeline stage control over the image-4 register layer (image 5, 0xd9e964..0xda027c):
//! enabling and disabling stages, resetting a transform bank, and programming a caller's region,
//! parameter and geometry blocks into the units.
//!
//! Suspected stock defect, kept: flag 0x400 of 0xd9ea08 and 0xd9f828 programs the processing
//! unit, and flags 0x2000 and 0x4000 of 0xd9fd54 the processing and input units, without holding
//! them (unit masks 0xfc at 0xd9ea50 and 0xd9f874, 0xb at 0xd9fd88).

/// The pipeline units a stage operation holds while it programs them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unit {
    /// CCDIF: 0x65df68 and 0x65e0c8, which stay stock with the input unit.
    Input,
    /// MVREC: 0x671aa8 and 0x671bf8.
    Transform,
    /// RAID0: 0x684eb4 and 0x685004.
    Processing,
    /// AUTO: 0x676104 and 0x676254.
    Filter,
}

/// Stages whose mode is set from the parameter block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    /// 0x685208.
    Stage400,
    /// 0x6852ec.
    Stage4,
    /// 0x6853d0.
    Stage8,
    /// 0x6854b4.
    Stage10,
}

/// One of the transform unit's two register banks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bank { First, Second }

/// One of the filter unit's three grids.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Grid { First, Second, Third }

/// One of the filter unit's four windows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterWindow { First, Second, Third, Fourth }

/// A part of the caller's block that one register-layer operation programs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Part {
    /// Region block: 0x685b94.
    ProcessingOrigin,
    /// Region block: 0x685dd0.
    ProcessingFormat,
    /// Region block: 0x65f3d8, which stays stock with the input unit.
    InputMode,
    /// Region block: 0x6768f8.
    FilterSelectors,
    /// Parameter block: 0x65fd70, stock.
    InputControl,
    /// Parameter block: 0x686308.
    Division,
    /// Parameter block: 0x6866d4.
    Points,
    /// Parameter block: 0x686904.
    Pairs,
    /// Parameter block: 0x688098.
    SecondWindow,
    /// Parameter block: 0x688730.
    RotatedTables,
    /// Parameter block: 0x6882f4.
    SecondExtent,
    /// Parameter block: 0x6889f0.
    FirstTables,
    /// Parameter block: 0x688c44.
    SecondTables,
    /// Parameter block: 0x686b54.
    Area,
    /// Parameter block: 0x686e10.
    BlockLayout,
    /// Parameter block: 0x66032c, stock.
    InputRegister3c,
    /// Parameter block: 0x6722ec.
    TransformOffset(Bank),
    /// Parameter block: 0x67266c.
    TransformThirdOffset(Bank),
    /// Parameter block: 0x672830.
    TransformWindow(Bank),
    /// Parameter block: 0x672b2c.
    TransformTable(Bank),
    /// Parameter block: 0x68752c.
    Window,
    /// Parameter block: 0x6879a8.
    Curves,
    /// Geometry block: 0x65f51c, stock.
    InputRegister14,
    /// Geometry block: 0x671eec.
    TransformDimensions(Bank),
    /// Geometry block: 0x6720d8.
    TransformGeometry(Bank),
    /// Geometry block: 0x6724a8.
    TransformSecondOffset(Bank),
    /// Geometry block: 0x672c84.
    TransformSecondaryGeometry(Bank),
    /// Geometry block: 0x6730b4.
    TransformReadback(Bank),
    /// Geometry block: 0x6734d4.
    TransformAddress(Bank),
    /// Geometry block: 0x67395c.
    TransformNibble(Bank),
    /// Geometry block: 0x660040, stock.
    InputRegister34,
    /// Geometry block: 0x660040 on the block's second input word pair.
    InputRegister34Alternate,
    /// Geometry block: 0x65f9c8, stock.
    InputRegister28,
    /// Geometry block: 0x676c18, 0x6789b4, 0x679490.
    FilterGrid(Grid),
    /// Geometry block: 0x6771b0.
    FilterFirstBank,
    /// Geometry block: 0x678f60.
    FilterSecondBank,
    /// Geometry block: 0x679c60, 0x679fb0, 0x67a384, 0x67b194.
    FilterWindow(FilterWindow),
    /// Geometry block: 0x68604c.
    Extent,
}

/// A one-bit processing-unit switch. Its word is 0 or 1; the unit refuses any other.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Switch {
    /// 0x68661c and 0x686560.
    Gate,
    /// 0x687474 and 0x6873b8.
    Output,
}

/// An enable bit the parameter block changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Enable { Gate, Mode, Output }

/// What the parameter block does to an enable bit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnableChange {
    Keep,
    Invert,
    /// The block's word, passed to the unit as it is.
    Set(u32),
}

impl EnableChange {
    fn applied_to(self, current: u32) -> u32 {
        match self {
            Self::Keep => current,
            Self::Invert => u32::from(current == 0),
            Self::Set(word) => word,
        }
    }
}

/// A parameter-block word the parameter write copies into a register.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Setting {
    /// The path's enable word.
    PathEnable,
    /// The second gate's word.
    SecondGate,
    /// The mode register's level, a byte.
    ModeLevel,
}

/// The processing unit's mode register as 0x687fc8 reads it and 0x687ef0 writes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModeWords {
    pub enabled: u32,
    pub level: u8,
}

/// A caller's region, parameter or geometry block, read where the stage control decides.
pub trait StageBlock {
    /// The mode word for `stage`; 2 selects the stage's second mode.
    fn stage_mode(&self, stage: Stage) -> u32;
    fn enable_change(&self, enable: Enable) -> EnableChange;
    fn setting(&self, setting: Setting) -> u32;
}

/// The register layer's operations the stage control uses; each returns the stock status.
pub trait ImageStages {
    type Session;
    type Block: StageBlock;

    fn acquire(&mut self, unit: Unit, handle: u32) -> (i32, Self::Session);
    fn release(&mut self, session: Self::Session) -> i32;
    /// Programs `part` of `block` under `session`.
    fn program(&mut self, session: &Self::Session, block: &Self::Block, part: Part) -> i32;
    /// 0x65fcb4.
    fn enable_input(&mut self, session: &Self::Session, enabled: bool) -> i32;
    fn set_stage(&mut self, session: &Self::Session, stage: Stage, mode: u32) -> i32;
    /// The processing unit's two gate words (0x6856cc).
    fn gates(&mut self, session: &Self::Session) -> (i32, [u32; 2]);
    /// 0x685598.
    fn set_gates(&mut self, session: &Self::Session, words: [u32; 2]) -> i32;
    /// The processing unit's path: source and enable words (0x6858e0).
    fn path(&mut self, session: &Self::Session) -> (i32, [u32; 2]);
    /// 0x68577c.
    fn set_path(&mut self, session: &Self::Session, words: [u32; 2]) -> i32;
    fn switch(&mut self, session: &Self::Session, switch: Switch) -> (i32, u32);
    fn set_switch(&mut self, session: &Self::Session, switch: Switch, word: u32) -> i32;
    /// 0x687fc8.
    fn mode(&mut self, session: &Self::Session) -> (i32, ModeWords);
    /// 0x687ef0.
    fn set_mode(&mut self, session: &Self::Session, mode: ModeWords) -> i32;
    /// 0x6722ec.
    fn set_transform_offset(&mut self, session: &Self::Session, offset: [u16; 2], bank: Bank) -> i32;
    /// The bank's window positions, repeat and count (0x672a50).
    fn transform_window(&mut self, session: &Self::Session, bank: Bank) -> (i32, [u16; 4]);
    /// 0x672830.
    fn set_transform_window(&mut self, session: &Self::Session, window: [u16; 4], bank: Bank) -> i32;
    /// 0x672b2c.
    fn load_transform_table(&mut self, session: &Self::Session, table: &[u32; 31], bank: Bank) -> i32;
    /// 0x66049c.
    fn reset_input_unit(&mut self) -> i32;
    /// The record stock passes for `unit` when the operation does not hold it: `handle` as the
    /// client word and a session word the unit's operations do not read (0x685298, 0x68634c,
    /// 0x686084, 0x660084).
    fn unheld(&self, unit: Unit, handle: u32) -> Self::Session;
}

struct Record<S: ImageStages> {
    session: S::Session,
    held: bool,
}

impl<S: ImageStages> Record<S> {
    fn take(stages: &mut S, unit: Unit, handle: u32, held: bool) -> Self {
        let session = if held { stages.acquire(unit, handle).1 } else { stages.unheld(unit, handle) };
        Self { session, held }
    }
}

/// Takes each unit's record, holding the units whose mask meets `flags`, in order.
fn records<S: ImageStages, const N: usize>(stages: &mut S, handle: u32, flags: u32, units: [(Unit, u32); N]) -> [Record<S>; N] {
    units.map(|(unit, mask)| Record::take(stages, unit, handle, flags & mask != 0))
}

/// Releases the held units in order; the last release's status, or `None` when none was held.
fn release<S: ImageStages, const N: usize>(stages: &mut S, records: [Record<S>; N]) -> Option<i32> {
    records.into_iter().filter(|record| record.held).fold(None, |_, record| Some(stages.release(record.session)))
}

/// The units stage control and the parameter write hold, by the flags that need each.
const SHARED_UNITS: [(Unit, u32); 3] = [(Unit::Input, 0x3), (Unit::Transform, 0x300), (Unit::Processing, 0xfc)];

/// Enables or disables the stages `flags` selects: input enable (bit 0), stage modes (0x400, 4,
/// 8, 0x10), the first gate (0x20) and the path source bits (0x40, 0x80). Enabling sets each
/// stage's mode to 2 where the block asks for 2, else 1, and sets the source bit; disabling sets
/// mode 0 and toggles the source bit as stock does (0xd9ea08). Returns the last release's status,
/// with no unit held the unheld stage write's (0xd9eaa4), else 0.
pub fn enable_stages<S: ImageStages>(stages: &mut S, handle: u32, flags: u32, enabled: bool, block: Option<&S::Block>) -> i32 {
    let [input, transform, processing] = records(stages, handle, flags, SHARED_UNITS);
    if flags & 1 != 0 {
        stages.enable_input(&input.session, enabled);
    }
    let mut unheld = None;
    for (mask, stage) in [(0x400, Stage::Stage400), (0x4, Stage::Stage4), (0x8, Stage::Stage8), (0x10, Stage::Stage10)] {
        if flags & mask != 0 {
            let mode = match block {
                Some(block) if enabled => if block.stage_mode(stage) == 2 { 2 } else { 1 },
                _ => 0,
            };
            let status = stages.set_stage(&processing.session, stage, mode);
            if !processing.held {
                unheld = Some(status);
            }
        }
    }
    if flags & 0x20 != 0 {
        let (_, [_, second]) = stages.gates(&processing.session);
        stages.set_gates(&processing.session, [u32::from(enabled), second]);
    }
    for (mask, bit) in [(0x40, 1), (0x80, 2)] {
        if flags & mask != 0 {
            let (_, [source, path_enabled]) = stages.path(&processing.session);
            let source = if enabled { source | bit } else { source ^ bit };
            stages.set_path(&processing.session, [source, path_enabled]);
        }
    }
    release(stages, [input, transform, processing]).or(unheld).unwrap_or(0)
}

/// 0xd9f7c0.
pub fn enable_configured_stages<S: ImageStages>(stages: &mut S, handle: u32, flags: u32, block: &S::Block) -> i32 {
    enable_stages(stages, handle, flags, true, Some(block))
}

/// Disables the stages and resets the transform banks `flags` names (0x100, 0x200)
/// (0xd9f7d8).
pub fn disable_stages<S: ImageStages>(stages: &mut S, handle: u32, flags: u32) -> i32 {
    let result = enable_stages(stages, handle, flags, false, None);
    for (mask, bank) in [(0x100, Bank::First), (0x200, Bank::Second)] {
        if flags & mask != 0 {
            reset_transform(stages, handle, bank);
        }
    }
    result
}

/// Resets a transform bank: first offsets 0x1000, window positions 0 and every table entry
/// 0x10000 (0xd9e964).
pub fn reset_transform<S: ImageStages>(stages: &mut S, handle: u32, bank: Bank) {
    let (_, session) = stages.acquire(Unit::Transform, handle);
    stages.set_transform_offset(&session, [0x1000, 0x1000], bank);
    let (_, [_, _, repeat, count]) = stages.transform_window(&session, bank);
    stages.set_transform_window(&session, [0, 0, repeat, count], bank);
    stages.load_transform_table(&session, &[0x1_0000; 31], bank);
    stages.release(session);
}

/// Writes the processing region and input mode (flag 4), then the filter selectors (flag 2)
/// (0xda027c). Returns the last release's status.
pub fn write_region<S: ImageStages>(stages: &mut S, handle: u32, flags: u32, block: &S::Block) -> i32 {
    let mut result = 0;
    if flags & 4 != 0 {
        let (_, processing) = stages.acquire(Unit::Processing, handle);
        stages.program(&processing, block, Part::ProcessingOrigin);
        stages.program(&processing, block, Part::ProcessingFormat);
        stages.release(processing);
        let (_, input) = stages.acquire(Unit::Input, handle);
        stages.program(&input, block, Part::InputMode);
        result = stages.release(input);
    }
    if flags & 2 != 0 {
        let (_, filter) = stages.acquire(Unit::Filter, handle);
        stages.program(&filter, block, Part::FilterSelectors);
        result = stages.release(filter);
    }
    result
}

/// Reads a switch and writes it back changed, unless the change keeps it.
fn change_switch<S: ImageStages>(stages: &mut S, session: &S::Session, switch: Switch, change: EnableChange) {
    if change != EnableChange::Keep {
        let (_, current) = stages.switch(session, switch);
        stages.set_switch(session, switch, change.applied_to(current));
    }
}

/// Programs the parameter block's groups `flags` selects into the units running them
/// (0xd9f828). Returns the last release's status, with no unit held the unheld division write's
/// (0xd9f8b0..0xd9f8b4), else 0.
pub fn apply_parameters<S: ImageStages>(stages: &mut S, handle: u32, flags: u32, block: &S::Block) -> i32 {
    let [input, transform, processing] = records(stages, handle, flags, SHARED_UNITS);
    let on = |mask: u32| flags & mask != 0;
    let (input_session, transform_session, processing_session) = (&input.session, &transform.session, &processing.session);
    if on(1) {
        stages.program(input_session, block, Part::InputControl);
    }
    let division = on(0x400).then(|| stages.program(processing_session, block, Part::Division));
    if on(4) {
        stages.program(processing_session, block, Part::Points);
        change_switch(stages, processing_session, Switch::Gate, block.enable_change(Enable::Gate));
    }
    if on(8) {
        stages.program(processing_session, block, Part::Pairs);
    }
    if on(0x40) {
        stages.program(processing_session, block, Part::SecondWindow);
        stages.program(processing_session, block, Part::RotatedTables);
    }
    if on(0x80) {
        for part in [Part::SecondExtent, Part::FirstTables, Part::SecondTables] {
            stages.program(processing_session, block, part);
        }
        let (_, mode) = stages.mode(processing_session);
        let (_, [source, _]) = stages.path(processing_session);
        stages.set_path(processing_session, [source, block.setting(Setting::PathEnable)]);
        let mode = ModeWords {
            enabled: block.enable_change(Enable::Mode).applied_to(mode.enabled),
            level: block.setting(Setting::ModeLevel) as u8,
        };
        stages.set_mode(processing_session, mode);
    }
    if on(0x10) {
        stages.program(processing_session, block, Part::Area);
        stages.program(processing_session, block, Part::BlockLayout);
    }
    if on(2) {
        stages.program(input_session, block, Part::InputRegister3c);
    }
    for (mask, bank) in [(0x100, Bank::First), (0x200, Bank::Second)] {
        if on(mask) {
            for part in [Part::TransformOffset(bank), Part::TransformThirdOffset(bank), Part::TransformWindow(bank), Part::TransformTable(bank)] {
                stages.program(transform_session, block, part);
            }
        }
    }
    if on(0x20) {
        stages.program(processing_session, block, Part::Window);
        stages.program(processing_session, block, Part::Curves);
        let (_, [first, _]) = stages.gates(processing_session);
        stages.set_gates(processing_session, [first, block.setting(Setting::SecondGate)]);
        change_switch(stages, processing_session, Switch::Output, block.enable_change(Enable::Output));
    }
    release(stages, [input, transform, processing]).or(division).unwrap_or(0)
}

/// Programs the geometry block's groups `flags` selects (0xd9fd54). Returns the last release's
/// status, with no unit held the last unheld write's (0xd9ff90, 0xd9ffa4), else 0.
pub fn configure_geometry<S: ImageStages>(stages: &mut S, handle: u32, flags: u32, block: &S::Block) -> i32 {
    let units = [(Unit::Filter, 0x7f0), (Unit::Input, 0xb), (Unit::Transform, 0x1800), (Unit::Processing, 0)];
    let [filter, input, transform, processing] = records(stages, handle, flags, units);
    let program = |stages: &mut S, record: &Record<S>, mask: u32, parts: &[Part]| {
        (flags & mask != 0).then(|| parts.iter().fold(0, |_, part| stages.program(&record.session, block, *part)))
    };
    program(stages, &input, 1, &[Part::InputRegister14]);
    for (mask, bank) in [(0x800, Bank::First), (0x1000, Bank::Second)] {
        let parts = [
            Part::TransformDimensions(bank),
            Part::TransformGeometry(bank),
            Part::TransformSecondOffset(bank),
            Part::TransformSecondaryGeometry(bank),
            Part::TransformReadback(bank),
            Part::TransformAddress(bank),
            Part::TransformNibble(bank),
        ];
        program(stages, &transform, mask, &parts);
    }
    program(stages, &input, 2, &[Part::InputRegister34]);
    program(stages, &filter, 0x10, &[Part::FilterGrid(Grid::Second)]);
    program(stages, &filter, 0x20, &[Part::FilterSecondBank]);
    program(stages, &filter, 0x40, &[Part::FilterGrid(Grid::First)]);
    program(stages, &filter, 0x80, &[Part::FilterFirstBank]);
    program(stages, &filter, 0x100, &[Part::FilterGrid(Grid::Third)]);
    let windows = [FilterWindow::First, FilterWindow::Second, FilterWindow::Third].map(Part::FilterWindow);
    program(stages, &filter, 0x200, &windows);
    program(stages, &filter, 0x400, &[Part::FilterWindow(FilterWindow::Fourth)]);
    program(stages, &input, 8, &[Part::InputRegister28]);
    let alternate = program(stages, &input, 0x4000, &[Part::InputRegister34Alternate]);
    let extent = program(stages, &processing, 0x2000, &[Part::Extent]);
    release(stages, [filter, input, transform, processing]).or(extent).or(alternate).unwrap_or(0)
}
