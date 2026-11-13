use crate::pipeline::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Op {
    Acquire(Unit),
    Release(Unit),
    Program(Unit, Part),
    EnableInput(bool),
    Stage(Stage, StageMode),
    Gates([u32; 2]),
    Path([u32; 2]),
    Switch(Switch, u32),
    Mode(ModeWords),
    Offset([u16; 2], Bank),
    Window([u16; 4], Bank),
    Table(Bank),
}

struct Block {
    modes: Vec<(Stage, StageMode)>,
    gate: EnableChange,
    mode: EnableChange,
    output: EnableChange,
    path_enable: u32,
    second_gate: u32,
    level: u32,
}

impl Default for Block {
    fn default() -> Self {
        Self {
            modes: Vec::new(),
            gate: EnableChange::Keep,
            mode: EnableChange::Keep,
            output: EnableChange::Keep,
            path_enable: 0,
            second_gate: 0,
            level: 0,
        }
    }
}

impl StageBlock for Block {
    fn stage_mode(&self, stage: Stage) -> StageMode { self.modes.iter().find(|(at, _)| *at == stage).map_or(StageMode::On, |(_, mode)| *mode) }

    fn enable_change(&self, enable: Enable) -> EnableChange {
        match enable {
            Enable::Gate => self.gate,
            Enable::Mode => self.mode,
            Enable::Output => self.output,
        }
    }

    fn setting(&self, setting: Setting) -> u32 {
        match setting {
            Setting::PathEnable => self.path_enable,
            Setting::SecondGate => self.second_gate,
            Setting::ModeLevel => self.level,
        }
    }
}

struct Stages {
    ops: Vec<Op>,
    gates: [u32; 2],
    path: [u32; 2],
    gate: u32,
    output: u32,
    mode: ModeWords,
    /// The part or stage whose write the unit refuses with -30.
    refused: Option<Part>,
    refused_stage: Option<Stage>,
}

fn stages() -> Stages {
    Stages {
        ops: Vec::new(),
        gates: [9, 8],
        path: [0b100, 0],
        gate: 0,
        output: 1,
        mode: ModeWords { enabled: 1, level: 3 },
        refused: None,
        refused_stage: None,
    }
}

/// Each unit's release status, so a caller can tell which release came last.
fn released(unit: Unit) -> i32 {
    match unit {
        Unit::Input => 11,
        Unit::Transform => 12,
        Unit::Processing => 13,
        Unit::Filter => 14,
    }
}

impl ImageStages for Stages {
    type Session = Unit;
    type Block = Block;

    fn acquire(&mut self, unit: Unit, _: Client) -> (i32, Unit) {
        self.ops.push(Op::Acquire(unit));
        (-1, unit)
    }
    fn release(&mut self, session: Unit) -> i32 {
        self.ops.push(Op::Release(session));
        released(session)
    }
    fn program(&mut self, session: &Unit, _: &Block, part: Part) -> i32 {
        self.ops.push(Op::Program(*session, part));
        if self.refused == Some(part) { -30 } else { -1 }
    }
    fn enable_input(&mut self, _: &Unit, enabled: bool) -> i32 {
        self.ops.push(Op::EnableInput(enabled));
        -1
    }
    fn set_stage(&mut self, _: &Unit, stage: Stage, mode: StageMode) -> i32 {
        self.ops.push(Op::Stage(stage, mode));
        if self.refused_stage == Some(stage) { -30 } else { -1 }
    }
    fn gates(&mut self, _: &Unit) -> (i32, [u32; 2]) { (-1, self.gates) }
    fn set_gates(&mut self, _: &Unit, words: [u32; 2]) -> i32 {
        self.gates = words;
        self.ops.push(Op::Gates(words));
        -1
    }
    fn path(&mut self, _: &Unit) -> (i32, [u32; 2]) { (-1, self.path) }
    fn set_path(&mut self, _: &Unit, words: [u32; 2]) -> i32 {
        self.path = words;
        self.ops.push(Op::Path(words));
        -1
    }
    fn switch(&mut self, _: &Unit, switch: Switch) -> (i32, u32) {
        match switch {
            Switch::Gate => (-1, self.gate),
            Switch::Output => (-1, self.output),
        }
    }
    fn set_switch(&mut self, _: &Unit, switch: Switch, word: u32) -> i32 {
        self.ops.push(Op::Switch(switch, word));
        -1
    }
    fn mode(&mut self, _: &Unit) -> (i32, ModeWords) { (-1, self.mode) }
    fn set_mode(&mut self, _: &Unit, mode: ModeWords) -> i32 {
        self.ops.push(Op::Mode(mode));
        -1
    }
    fn set_transform_offset(&mut self, _: &Unit, offset: [u16; 2], bank: Bank) -> i32 {
        self.ops.push(Op::Offset(offset, bank));
        -1
    }
    fn transform_window(&mut self, _: &Unit, _: Bank) -> (i32, [u16; 4]) { (-1, [1, 2, 3, 4]) }
    fn set_transform_window(&mut self, _: &Unit, window: [u16; 4], bank: Bank) -> i32 {
        self.ops.push(Op::Window(window, bank));
        -1
    }
    fn load_transform_table(&mut self, _: &Unit, table: &[u32; 31], bank: Bank) -> i32 {
        assert!(table.iter().all(|&word| word == 0x1_0000));
        self.ops.push(Op::Table(bank));
        -1
    }
    fn reset_input_unit(&mut self) -> i32 { -1 }
    fn unheld(&self, unit: Unit, _: Client) -> Unit { unit }
}

