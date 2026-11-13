#![forbid(unsafe_code)]

use std::collections::VecDeque;

use crate::engine::{
    cell_index, dispatch_raw, CellSource, Machine, MachineTable, Ports, Row, State, StockCodeWord,
    TableBase, Tables, WordIndex, WordSource,
};
use crate::machine0::{route, row_for_event, MACHINE0};

fn machine(index: u32) -> Machine {
    Machine::new(index).expect("test machine index is inside the table")
}

struct Cells(Vec<u8>);

impl CellSource for Cells {
    fn action(&self, index: crate::engine::CellIndex) -> u8 {
        self.0[index.get() as usize * 2]
    }

    fn next(&self, index: crate::engine::CellIndex) -> i8 {
        self.0[index.get() as usize * 2 + 1] as i8
    }
}

struct Words {
    base: u32,
    values: Vec<u32>,
}

impl WordSource for Words {
    fn base(&self) -> TableBase {
        TableBase::new(self.base)
    }

    fn word(&self, index: WordIndex) -> u32 {
        self.values[index.get() as usize]
    }
}

struct MachineFixture {
    cells: Cells,
    actions: Words,
    seconds: Words,
    state_count: u16,
    row_count: u16,
    cell_count: u16,
    action_count: u32,
    second_count: u32,
}

struct TestTables {
    machines: Vec<Option<MachineFixture>>,
}

impl Tables for TestTables {
    type Cells = Cells;
    type Words = Words;

