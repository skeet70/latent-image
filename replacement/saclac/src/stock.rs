//! The stock boundary: the only module with guest addresses and stock memory layouts.

use crate::engine::{
    dispatch_raw, CellIndex, CellSource, Machine, MachineTable, Ports, Row, State, StockCodeWord,
    TableBase, Tables, WordIndex, WordSource,
};
use crate::machine0::route;

const CELL_TABLES: u32 = 0xf12f70;
const ACTION_TABLES: u32 = 0xf133ec;
const SECOND_TABLES: u32 = 0xf12074;
const STATE_COUNTS: u32 = 0xf1158c;
const ROWS: u32 = 0x169d468;
const STATES: u32 = 0x169d4b4;
const TRACE: u32 = 0xf46c38;

fn read_word(address: u32) -> u32 {
    unsafe { core::ptr::read_volatile(address as *const u32) }
}

fn read_word_indexed(base: u32, index: u32) -> u32 {
    unsafe { core::ptr::read_volatile((base as *const u32).add(index as usize)) }
}

fn read_half(address: u32) -> u16 {
    unsafe { core::ptr::read_volatile(address as *const u16) }
}

fn write_half(address: u32, value: u16) {
    unsafe { core::ptr::write_volatile(address as *mut u16, value) }
}

fn call3(word: u32, a: u32, b: u32, c: u32) -> i32 {
    let entry: extern "C" fn(u32, u32, u32) -> i32 =
        unsafe { core::mem::transmute(word as usize as *const ()) };
    entry(a, b, c)
}

fn entry(values: &[u32; 37], machine: Machine) -> u32 {
    values.get(machine.index() as usize).copied().unwrap_or(0)
}

/// Machine 0's event-to-row map, from the compare chain at 0xefcb88..0xefd2f8.
#[no_mangle]
#[used]
pub static SACLAC_ROW_MAP: [u16; 61] = [
    0, 1, 4, 5, 2, 3, 19, 20, 23, 24, 25, 26, 21, 16, 27, 28, 29, 30, 31, 32, 33, 34, 35, 6, 7,
    8, 9, 37, 10, 12, 11, 13, 14, 15, 18, 17, 38, 39, 42, 41, 22, 43, 44, 36, 45, 46, 47, 47, 47,
    47, 47, 47, 47, 47, 47, 47, 47, 47, 47, 47, 40,
];

/// Each machine's row count, from its cell table's bytes and state count.
#[no_mangle]
#[used]
pub static SACLAC_ROW_COUNTS: [u32; 37] = [
    48, 20, 9, 15, 5, 4, 8, 7, 28, 12, 6, 21, 51, 20, 6, 4, 7, 28, 8, 7, 3, 4, 13, 9, 8, 2, 52,
    9, 14, 6, 6, 6, 8, 4, 9, 6, 5,
];

/// Each machine's cell count, from its cell table's bytes.
#[no_mangle]
#[used]
pub static SACLAC_CELL_COUNTS: [u32; 37] = [
    528, 280, 54, 150, 15, 24, 24, 63, 84, 72, 42, 651, 1275, 120, 24, 16, 21, 504, 48, 49, 9, 8,
    104, 180, 40, 8, 676, 99, 252, 24, 24, 66, 56, 28, 144, 12, 20,
];

/// Each machine's action-table word count, from its table's bytes.
#[no_mangle]
#[used]
pub static SACLAC_ACTION_COUNTS: [u32; 37] = [
    46, 26, 13, 35, 8, 12, 11, 15, 45, 24, 12, 57, 95, 25, 13, 9, 14, 49, 17, 13, 8, 7, 33, 43,
    16, 8, 55, 24, 33, 13, 13, 14, 19, 16, 27, 11, 8,
];

/// Each machine's second-table word count, from its table's bytes.
#[no_mangle]
#[used]
pub static SACLAC_SECOND_COUNTS: [u32; 37] = [
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 2, 2,
];

struct RawCells {
    base: *const u8,
}