use Op::{Acquire, Program, Release};
use Unit::{Filter, Input, Processing, Transform};

fn client() -> Client { Client(1) }

fn flags(bits: u32) -> Flags { Flags(bits) }

#[test]
fn enabling_stages_holds_the_units_sets_modes_gates_and_path_and_releases_in_order() {
    let mut stages = stages();
    let block = Block { modes: vec![(Stage::Second, StageMode::Alternate)], ..Block::default() };
    assert_eq!(enable_configured_stages(&mut stages, client(), flags(0x1 | 0x4 | 0x8 | 0x20 | 0x40), &block), released(Processing));
    assert_eq!(
        stages.ops,
        [
            Acquire(Input),
            Acquire(Processing),
            Op::EnableInput(true),
            Op::Stage(Stage::Second, StageMode::Alternate),
            Op::Stage(Stage::Third, StageMode::On),
            Op::Gates([1, 8]),
            Op::Path([0b101, 0]),
            Release(Input),
            Release(Processing),
        ]
    );
}

#[test]
fn stage_400_alone_is_written_unheld_and_returns_the_writes_status() {
    let mut stages = Stages { refused_stage: Some(Stage::First), ..stages() };
    assert_eq!(enable_configured_stages(&mut stages, client(), flags(0x400), &Block::default()), -30);
    assert_eq!(stages.ops, [Op::Stage(Stage::First, StageMode::On)]);
}

#[test]
fn a_held_unit_makes_the_last_release_the_status() {
    let mut stages = Stages { refused_stage: Some(Stage::First), ..stages() };
    assert_eq!(enable_configured_stages(&mut stages, client(), flags(0x400 | 0x1), &Block::default()), released(Input));
    assert_eq!(stages.ops, [Acquire(Input), Op::EnableInput(true), Op::Stage(Stage::First, StageMode::On), Release(Input)]);
}

#[test]
fn disabling_stages_zeroes_modes_toggles_the_path_and_resets_named_banks() {
    let mut stages = stages();
    disable_stages(&mut stages, client(), flags(0x10 | 0x80 | 0x200));
    assert_eq!(
        stages.ops,
        [
            Acquire(Transform),
            Acquire(Processing),
            Op::Stage(Stage::Fourth, StageMode::Off),
            Op::Path([0b110, 0]),
            Release(Transform),
            Release(Processing),
            Acquire(Transform),
            Op::Offset([0x1000, 0x1000], Bank::Second),
            Op::Window([0, 0, 3, 4], Bank::Second),
            Op::Table(Bank::Second),
            Release(Transform),
        ]
    );
}

#[test]
fn a_region_write_programs_processing_and_input_then_the_filter() {
    let mut stages = stages();
    assert_eq!(write_region(&mut stages, client(), flags(6), &Block::default()), released(Filter));
    assert_eq!(
        stages.ops,
        [
            Acquire(Processing),
            Program(Processing, Part::ProcessingOrigin),
            Program(Processing, Part::ProcessingFormat),
            Release(Processing),
            Acquire(Input),
            Program(Input, Part::InputMode),
            Release(Input),
            Acquire(Filter),
            Program(Filter, Part::FilterSelectors),
            Release(Filter),
        ]
    );
}

#[test]
fn applying_parameters_programs_every_selected_group_in_order() {
    let mut stages = stages();
    let block = Block { gate: EnableChange::Invert, mode: EnableChange::Set(0), path_enable: 1, second_gate: 1, level: 5, ..Block::default() };
    assert_eq!(apply_parameters(&mut stages, client(), flags(0x7ff), &block), released(Processing));
    let bank = |bank| {
        [Part::TransformOffset(bank), Part::TransformThirdOffset(bank), Part::TransformWindow(bank), Part::TransformTable(bank)]
            .map(|part| Program(Transform, part))
    };
    let expected: Vec<Op> = [
        Acquire(Input),
        Acquire(Transform),
        Acquire(Processing),
        Program(Input, Part::InputControl),
        Program(Processing, Part::Division),
        Program(Processing, Part::Points),
        Op::Switch(Switch::Gate, 1),
        Program(Processing, Part::Pairs),
        Program(Processing, Part::SecondWindow),
        Program(Processing, Part::RotatedTables),
        Program(Processing, Part::SecondExtent),
        Program(Processing, Part::FirstTables),
        Program(Processing, Part::SecondTables),
        Op::Path([0b100, 1]),
        Op::Mode(ModeWords { enabled: 0, level: 5 }),
        Program(Processing, Part::Area),
        Program(Processing, Part::BlockLayout),
        Program(Input, Part::InputRegister3c),
    ]
    .into_iter()
    .chain(bank(Bank::First))
    .chain(bank(Bank::Second))
    .chain([
        Program(Processing, Part::Window),
        Program(Processing, Part::Curves),
        Op::Gates([9, 1]),
        Release(Input),
        Release(Transform),
        Release(Processing),
    ])
    .collect();
    assert_eq!(stages.ops, expected);
}

