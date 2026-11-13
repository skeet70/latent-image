#![forbid(unsafe_code)]

//! Machine 0's router: event code to row, then one dispatch of machine 0.

use crate::engine::{dispatch, Machine, Ports, Tables};
use crate::stock::SACLAC_ROW_MAP;

pub const MACHINE0: Machine = match Machine::new(0) {
    Some(machine) => machine,
    None => panic!("machine 0 is in range"),
};

const DEFAULT_ROW: u16 = 47;

pub fn row_for_event(event: u32) -> u16 {
    let index = event as usize;
    if index < SACLAC_ROW_MAP.len() {
        SACLAC_ROW_MAP[index]
    } else {
        DEFAULT_ROW
    }
}

/// Store the row, then dispatch; stock's router stores before the call.
pub fn route<T: Tables, P: Ports>(tables: &T, ports: &mut P, event: u32, a2: u32) -> i32 {
    ports.set_row(MACHINE0, row_for_event(event));
    dispatch(tables, ports, MACHINE0, event, a2)
}
