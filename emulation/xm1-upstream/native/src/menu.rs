use crate::engine::Inner;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

const KEY_TABLE: usize = 0x0082_8120;
const RELEASE_POLL_INSTRUCTIONS: u64 = 250_000;
const KEY_ROWS: [KeyRow ; 13] = [
    KeyRow::Up, KeyRow::Down, KeyRow::Left, KeyRow::Right, KeyRow::Ok,
    KeyRow::Q, KeyRow::Back, KeyRow::Play, KeyRow::Fn, KeyRow::DialPush,
    KeyRow::HalfPress, KeyRow::FullPress, KeyRow::Movie,
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
    FullPress = 6,
    Movie = 21,
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
            Self::FullPress => "full-press",
            Self::Movie => "movie",
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
    Power,
    MainDial(MainDialDirection),
}

impl ReleaseInput {
    pub(crate) fn parse(key: &str) -> PyResult<Self> {
        match key {
            "dial-left" => Ok(Self::Subcommand(DialDirection::Left)),
            "dial-right" => Ok(Self::Subcommand(DialDirection::Right)),
            "power" => Ok(Self::Power),
            "main-dial-up" => Ok(Self::MainDial(MainDialDirection::Up)),
            "main-dial-down" => Ok(Self::MainDial(MainDialDirection::Down)),
            _ => KEY_ROWS.into_iter()
                .find(|row| row.name() == key)
                .map(Self::Key)
                .ok_or_else(|| PyValueError::new_err("Unknown X-M1 menu input")),
        }
    }

    fn released(&self, inner: &Inner) -> PyResult<bool> {
        if matches!(self, Self::MainDial(_)) {
            if inner.ram.len() < 0x0097_2ecd {
                return Err(PyValueError::new_err("X-M1 main dial is outside DRAM"));
            }
            let state: [u8; 5] = std::array::from_fn(|index| unsafe {
                *inner.ram.as_ptr().add(0x0097_2ec8 + index)
            });
            return Ok(state[0] == 1 && state[1] != 0 && state[1] == state[3] && state[4] >= 3);
        }
        let (address, expected) = match self {
            Self::Key(row) => (KEY_TABLE + *row as usize * 28 + 0x15, 0),
            Self::Subcommand(_) => (0x0095_5be9, 3),
            Self::Power => (KEY_TABLE + 11 * 28 + 0x15, 0),
            Self::MainDial(_) => unreachable!(),
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
            Self::Power => ("power", Some(11)),
            Self::MainDial(MainDialDirection::Up) => ("main-dial-up", None),
            Self::MainDial(MainDialDirection::Down) => ("main-dial-down", None),
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

/// Whether a key-table press latched when its physical hold ended. Other input mechanisms return None.
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
        ReleaseInput::Subcommand(_) | ReleaseInput::Power | ReleaseInput::MainDial(_) => None,
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
