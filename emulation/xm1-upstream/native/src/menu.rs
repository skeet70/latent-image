use crate::engine::Inner;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

const KEY_TABLE: usize = 0x0082_8120;
const RELEASE_POLL_INSTRUCTIONS: u64 = 250_000;
/// Menu key names and their KEY_TABLE rows.
const KEY_ROWS: [(&str, usize); 13] = [
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
    ("half-press", 5),
    ("full-press", 6),
    ("movie", 21),
];

pub(crate) struct KeyRow(usize);

pub(crate) enum ReleaseInput {
    Key(KeyRow),
    Subcommand,
    Power,
    MainDial,
}

impl ReleaseInput {
    pub(crate) fn parse(key: &str) -> PyResult<Self> {
        if matches!(key, "dial-left" | "dial-right") {
            return Ok(Self::Subcommand);
        }
        if key == "power" {
            return Ok(Self::Power);
        }
        if matches!(key, "main-dial-up" | "main-dial-down") {
            return Ok(Self::MainDial);
        }
        KEY_ROWS
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, row)| Self::Key(KeyRow(*row)))
            .ok_or_else(|| PyValueError::new_err("Unknown X-M1 menu input"))
    }

    fn released(&self, inner: &Inner) -> PyResult<bool> {
        if matches!(self, Self::MainDial) {
            if inner.ram.len() < 0x0097_2ecd {
                return Err(PyValueError::new_err("X-M1 main dial is outside DRAM"));
            }
            let state: [u8; 5] = std::array::from_fn(|index| unsafe {
                *inner.ram.as_ptr().add(0x0097_2ec8 + index)
            });
            return Ok(state[0] == 1 && state[1] != 0 && state[1] == state[3] && state[4] >= 3);
        }
        let (address, expected) = match self {
            Self::Key(KeyRow(row)) => (KEY_TABLE + row * 28 + 0x15, 0),
            Self::Subcommand => (0x0095_5be9, 3),
            Self::Power => (KEY_TABLE + 11 * 28 + 0x15, 0),
            Self::MainDial => unreachable!(),
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
        ReleaseInput::Subcommand | ReleaseInput::Power | ReleaseInput::MainDial => None,
    };
    let board = session.getattr("board")?;
    let start = board.getattr("instructions")?.extract::<u64>()?;
    loop {
        let spent = board.getattr("instructions")?.extract::<u64>()? - start;
        if spent >= cap || input.released(inner)?
            || !board.getattr("failure")?.is_none()
            || !session.getattr("execution_error")?.is_none()
        {
            return Ok(latched);
        }
        session.call_method1("_advance", ((cap - spent).min(RELEASE_POLL_INSTRUCTIONS),))?;
        if board.getattr("instructions")?.extract::<u64>()? == start + spent {
            return Ok(latched);
        }
    }
}

#[pyclass(eq, eq_int, frozen, from_py_object, module = "x70_native._native")]
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum MainDialDirection {
    Up,
    Down,
}

impl MainDialDirection {
    fn code(self) -> u32 {
        match self {
            Self::Up => 0x1d,
            Self::Down => 0x1e,
        }
    }
}

fn main_dial_target(
    thresholds: &[u16; 17],
    transitions: &[u8],
    previous: u8,
    current: u16,
    direction: MainDialDirection,
) -> PyResult<u16> {
    if thresholds.windows(2).any(|pair| pair[0] >= pair[1])
        || thresholds[16] > 1023 || current > 1023
        || transitions.len() != 17 * 17 * 4
    {
        return Err(PyValueError::new_err("X-M1 main dial calibration or transition table is invalid"));
    }
    let observed = thresholds.iter().position(|upper| current <= *upper)
        .ok_or_else(|| PyValueError::new_err("Main dial sample is outside its calibration"))?;
    if !(1..=16).contains(&previous) || observed != usize::from(previous) {
        return Err(PyValueError::new_err("Wait for the main dial scanner to sample its current ADC4 level"));
    }
    for next in [previous % 16 + 1, (previous + 14) % 16 + 1] {
        let index = (usize::from(next) * 17 + usize::from(previous)) * 4;
        let code = u32::from_le_bytes(transitions[index..index + 4].try_into().unwrap());
        if code == direction.code() {
            let low = thresholds[usize::from(next) - 1] + 1;
            let high = thresholds[usize::from(next)];
            return Ok(low + (high - low) / 2);
        }
    }
    Err(PyValueError::new_err("X-M1 main dial has no adjacent transition for that direction"))
}

#[pyfunction]
fn main_dial_sample(
    board: &Bound<'_, PyAny>,
    direction: MainDialDirection,
    current: u16,
) -> PyResult<u16> {
    use pyo3::types::PyBytes;
    let descriptor = board.call_method1("read_memory", (0x007c_b1f4_u32, 12))?;
    let descriptor = descriptor.cast::<PyBytes>()?.as_bytes();
    if descriptor != [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
        return Err(PyValueError::new_err("Main dial descriptor does not match X-M1 1.50"));
    }
    let state = board.call_method1("read_memory", (0x0097_2ec8_u32, 5))?;
    let state = state.cast::<PyBytes>()?.as_bytes();
    if state.len() != 5 || state[0] != 1 {
        return Err(PyValueError::new_err("X-M1 main dial scanner is not enabled"));
    }
    let thresholds = board.call_method1("read_memory", (0x0082_8510_u32, 34))?;
    let thresholds = thresholds.cast::<PyBytes>()?.as_bytes();
    if thresholds.len() != 34 {
        return Err(PyValueError::new_err("Truncated X-M1 main dial calibration"));
    }
    let thresholds = std::array::from_fn(|index| {
        u16::from_le_bytes([thresholds[index * 2], thresholds[index * 2 + 1]])
    });
    let table = board.call_method1("read_memory", (0x007c_b240_u32, 17 * 17 * 4))?;
    let table = table.cast::<PyBytes>()?.as_bytes();
    main_dial_target(&thresholds, table, state[3], current, direction)
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<MainDialDirection>()?;
    module.add_function(wrap_pyfunction!(main_dial_sample, module)?)
}
