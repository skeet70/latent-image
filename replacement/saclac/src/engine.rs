#![forbid(unsafe_code)]

//! The saclac engine: one dispatch is one cell, and every effect leaves through `Ports`.
//!
//! Stock's dispatcher (0xefca6c) reads a machine's state and pending row, indexes the
//! machine's cell table, enters the cell's action word, then applies the cell's second
//! byte.

use core::num::NonZeroU32;

/// The number of machines stock's per-machine tables cover.
pub const MACHINE_COUNT: u32 = 37;

/// A machine's index into stock's four per-machine tables. Stock indexes with the raw
/// argument and never range-checks it; this type holds only `0..MACHINE_COUNT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Machine(u8);

impl Machine {
    /// The machine at `raw`, or `None` outside `0..MACHINE_COUNT`.
    pub const fn new(raw: u32) -> Option<Machine> {
        if raw < MACHINE_COUNT {
            Some(Machine(raw as u8))
        } else {
            None
        }
    }

    pub const fn index(self) -> u32 {
        self.0 as u32
    }
}

/// A machine's pending row, inside the machine's row count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row(u16);

impl Row {
    /// The row at `raw`, or `None` at or above the machine's row count.
    pub const fn new(raw: u16, count: u16) -> Option<Row> {
        if (raw as u32) < count as u32 {
            Some(Row(raw))
        } else {
            None
        }
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A machine's state, inside the machine's state count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State(u16);

impl State {
    /// The state at `raw`, or `None` at or above the machine's state count.
    pub const fn new(raw: u16, count: u16) -> Option<State> {
        if (raw as u32) < count as u32 {
            Some(State(raw))
        } else {
            None
        }
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A cell's index into its machine's cell table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellIndex(u16);

impl CellIndex {
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// An index into one of a machine's two code-word tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WordIndex(u8);

impl WordIndex {
    /// The index at `raw`, or `None` at or above the table's word count.
    pub const fn new(raw: u8, count: u32) -> Option<WordIndex> {
        if (raw as u32) < count {
            Some(WordIndex(raw))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// A stock code word: a nonzero address the boundary enters. It is the one value the
/// engine hands back to stock for execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StockCodeWord(NonZeroU32);

impl StockCodeWord {
    /// The word at a table entry; zero means no call, not a code word.
    pub fn new(word: u32) -> Option<Self> {
        NonZeroU32::new(word).map(Self)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

/// The cell a machine's state and row select, with stock's low-16-bit truncation
/// of `state + row * state_count`. The declared stock table dimensions fit in 16 bits.
pub fn cell_index(row: Row, state: State, state_count: u16) -> CellIndex {
    let index = (state.get() as u32) + (row.get() as u32) * (state_count as u32);
    CellIndex(index as u16)
}

/// A machine's cell table. Byte 0 of a cell is an action index; byte 1 is a signed state
/// delta or a second-table index.
pub trait CellSource {
    fn action(&self, index: CellIndex) -> u8;
    fn next(&self, index: CellIndex) -> i8;
}

/// A machine's table base word: what stock's dispatcher leaves in r2 when it enters a word from that table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableBase(u32);

impl TableBase {
    pub fn new(word: u32) -> Self {
        Self(word)
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

/// A table of code words indexed by a byte from a cell.
pub trait WordSource {
    fn base(&self) -> TableBase;
    fn word(&self, index: WordIndex) -> u32;
}

pub struct MachineTable<C, W> {
    pub cells: C,
    pub actions: W,
    pub seconds: W,
    pub state_count: u16,
    pub row_count: u16,
    pub cell_count: u16,
    pub action_count: u32,
    pub second_count: u32,
}

/// A machine's tables, or `None` when its cell-table word is zero.
pub trait Tables {
    type Cells: CellSource;
    type Words: WordSource;

    fn machine(&self, id: Machine) -> Option<MachineTable<Self::Cells, Self::Words>>;
}

/// What a dispatch reads and writes outside the tables, and what it enters.
pub trait Ports {
    fn row(&self, id: Machine) -> u16;
    fn state(&self, id: Machine) -> u16;
    fn set_row(&mut self, id: Machine, row: u16);
    fn set_state(&mut self, id: Machine, state: u16);
    fn trace(&mut self, id: Machine, state: State, row: Row);
    /// Enters `word` with `table` where stock's dispatcher has the table's base word in r2.
    fn enter(&mut self, word: StockCodeWord, table: TableBase, a1: u32, a2: u32) -> i32;
}

/// One dispatch from a raw machine word. A machine outside the tables returns zero
/// without dispatching, as a machine with no cell table does.
pub fn dispatch_raw<T: Tables, P: Ports>(
    tables: &T,
    ports: &mut P,
    machine: u32,
    a1: u32,
    a2: u32,
) -> i32 {
    match Machine::new(machine) {
        Some(machine) => dispatch(tables, ports, machine, a1, a2),
        None => 0,
    }
}

/// One dispatch. The result is the action's return word, or zero when the cell selects no
/// action. An action may call back into `dispatch`; nothing here is held across `enter`.
///
/// A machine, row, state or cell action index outside its table takes the no-dispatch
/// path and returns zero. A second-table index outside its table selects no second entry,
/// as a zero word does.
pub fn dispatch<T: Tables, P: Ports>(
    tables: &T,
    ports: &mut P,
    machine: Machine,
    a1: u32,
    a2: u32,
) -> i32 {
    let Some(table) = tables.machine(machine) else {
        return 0;
    };
    let Some(row) = Row::new(ports.row(machine), table.row_count) else {
        return 0;
    };
    let Some(state) = State::new(ports.state(machine), table.state_count) else {
        return 0;
    };
    ports.trace(machine, state, row);
    let index = cell_index(row, state, table.state_count);
    if index.get() >= table.cell_count {
        return 0;
    }
    let action = table.cells.action(index);
    let Some(action) = WordIndex::new(action, table.action_count) else {
        return 0;
    };
    let mut result = 0;
    if let Some(word) = StockCodeWord::new(table.actions.word(action)) {
        result = ports.enter(word, table.actions.base(), a1, a2);
    }
    // Stock reads the second byte after the action, from the cell address it computed
    // before the call.
    let next = table.cells.next(index);
    if next > 0 {
        if let Some(index) = WordIndex::new(next as u8, table.second_count) {
            if let Some(word) = StockCodeWord::new(table.seconds.word(index)) {
                ports.enter(word, table.seconds.base(), a1, a2);
            }
        }
    } else {
        ports.set_state(machine, (-(next as i32)) as u16);
    }
    result
}
