use crate::engine::Inner;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

const KEY_TABLE: usize = 0x0082_8120;
const RELEASE_POLL_INSTRUCTIONS: u64 = 250_000;
/// Menu key names and their KEY_TABLE rows.
const KEY_ROWS: [(&str, usize); 10] = [
    ("up", 14),
    ("down", 15),
    ("left", 16),
    ("right", 17),
    ("ok", 18),
    ("q", 19),
    ("back", 20),
    ("play", 12),
    ("fn", 13),
    ("dial-push", 22),
];

pub(crate) struct KeyRow(usize);

pub(crate) enum ReleaseInput {
    Key(KeyRow),
    Subcommand,
}

impl ReleaseInput {
    pub(crate) fn parse(key: &str) -> PyResult<Self> {
        if matches!(key, "dial-left" | "dial-right") {
            return Ok(Self::Subcommand);
        }
        KEY_ROWS
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, row)| Self::Key(KeyRow(*row)))
            .ok_or_else(|| PyValueError::new_err("Unknown X-M1 menu input"))
    }

    fn released(&self, inner: &Inner) -> PyResult<bool> {
        let (address, expected) = match self {
            Self::Key(KeyRow(row)) => (KEY_TABLE + row * 28 + 0x15, 0),
            Self::Subcommand => (0x0095_5be9, 3),
        };
        if address >= inner.ram.len() {
            return Err(PyValueError::new_err("X-M1 input row is outside DRAM"));
        }
        // The owner thread pauses guest execution between release samples.
        Ok(unsafe { *inner.ram.as_ptr().add(address) } == expected)
    }
}

/// Whether every key row's debounced level reads released.
pub(crate) fn keys_released(inner: &Inner) -> PyResult<bool> {
    inner.paused().map_err(crate::error)?;
    for (_, row) in KEY_ROWS {
        if !ReleaseInput::Key(KeyRow(row)).released(inner)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn release(
    inner: &Inner,
    session: &Bound<'_, PyAny>,
    input: ReleaseInput,
    cap: u64,
) -> PyResult<()> {
    inner.paused().map_err(crate::error)?;
    if !(1..=15_000_000).contains(&cap) {
        return Err(PyValueError::new_err("Release cap must be 1..15M instructions"));
    }
    let board = session.getattr("board")?;
    let start = board.getattr("instructions")?.extract::<u64>()?;
    loop {
        let spent = board.getattr("instructions")?.extract::<u64>()? - start;
        if spent >= cap || input.released(inner)?
            || !board.getattr("failure")?.is_none()
            || !session.getattr("execution_error")?.is_none()
        {
            return Ok(());
        }
        session.call_method1("_advance", ((cap - spent).min(RELEASE_POLL_INSTRUCTIONS),))?;
        if board.getattr("instructions")?.extract::<u64>()? == start + spent {
            return Ok(());
        }
    }
}