    fn machine(&self, id: Machine) -> Option<MachineTable<Cells, Words>> {
        let machine = self.machines.get(id.index() as usize)?.as_ref()?;
        Some(MachineTable {
            cells: Cells(machine.cells.0.clone()),
            actions: Words {
                base: machine.actions.base,
                values: machine.actions.values.clone(),
            },
            seconds: Words {
                base: machine.seconds.base,
                values: machine.seconds.values.clone(),
            },
            state_count: machine.state_count,
            row_count: machine.row_count,
            cell_count: machine.cell_count,
            action_count: machine.action_count,
            second_count: machine.second_count,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Effect {
    Trace(Machine, u16, u16),
    Enter(u32, u32, u32, u32),
}

enum Step {
    Return(i32),
    Route(u32, u32),
    Dispatch(Machine, u32, u32),
    Store(Machine, u16),
}

struct TestPorts<'a> {
    tables: &'a TestTables,
    rows: Vec<u16>,
    states: Vec<u16>,
    effects: Vec<Effect>,
    steps: VecDeque<Step>,
}

impl<'a> TestPorts<'a> {
    fn new(tables: &'a TestTables, rows: Vec<u16>, states: Vec<u16>) -> Self {
        Self {
            tables,
            rows,
            states,
            effects: Vec::new(),
            steps: VecDeque::new(),
        }
    }
}

impl Ports for TestPorts<'_> {
    fn row(&self, id: Machine) -> u16 {
        self.rows[id.index() as usize]
    }

    fn state(&self, id: Machine) -> u16 {
        self.states[id.index() as usize]
    }

    fn set_row(&mut self, id: Machine, row: u16) {
        self.rows[id.index() as usize] = row;
    }

    fn set_state(&mut self, id: Machine, state: u16) {
        self.states[id.index() as usize] = state;
    }

    fn trace(&mut self, id: Machine, state: State, row: Row) {
        self.effects.push(Effect::Trace(id, state.get(), row.get()));
    }

    fn enter(&mut self, word: StockCodeWord, table: TableBase, a1: u32, a2: u32) -> i32 {
        self.effects
            .push(Effect::Enter(word.get(), table.get(), a1, a2));
        let tables = self.tables;
        match self.steps.pop_front() {
            Some(Step::Return(value)) => value,
            Some(Step::Route(event, nested_a2)) => route(tables, self, event, nested_a2),
            Some(Step::Dispatch(machine, nested_a1, nested_a2)) => {
                dispatch_raw(tables, self, machine.index(), nested_a1, nested_a2)
            }
            Some(Step::Store(machine, value)) => {
                self.states[machine.index() as usize] = value;
                0
            }
            None => 0,
        }
    }
}

/// `states` states and `rows` rows of zeros, then the given `(state, row, action, next)` cells.
fn cells(states: u16, rows: u16, entries: &[(u16, u16, u8, i8)]) -> Cells {
    let mut bytes = vec![0u8; states as usize * rows as usize * 2];
    for &(state, row, action, next) in entries {
        let index = (state + row * states) as usize * 2;
        bytes[index] = action;
        bytes[index + 1] = next as u8;
    }
    Cells(bytes)
}

fn words(values: &[u32]) -> Words {
    table(0, values)
}

fn table(base: u32, values: &[u32]) -> Words {
    Words {
        base,
        values: values.to_vec(),
    }
}

fn tables(machines: Vec<Option<MachineFixture>>) -> TestTables {
    TestTables { machines }
}

fn fixture(
    cells: Cells,
    actions: Words,
    seconds: Words,
    state_count: u16,
    row_count: u16,
) -> MachineFixture {
    MachineFixture {
        cell_count: (cells.0.len() / 2) as u16,
        action_count: actions.values.len() as u32,
        second_count: seconds.values.len() as u32,
        cells,
        actions,
        seconds,
        state_count,
        row_count,
    }
}

fn one_machine(
    cells: Cells,
    actions: Words,
    seconds: Words,
    state_count: u16,
    row_count: u16,
) -> TestTables {
    tables(vec![Some(fixture(cells, actions, seconds, state_count, row_count))])
}

#[test]
fn a_cell_enters_its_action_and_returns_the_action_word() {
    let tables = one_machine(
        cells(2, 4, &[(0, 0, 3, 1)]),
        words(&[0, 0, 0, 0x1003]),
        words(&[0, 0]),
        2,
        4,
    );
    let mut ports = TestPorts::new(&tables, vec![0], vec![0]);
    ports.steps.push_back(Step::Return(7));
    assert_eq!(dispatch_raw(&tables, &mut ports, 0, 0x11, 0x22), 7);
    assert_eq!(
        ports.effects,
        vec![
            Effect::Trace(MACHINE0, 0, 0),
            Effect::Enter(0x1003, 0, 0x11, 0x22)
        ]
    );
    assert_eq!(ports.states, vec![0]);
}

#[test]
fn a_negative_second_byte_stores_the_negated_state_after_the_action() {
    let tables = one_machine(
        cells(2, 4, &[(0, 0, 3, -3)]),
        words(&[0, 0, 0, 0x1003]),
        words(&[0, 0]),
        2,
        4,
    );
    let mut ports = TestPorts::new(&tables, vec![0], vec![0]);
    ports.steps.push_back(Step::Return(7));
    assert_eq!(dispatch_raw(&tables, &mut ports, 0, 0, 0), 7);
    assert_eq!(ports.states, vec![3]);
}

#[test]
fn a_zero_action_word_enters_nothing_and_returns_zero() {
    let tables = one_machine(
        cells(2, 4, &[(0, 0, 0, 1)]),
        words(&[0, 0]),
        words(&[0, 0]),
        2,
        4,
    );
    let mut ports = TestPorts::new(&tables, vec![0], vec![0]);
    assert_eq!(dispatch_raw(&tables, &mut ports, 0, 0, 0), 0);
    assert_eq!(ports.effects, vec![Effect::Trace(MACHINE0, 0, 0)]);
}

#[test]
fn a_machine_without_a_cell_table_returns_zero_before_tracing() {
    let tables = tables(vec![
        Some(fixture(cells(2, 4, &[]), words(&[0]), words(&[0]), 2, 4)),
        None,
    ]);
    let mut ports = TestPorts::new(&tables, vec![0, 0], vec![0, 0]);
    assert_eq!(dispatch_raw(&tables, &mut ports, 1, 0, 0), 0);
    assert!(ports.effects.is_empty());
}

#[test]
fn a_machine_number_outside_the_tables_returns_zero_without_effects() {
    let tables = one_machine(cells(2, 4, &[(0, 0, 1, 1)]), words(&[0, 1]), words(&[0, 0]), 2, 4);
    let mut ports = TestPorts::new(&tables, vec![0], vec![0]);
    assert_eq!(dispatch_raw(&tables, &mut ports, 37, 0, 0), 0);
    assert_eq!(dispatch_raw(&tables, &mut ports, u32::MAX, 0, 0), 0);
    assert!(ports.effects.is_empty());
}

#[test]
fn a_row_at_its_machines_row_count_returns_zero_without_effects() {
    let tables = one_machine(cells(2, 4, &[(0, 0, 1, 1)]), words(&[0, 1]), words(&[0, 0]), 2, 4);
    let mut ports = TestPorts::new(&tables, vec![4], vec![0]);
    assert_eq!(dispatch_raw(&tables, &mut ports, 0, 0, 0), 0);
    assert!(ports.effects.is_empty());
    assert_eq!(ports.states, vec![0]);
}

#[test]
fn a_state_at_its_machines_state_count_returns_zero_without_effects() {
    let tables = one_machine(cells(2, 4, &[(0, 0, 1, 1)]), words(&[0, 1]), words(&[0, 0]), 2, 4);
    let mut ports = TestPorts::new(&tables, vec![0], vec![2]);
    assert_eq!(dispatch_raw(&tables, &mut ports, 0, 0, 0), 0);
    assert!(ports.effects.is_empty());
    assert_eq!(ports.states, vec![2]);
}

#[test]
fn an_action_index_at_the_action_table_length_returns_zero_without_entering() {
    let tables = one_machine(
        cells(2, 4, &[(0, 0, 2, 1)]),
        words(&[0, 0x1001]),
        words(&[0, 0]),
        2,
        4,
    );
    let mut ports = TestPorts::new(&tables, vec![0], vec![0]);
    assert_eq!(dispatch_raw(&tables, &mut ports, 0, 0, 0), 0);
    assert_eq!(ports.effects, vec![Effect::Trace(MACHINE0, 0, 0)]);
    assert_eq!(ports.states, vec![0]);
}

#[test]
fn a_cell_index_at_the_cell_table_length_returns_zero_without_entering() {
    let mut machine = fixture(
        cells(2, 4, &[(1, 3, 1, 1)]),
        words(&[0, 0x1001]),
        words(&[0, 0]),
        2,
        4,
    );
    machine.cell_count = 7;
    let tables = tables(vec![Some(machine)]);
    let mut ports = TestPorts::new(&tables, vec![3], vec![1]);
    assert_eq!(dispatch_raw(&tables, &mut ports, 0, 0, 0), 0);
    assert_eq!(ports.effects, vec![Effect::Trace(MACHINE0, 1, 3)]);
    assert_eq!(ports.states, vec![1]);
}

#[test]
fn a_second_index_at_the_second_table_length_enters_no_second_word() {
    let tables = one_machine(
        cells(2, 4, &[(0, 0, 1, 2)]),
        table(0x0f13_3ec, &[0, 0x1001]),
        table(0x0f12_074, &[0, 0x2001]),
        2,
        4,
    );
    let mut ports = TestPorts::new(&tables, vec![0], vec![0]);
    ports.steps.push_back(Step::Return(9));
    assert_eq!(dispatch_raw(&tables, &mut ports, 0, 0, 0), 9);
    assert_eq!(
        ports.effects,
        vec![
            Effect::Trace(MACHINE0, 0, 0),
            Effect::Enter(0x1001, 0x0f13_3ec, 0, 0),
        ]
    );
}

#[test]
fn the_cell_index_is_the_stock_formula_for_an_in_range_row_and_state() {
    assert_eq!(cell_index(Row::new(3, 4).unwrap(), State::new(2, 4).unwrap(), 2).get(), 8);
    assert_eq!(cell_index(Row::new(0, 4).unwrap(), State::new(0, 4).unwrap(), 2).get(), 0);
}

#[test]
fn a_positive_second_byte_enters_the_second_table_word_and_keeps_the_first_result() {
    let tables = one_machine(
        cells(2, 4, &[(0, 0, 3, 1)]),
        table(0x0f13_3ec, &[0, 0, 0, 0x1003]),
        table(0x0f12_074, &[0, 0x2001]),
        2,
        4,
    );
    let mut ports = TestPorts::new(&tables, vec![0], vec![0]);
    ports.steps.push_back(Step::Return(9));
    ports.steps.push_back(Step::Return(4));
    assert_eq!(dispatch_raw(&tables, &mut ports, 0, 0x11, 0x22), 9);
    assert_eq!(
        ports.effects,
        vec![
            Effect::Trace(MACHINE0, 0, 0),
            Effect::Enter(0x1003, 0x0f13_3ec, 0x11, 0x22),
            Effect::Enter(0x2001, 0x0f12_074, 0x11, 0x22),
        ]
    );
    assert_eq!(ports.states, vec![0]);
}

#[test]
fn a_zero_action_word_still_enters_the_second_word_with_the_second_table_base() {
    let tables = one_machine(
        cells(2, 4, &[(0, 0, 0, 1)]),
        table(0x0f13_3ec, &[0, 0]),
        table(0x0f12_074, &[0, 0x2001]),
        2,
        4,
    );
    let mut ports = TestPorts::new(&tables, vec![0], vec![0]);
    ports.steps.push_back(Step::Return(4));
    assert_eq!(dispatch_raw(&tables, &mut ports, 0, 0x11, 0x22), 0);
    assert_eq!(
        ports.effects,
        vec![
            Effect::Trace(MACHINE0, 0, 0),
            Effect::Enter(0x2001, 0x0f12_074, 0x11, 0x22),
        ]
    );
}

#[test]
fn a_negative_second_byte_enters_the_action_with_the_action_table_base_only() {
    let tables = one_machine(
        cells(2, 4, &[(0, 0, 3, -3)]),
        table(0x0f13_3ec, &[0, 0, 0, 0x1003]),
        table(0x0f12_074, &[0, 0x2001]),
        2,
        4,
    );
    let mut ports = TestPorts::new(&tables, vec![0], vec![0]);
    ports.steps.push_back(Step::Return(7));
    assert_eq!(dispatch_raw(&tables, &mut ports, 0, 0x11, 0x22), 7);
    assert_eq!(
        ports.effects,
        vec![
            Effect::Trace(MACHINE0, 0, 0),
            Effect::Enter(0x1003, 0x0f13_3ec, 0x11, 0x22),
        ]
    );
    assert_eq!(ports.states, vec![3]);
}

#[test]
fn an_action_can_reenter_its_own_machine_through_the_router() {
    let tables = one_machine(
        cells(2, 2, &[(0, 0, 1, -2), (0, 1, 2, 1)]),
        words(&[0, 0x1001, 0x1002]),
        words(&[0, 0]),
        2,
        2,
    );
    let mut ports = TestPorts::new(&tables, vec![0], vec![0]);
    ports.steps.push_back(Step::Route(1, 0x55));
    ports.steps.push_back(Step::Return(5));
    assert_eq!(dispatch_raw(&tables, &mut ports, 0, 0x11, 0x22), 5);
    assert_eq!(ports.rows, vec![1]);
    assert_eq!(ports.states, vec![2]);
    assert_eq!(
        ports.effects,
        vec![
            Effect::Trace(MACHINE0, 0, 0),
            Effect::Enter(0x1001, 0, 0x11, 0x22),
            Effect::Trace(MACHINE0, 0, 1),
            Effect::Enter(0x1002, 0, 1, 0x55),
        ]
    );
}

#[test]
fn an_action_can_dispatch_another_machine() {
    let tables = tables(vec![
        Some(fixture(cells(2, 2, &[(0, 0, 1, 1)]), words(&[0, 0x1001]), words(&[0, 0]), 2, 2)),
        Some(fixture(cells(1, 1, &[(0, 0, 1, -1)]), words(&[0, 0x3001]), words(&[0]), 1, 1)),
    ]);
    let mut ports = TestPorts::new(&tables, vec![0, 0], vec![0, 0]);
    ports
        .steps
        .push_back(Step::Dispatch(machine(1), 0x77, 0x88));
    ports.steps.push_back(Step::Return(8));
    assert_eq!(dispatch_raw(&tables, &mut ports, 0, 0x11, 0x22), 8);
    assert_eq!(ports.states, vec![0, 1]);
    assert_eq!(
        ports.effects,
        vec![
            Effect::Trace(MACHINE0, 0, 0),
            Effect::Enter(0x1001, 0, 0x11, 0x22),
            Effect::Trace(machine(1), 0, 0),
            Effect::Enter(0x3001, 0, 0x77, 0x88),
        ]
    );
}

#[test]
fn the_outer_store_replaces_a_state_its_action_stored() {
    // Machine 0 at state 1, row 0: cell (1, 0) is action 1 with next -1, so the
    // dispatch stores 1 after the action. The action stores 7 first.
    let tables = one_machine(
        cells(2, 2, &[(1, 0, 1, -1)]),
        words(&[0, 0x1001]),
        words(&[0, 0]),
        2,
        2,
    );
    let mut ports = TestPorts::new(&tables, vec![0], vec![1]);
    ports.steps.push_back(Step::Store(MACHINE0, 7));
    assert_eq!(dispatch_raw(&tables, &mut ports, 0, 0x11, 0x22), 0);
    assert_eq!(ports.states, vec![1]);
}

#[test]
fn a_nested_machines_store_and_the_outer_store_stay_on_their_own_machines() {
    let tables = tables(vec![
        Some(fixture(cells(2, 2, &[(1, 0, 1, -1)]), words(&[0, 0x1001]), words(&[0, 0]), 2, 2)),
        Some(fixture(cells(1, 1, &[(0, 0, 1, -5)]), words(&[0, 0x3001]), words(&[0]), 1, 1)),
    ]);
    let mut ports = TestPorts::new(&tables, vec![0, 0], vec![1, 0]);
    ports
        .steps
        .push_back(Step::Dispatch(machine(1), 0x77, 0x88));
    ports.steps.push_back(Step::Return(8));
    assert_eq!(dispatch_raw(&tables, &mut ports, 0, 0x11, 0x22), 8);
    assert_eq!(ports.states, vec![1, 5]);
}

#[test]
fn the_router_maps_every_named_event_and_defaults_the_rest() {
    let map = [
        (0u32, 0u16),
        (1, 1),
        (2, 4),
        (3, 5),
        (4, 2),
        (5, 3),
        (6, 19),
        (7, 20),
        (8, 23),
        (9, 24),
        (10, 25),
        (11, 26),
        (12, 21),
        (13, 16),
        (14, 27),
        (15, 28),
        (16, 29),
        (17, 30),
        (18, 31),
        (19, 32),
        (20, 33),
        (21, 34),
        (22, 35),
        (23, 6),
        (24, 7),
        (25, 8),
        (26, 9),
        (27, 37),
        (28, 10),
        (29, 12),
        (30, 11),
        (31, 13),
        (32, 14),
        (33, 15),
        (34, 18),
        (35, 17),
        (36, 38),
        (37, 39),
        (38, 42),
        (39, 41),
        (40, 22),
        (41, 43),
        (42, 44),
        (43, 36),
        (44, 45),
        (45, 46),
        (60, 40),
    ];
    for (event, row) in map {
        assert_eq!(row_for_event(event), row, "event {event}");
    }
    for event in [46, 47, 48, 59, 61, 255, 0x10000, u32::MAX] {
        assert_eq!(row_for_event(event), 47, "event {event}");
    }
}

#[test]
fn the_router_stores_the_row_before_dispatching() {
    let tables = one_machine(
        cells(1, 8, &[(0, 6, 1, 1)]),
        words(&[0, 0x1001]),
        words(&[0, 0]),
        1,
        8,
    );
    let mut ports = TestPorts::new(&tables, vec![0], vec![0]);
    ports.steps.push_back(Step::Return(3));
    assert_eq!(route(&tables, &mut ports, 23, 0x9), 3);
    assert_eq!(ports.rows, vec![6]);
    assert_eq!(
        ports.effects,
        vec![Effect::Trace(MACHINE0, 0, 6), Effect::Enter(0x1001, 0, 23, 0x9)]
    );
}
