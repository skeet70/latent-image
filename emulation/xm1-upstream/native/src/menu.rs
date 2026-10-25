use crate::engine::Inner;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

const KEY_TABLE: usize = 0x0082_8120;
const RELEASE_POLL_INSTRUCTIONS: u64 = 250_000;
const KEY_ROWS: [KeyRow ; 11] = [
    KeyRow::Up, KeyRow::Down, KeyRow::Left, KeyRow::Right, KeyRow::Ok,
    KeyRow::Q, KeyRow::Back, KeyRow::Play, KeyRow::Fn, KeyRow::DialPush,
    KeyRow::HalfPress,
];

#[derive(Clone, Copy)]
pub(crate) enum KeyRow {
    Up = 14,
    Down = 15,
    Left = 16,
    Right = 17,
    Ok = 18,
    Q = 19,
    Back = 20,
    Play = 12,
    Fn = 13,
    DialPush = 22,
    HalfPress = 5,
}

impl KeyRow {
    fn name(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::Ok => "ok",
            Self::Q => "q",
            Self::Back => "back",
            Self::Play => "play",
            Self::Fn => "fn",
            Self::DialPush => "dial-push",
            Self::HalfPress => "half-press",
        }
    }
}

pub(crate) enum DialDirection {
    Left,
    Right,
}

pub(crate) enum ReleaseInput {
    Key(KeyRow),
    Subcommand(DialDirection),
}

impl ReleaseInput {
    pub(crate) fn parse(key: &str) -> PyResult<Self> {
        match key {
            "dial-left" => Ok(Self::Subcommand(DialDirection::Left)),
            "dial-right" => Ok(Self::Subcommand(DialDirection::Right)),
            _ => KEY_ROWS.into_iter()
                .find(|row| row.name() == key)
                .map(Self::Key)
                .ok_or_else(|| PyValueError::new_err("Unknown X-M1 menu input")),
        }
    }

    fn released(&self, inner: &Inner) -> PyResult<bool> {
        let (address, expected) = match self {
            Self::Key(row) => (KEY_TABLE + *row as usize * 28 + 0x15, 0),
            Self::Subcommand(_) => (0x0095_5be9, 3),
        };
        if address >= inner.ram.len() {
            return Err(PyValueError::new_err("X-M1 input row is outside DRAM"));
        }
        // The owner thread pauses guest execution between release samples.
        Ok(unsafe { *inner.ram.as_ptr().add(address) } == expected)
    }

    fn observe_incomplete(
        &self,
        session: &Bound<'_, PyAny>,
        cap: u64,
        used: u64,
        instructions: u64,
    ) -> PyResult<()> {
        let (key, row) = match self {
            Self::Key(row) => (row.name(), Some(*row as usize)),
            Self::Subcommand(DialDirection::Left) => ("dial-left", None),
            Self::Subcommand(DialDirection::Right) => ("dial-right", None),
        };
        session.call_method1(
            "_observe_incomplete_key_release",
            (key, row, cap, used, instructions),
        )?;
        Ok(())
    }
}

/// Whether every key row's debounced level reads released.
pub(crate) fn keys_released(inner: &Inner) -> PyResult<bool> {
    inner.paused().map_err(crate::error)?;
    for row in KEY_ROWS {
        if !ReleaseInput::Key(row).released(inner)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether a key row's debounced level still read pressed when the physical hold ended; a row
/// already reading released never latched the press. None for the sub-command dial.
pub(crate) fn release(
    inner: &Inner,
    session: &Bound<'_, PyAny>,
    input: ReleaseInput,
    cap: u64,
) -> PyResult<Option<bool>> {
    inner.paused().map_err(crate::error)?;
    if !(1..=15_000_000).contains(&cap) {
        return Err(PyValueError::new_err("Release cap must be 1..15M instructions"));
    }
    let latched = match input {
        ReleaseInput::Key(_) => Some(!input.released(inner)?),
        ReleaseInput::Subcommand(_) => None,
    };
    let board = session.getattr("board")?;
    let start = board.getattr("instructions")?.extract::<u64>()?;
    loop {
        let instructions = board.getattr("instructions")?.extract::<u64>()?;
        let spent = instructions - start;
        if spent >= cap || input.released(inner)?
            || !board.getattr("failure")?.is_none()
            || !session.getattr("execution_error")?.is_none()
        {
            if !input.released(inner)? {
                input.observe_incomplete(session, cap, spent, instructions)?;
            }
            return Ok(latched);
        }
        session.call_method1("_advance", ((cap - spent).min(RELEASE_POLL_INSTRUCTIONS),))?;
        if board.getattr("instructions")?.extract::<u64>()? == instructions {
            if !input.released(inner)? {
                input.observe_incomplete(session, cap, spent, instructions)?;
            }
            return Ok(latched);
        }
    }
}