#[test]
fn a_kept_switch_is_neither_read_nor_written_and_a_kept_mode_is_written_back_with_the_level() {
    let mut stages = stages();
    let block = Block { level: 2, ..Block::default() };
    apply_parameters(&mut stages, client(), flags(0x4 | 0x80 | 0x20), &block);
    assert!(!stages.ops.iter().any(|op| matches!(op, Op::Switch(..))));
    assert!(stages.ops.contains(&Op::Mode(ModeWords { enabled: 1, level: 2 })));
}

#[test]
fn switch_changes_invert_the_current_word_or_set_the_block_word_unchanged() {
    let mut stages = stages();
    let block = Block { gate: EnableChange::Set(5), output: EnableChange::Invert, ..Block::default() };
    apply_parameters(&mut stages, client(), flags(0x4 | 0x20), &block);
    let switches: Vec<Op> = stages.ops.into_iter().filter(|op| matches!(op, Op::Switch(..))).collect();
    assert_eq!(switches, [Op::Switch(Switch::Gate, 5), Op::Switch(Switch::Output, 0)]);
}

#[test]
fn nothing_selected_holds_nothing_and_returns_0() {
    let mut stages = stages();
    assert_eq!(apply_parameters(&mut stages, client(), flags(0), &Block::default()), 0);
    assert_eq!(configure_geometry(&mut stages, client(), flags(0x8000), &Block::default()), 0);
    assert!(stages.ops.is_empty());
}

#[test]
fn the_division_alone_is_written_unheld_and_returns_its_status() {
    let mut stages = Stages { refused: Some(Part::Division), ..stages() };
    assert_eq!(apply_parameters(&mut stages, client(), flags(0x400), &Block::default()), -30);
    assert_eq!(stages.ops, [Program(Processing, Part::Division)]);
}

#[test]
fn configuring_geometry_programs_every_selected_group_in_order() {
    let mut stages = stages();
    assert_eq!(configure_geometry(&mut stages, client(), flags(0x7fff), &Block::default()), released(Transform));
    let bank = |bank| {
        [
            Part::TransformDimensions(bank),
            Part::TransformGeometry(bank),
            Part::TransformSecondOffset(bank),
            Part::TransformSecondaryGeometry(bank),
            Part::TransformReadback(bank),
            Part::TransformAddress(bank),
            Part::TransformNibble(bank),
        ]
        .map(|part| Program(Transform, part))
    };
    let expected: Vec<Op> = [Acquire(Filter), Acquire(Input), Acquire(Transform), Program(Input, Part::InputRegister14)]
        .into_iter()
        .chain(bank(Bank::First))
        .chain(bank(Bank::Second))
        .chain([
            Program(Input, Part::InputRegister34),
            Program(Filter, Part::FilterGrid(Grid::Second)),
            Program(Filter, Part::FilterSecondBank),
            Program(Filter, Part::FilterGrid(Grid::First)),
            Program(Filter, Part::FilterFirstBank),
            Program(Filter, Part::FilterGrid(Grid::Third)),
            Program(Filter, Part::FilterWindow(FilterWindow::First)),
            Program(Filter, Part::FilterWindow(FilterWindow::Second)),
            Program(Filter, Part::FilterWindow(FilterWindow::Third)),
            Program(Filter, Part::FilterWindow(FilterWindow::Fourth)),
            Program(Input, Part::InputRegister28),
            Program(Input, Part::InputRegister34Alternate),
            Program(Processing, Part::Extent),
            Release(Filter),
            Release(Input),
            Release(Transform),
        ])
        .collect();
    assert_eq!(stages.ops, expected);
}

#[test]
fn the_alternate_input_word_and_the_extent_are_written_unheld_and_the_last_returns_its_status() {
    let mut stages = Stages { refused: Some(Part::Extent), ..stages() };
    assert_eq!(configure_geometry(&mut stages, client(), flags(0x4000 | 0x2000), &Block::default()), -30);
    stages.refused = Some(Part::InputRegister34Alternate);
    assert_eq!(configure_geometry(&mut stages, client(), flags(0x4000), &Block::default()), -30);
    assert_eq!(configure_geometry(&mut stages, client(), flags(0x4000 | 0x2000), &Block::default()), -1);
    assert_eq!(
        stages.ops,
        [
            Program(Input, Part::InputRegister34Alternate),
            Program(Processing, Part::Extent),
            Program(Input, Part::InputRegister34Alternate),
            Program(Input, Part::InputRegister34Alternate),
            Program(Processing, Part::Extent),
        ]
    );
}
