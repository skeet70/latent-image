//! Fuji LZ method-4 token decoder, the byte loop behind `FujiLZStream.feed`.
//!
//! The pure-Python decoder in `emulator/loader_devices.py` defines the
//! behavior; this performs the same token walk on the same `history` and
//! `pending` bytearrays, including their state after a malformed token
//! (history holds every token decoded before it, pending keeps all bytes).
//! It has no machine, memory, MMIO, queue, callback or IRQ access.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes, PySlice};

/// Decode what `pending + source` completes onto `history`.
///
/// Tokens: 0 is invalid; 1..=0x7f is a literal of that many bytes; a byte
/// with the top bit set and the next byte form `distance = (t & 0x7f) << 4 |
/// next >> 4` and `length = next & 15`, copying `length` bytes cyclically from
/// `distance` bytes back (a repeat of the distance-byte seed when `length >
/// distance`). A token split by the end of the data stays in `pending`.
/// Returns the bytes appended to `history`.
#[pyfunction]
pub fn fuji_lz_feed<'py>(
    py: Python<'py>,
    history: &Bound<'py, PyByteArray>,
    pending: &Bound<'py, PyByteArray>,
    source: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyBytes>> {
    pending.call_method1("extend", (source,))?;
    let pend = pending.to_vec();
    let mut out = history.to_vec();
    let start = out.len();
    let mut position = 0usize;
    let mut failure: Option<&'static str> = None;
    while position < pend.len() {
        let token = pend[position];
        if token == 0 {
            failure = Some("Zero-length Fuji literal");
            break;
        }
        if token < 0x80 {
            let stop = position + 1 + token as usize;
            if stop > pend.len() {
                break;
            }
            out.extend_from_slice(&pend[position + 1..stop]);
            position = stop;
        } else {
            if position + 2 > pend.len() {
                break;
            }
            let second = pend[position + 1];
            let distance = (((token & 0x7f) as usize) << 4) | (second >> 4) as usize;
            let length = (second & 15) as usize;
            if distance == 0 || distance > out.len() || length == 0 {
                failure = Some("Invalid Fuji LZ match");
                break;
            }
            let match_start = out.len() - distance;
            // Byte i repeats history[match_start + i % distance], read from
            // the history as it was before this token.
            for i in 0..length {
                out.push(out[match_start + i % distance]);
            }
            position += 2;
        }
    }
    let produced = PyBytes::new(py, &out[start..]);
    history.call_method1("extend", (&produced,))?;
    if let Some(message) = failure {
        return Err(PyValueError::new_err(message));
    }
    if position > 0 {
        pending.call_method1("__delitem__", (PySlice::new(py, 0, position as isize, 1),))?;
    }
    Ok(produced)
}