impl CellSource for RawCells {
    fn action(&self, index: CellIndex) -> u8 {
        unsafe { core::ptr::read_volatile(self.base.wrapping_add(index.get() as usize * 2)) }
    }

    fn next(&self, index: CellIndex) -> i8 {
        let raw = unsafe {
            core::ptr::read_volatile(self.base.wrapping_add(index.get() as usize * 2 + 1))
        };
        raw as i8
    }
}

struct RawWords {
    base: *const u32,
}

impl WordSource for RawWords {
    fn base(&self) -> TableBase {
        TableBase::new(self.base as u32)
    }

    fn word(&self, index: WordIndex) -> u32 {
        unsafe { core::ptr::read_volatile(self.base.wrapping_add(index.get() as usize)) }
    }
}

struct StockTables;

impl Tables for StockTables {
    type Cells = RawCells;
    type Words = RawWords;

    fn machine(&self, id: Machine) -> Option<MachineTable<RawCells, RawWords>> {
        let cells = read_word_indexed(CELL_TABLES, id.index());
        if cells == 0 {
            return None;
        }
        let actions = read_word(ACTION_TABLES + id.index() * 4);
        let seconds = read_word(SECOND_TABLES + id.index() * 4);
        let state_count = read_half(STATE_COUNTS + id.index() * 2);
        Some(MachineTable {
            cells: RawCells {
                base: cells as *const u8,
            },
            actions: RawWords {
                base: actions as *const u32,
            },
            seconds: RawWords {
                base: seconds as *const u32,
            },
            state_count,
            row_count: entry(&SACLAC_ROW_COUNTS, id) as u16,
            cell_count: entry(&SACLAC_CELL_COUNTS, id) as u16,
            action_count: entry(&SACLAC_ACTION_COUNTS, id),
            second_count: entry(&SACLAC_SECOND_COUNTS, id),
        })
    }
}

struct StockPorts;

impl Ports for StockPorts {
    fn row(&self, id: Machine) -> u16 {
        read_half(ROWS + id.index() * 2)
    }

    fn state(&self, id: Machine) -> u16 {
        read_half(STATES + id.index() * 2)
    }

    fn set_row(&mut self, id: Machine, row: u16) {
        write_half(ROWS + id.index() * 2, row)
    }

    fn set_state(&mut self, id: Machine, state: u16) {
        write_half(STATES + id.index() * 2, state)
    }

    fn trace(&mut self, id: Machine, state: State, row: Row) {
        call3(TRACE, id.index(), state.get() as u32, row.get() as u32);
    }

    fn enter(&mut self, word: StockCodeWord, table: TableBase, a1: u32, a2: u32) -> i32 {
        enter_word(a1, a2, table.get(), word.get())
    }
}

/// A stock code word called with four words: the callee sees the table base in r2 and the word in r3.
#[cfg(not(saclac_wrong_r3))]
#[inline(always)]
fn enter_word(a1: u32, a2: u32, table: u32, word: u32) -> i32 {
    let entry: extern "C" fn(u32, u32, u32, u32) -> i32 =
        unsafe { core::mem::transmute(word as usize as *const ()) };
    entry(a1, a2, table, word)
}

/// The comparison's wrong variant: r3 carries the table base.
#[cfg(saclac_wrong_r3)]
#[inline(always)]
fn enter_word(a1: u32, a2: u32, table: u32, word: u32) -> i32 {
    let entry: extern "C" fn(u32, u32, u32, u32) -> i32 =
        unsafe { core::mem::transmute(word as usize as *const ()) };
    entry(a1, a2, table, table)
}

/// The dispatcher entry retained stock callers enter at 0xefca6c.
#[no_mangle]
pub extern "C" fn xm1_saclac_dispatch(machine: u32, a1: u32, a2: u32) -> i32 {
    dispatch_raw(&StockTables, &mut StockPorts, machine, a1, a2)
}

/// Machine 0's router entry retained stock callers enter at 0xefcb88.
#[no_mangle]
pub extern "C" fn xm1_saclac_router(event: u32, a2: u32) -> i32 {
    route(&StockTables, &mut StockPorts, event, a2)
}
