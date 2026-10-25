use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyModule};

use crate::mode_devices::{exact_int, unsupported, word};
use crate::raster::DeclaredRasterPlan;
use crate::scene_scaler::Plan;

const BASE: u64 = 0xfff4_7000;
const PROFILE: &str = "xm1-150-declared-scene-opaque16-nearest-unmeasured";
const OFFSETS: [u32; 17] = [
    0x10, 0x14, 0x18, 0x20, 0x24, 0x28, 0x2c,
    0x40, 0x44, 0x48, 0x4c, 0x50, 0x54, 0x58, 0x5c, 0x60, 0x64,
];
const MASKS: [u32; 17] = [
    0x3fff_ffff, 0x7fff, 0x1fff_1fff, 0x3fff_ffff, 0x7fff, 0x1fff_1fff,
    0x1fff_1fff, 0x3ffff, 0x3ffff, 0xf03ff, 0x3fff_ffff,
    0x3ffff, 0x3ffff, 0xf03ff, 0x3fff_ffff, 0x1fff_1fff, 0x1fff_1fff,
];
const COMPLETE_WORDS: u32 = (1 << 17) - 1;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase { Programming, Active, Completed, Acknowledged }
impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Programming => "programming", Self::Active => "active",
            Self::Completed => "completed-latched", Self::Acknowledged => "acknowledged",
        }
    }
    fn parse(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        match value.extract::<&str>()? {
            "programming" => Ok(Self::Programming), "active" => Ok(Self::Active),
            "completed-latched" => Ok(Self::Completed), "acknowledged" => Ok(Self::Acknowledged),
            _ => Err(PyValueError::new_err("Unknown scene scaler phase")),
        }
    }
}

#[derive(Clone, Copy)]
enum Core { Primary, Secondary }
impl Core {
    fn parse(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        match exact_int(value) {
            Some(0) => Ok(Self::Primary), Some(1) => Ok(Self::Secondary),
            _ => Err(PyValueError::new_err("Scene scaler requires CPU 0 or 1")),
        }
    }
    fn code(self) -> u8 { match self { Self::Primary => 0, Self::Secondary => 1 } }
}

fn field<'py>(state: &Bound<'py, PyDict>, name: &str) -> PyResult<Bound<'py, PyAny>> {
    state.get_item(name)?.ok_or_else(|| PyValueError::new_err("Incomplete scene scaler state"))
}

fn selected(device: &Bound<'_, PyAny>) -> PyResult<bool> {
    let profile = device.getattr("scene_profile")?;
    if profile.is_none() { return Ok(false); }
    if !profile.eq(crate::mode_devices::PROFILE)? {
        return Err(PyValueError::new_err("Unknown scene scaler selection"));
    }
    Ok(true)
}

fn state<'py>(device: &Bound<'py, PyAny>) -> PyResult<Option<Bound<'py, PyDict>>> {
    let state = device.getattr("scene_state")?;
    if state.is_none() { return Ok(None); }
    if !selected(device)? {
        return Err(PyValueError::new_err("Scene scaler state requires its named profile"));
    }
    Ok(Some(state.cast_into::<PyDict>()?))
}

fn access(core: &Bound<'_, PyAny>, address: u64, size: u64) -> PyResult<u64> {
    Core::parse(core)?;
    if size != 4 || address & 3 != 0 {
        return Err(PyValueError::new_err("Scene scaler requires an aligned unsigned word"));
    }
    address.checked_sub(BASE).filter(|offset| *offset < 0x1000)
        .ok_or_else(|| PyValueError::new_err("Scene scaler address is outside FFF470"))
}

fn programming(state: &Bound<'_, PyDict>) -> PyResult<[u32; 17]> {
    let saved = field(state, "words")?.cast_into::<PyList>()?;
    if saved.len() != 17 {
        return Err(PyValueError::new_err("Scene scaler requires seventeen programming words"));
    }
    let mut words = [0u32; 17];
    for (index, value) in saved.iter().enumerate() {
        words[index] = word(&value).filter(|value| value & !MASKS[index] == 0)
            .ok_or_else(|| PyValueError::new_err("Scene scaler register exceeds its recovered field mask"))?;
    }
    Ok(words)
}

fn fresh_state(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    let state = PyDict::new(py);
    state.set_item("phase", Phase::Programming.name())?;
    state.set_item("words", PyList::new(py, [0u32; 17])?)?;
    state.set_item("written", 0)?;
    state.set_item("auxiliary", false)?;
    state.set_item("mask", 0)?;
    state.set_item("cleared", false)?;
    state.set_item("deadline", py.None())?;
    state.set_item("origin", py.None())?;
    state.set_item("route", py.None())?;
    state.set_item("plan", py.None())?;
    state.set_item("failed", false)?;
    Ok(state)
}

#[pyfunction]
pub(crate) fn scene_scaler_busy(device: &Bound<'_, PyAny>) -> PyResult<bool> {
    match state(device)? {
        Some(state) => Ok(Phase::parse(&field(&state, "phase")?)? == Phase::Active),
        None => Ok(false),
    }
}

#[pyfunction]
pub(crate) fn scene_scaler_read(
    device: &Bound<'_, PyAny>, core: &Bound<'_, PyAny>, address: u64, size: u64,
) -> PyResult<Option<u32>> {
    let Some(state) = state(device)? else { return Ok(None); };
    let offset = access(core, address, size)?;
    if !matches!(offset, 0x74 | 0x78) {
        return Err(unsupported(device.py(), "Scene scaler has no recovered read for this register"));
    }
    let completed = Phase::parse(&field(&state, "phase")?)? == Phase::Completed;
    Ok(Some(u32::from(completed)))
}

#[pyfunction]
pub(crate) fn scene_scaler_write(
    device: &Bound<'_, PyAny>, core: &Bound<'_, PyAny>, address: u64, size: u64,
    value: &Bound<'_, PyAny>,
) -> PyResult<bool> {
    if !selected(device)? { return Ok(false); }
    let current = state(device)?;
    if current.is_none() && (address != BASE + 8 || word(value) != Some(0)) {
        return Ok(false);
    }
    let offset = access(core, address, size)?;
    let value = word(value).ok_or_else(|| unsupported(device.py(), "Scene scaler requires an unsigned word"))?;
    let Some(state) = current else {
        if !device.getattr("reset_seen")?.extract::<bool>()?
            || device.getattr("phase")?.getattr("value")?.extract::<&str>()? != "idle"
        {
            return Err(unsupported(device.py(), "Scene scaler selector requires a fresh idle-zero generation"));
        }
        device.setattr("scene_state", fresh_state(device.py())?)?;
        return Ok(true);
    };
    let phase = Phase::parse(&field(&state, "phase")?)?;
    if phase == Phase::Active {
        return Err(unsupported(device.py(), "Scene scaler active programming cannot change"));
    }
    if offset == 0 {
        if value != 0 || phase != Phase::Acknowledged {
            return Err(unsupported(device.py(), "Scene scaler reset requires completed acknowledgement"));
        }
        device.setattr("scene_state", device.py().None())?;
        return Ok(false);
    }
    if offset == 0x78 {
        if value & !0x1001 != 0 {
            return Err(unsupported(device.py(), "Scene scaler acknowledgement exceeds the recovered 0x1001 mask"));
        }
        if phase == Phase::Programming && value == 0x1001 {
            state.set_item("cleared", true)?;
        } else if phase == Phase::Completed && value & 1 != 0 {
            state.set_item("phase", Phase::Acknowledged.name())?;
        } else {
            return Err(unsupported(device.py(), "Scene scaler acknowledgement is outside its generation"));
        }
        return Ok(true);
    }
    if phase == Phase::Acknowledged {
        if offset == 0x7c && value == 0 {
            state.set_item("mask", 0)?;
            return Ok(true);
        }
        return Err(unsupported(device.py(), "Scene scaler acknowledged cleanup requires mask zero or reset"));
    }
    if phase != Phase::Programming {
        return Err(unsupported(device.py(), "Scene scaler completed generation requires acknowledgement"));
    }
    match offset {
        4 => {
            if value != 1 || field(&state, "written")?.extract::<u32>()? != COMPLETE_WORDS
                || !field(&state, "auxiliary")?.extract::<bool>()?
                || !field(&state, "cleared")?.extract::<bool>()?
                || field(&state, "mask")?.extract::<u32>()? != 0x1001
            {
                return Err(unsupported(device.py(), "Scene scaler start requires complete selector-zero command-one programming"));
            }
            let board = device.getattr("board")?;
            let plan = Plan::new(programming(&state)?)?;
            plan.validate_bounds(board.getattr("ram")?.len()?).map_err(PyValueError::new_err)?;
            let deadline = exact_int(&board.getattr("virtual_ticks")?)
                .and_then(|tick| tick.checked_add(1)).ok_or_else(|| PyValueError::new_err("Scene scaler deadline overflow"))?;
            let origin = Core::parse(core)?;
            let route = Core::parse(&board.getattr("interrupts")?.call_method0("declared_ui_irq_target")?.getattr("value")?)?;
            state.set_item("plan", Py::new(device.py(), plan)?)?;
            state.set_item("deadline", deadline)?;
            state.set_item("origin", origin.code())?;
            state.set_item("route", route.code())?;
            state.set_item("phase", Phase::Active.name())?;
        }
        8 if value == 0 => {}
        0x70 if value == 0 => state.set_item("auxiliary", true)?,
        0x7c if matches!(value, 0 | 0x1001) => state.set_item("mask", value)?,
        _ => {
            let index = OFFSETS.iter().position(|candidate| u64::from(*candidate) == offset)
                .ok_or_else(|| unsupported(device.py(), "Scene scaler register is outside its recovered family"))?;
            if value & !MASKS[index] != 0 {
                return Err(unsupported(device.py(), "Scene scaler value exceeds the recovered field mask"));
            }
            field(&state, "words")?.set_item(index, value)?;
            let written = field(&state, "written")?.extract::<u32>()?;
            state.set_item("written", written | 1 << index)?;
        }
    }
    Ok(true)
}

#[pyfunction]
pub(crate) fn scene_scaler_next(device: &Bound<'_, PyAny>, now: u64) -> PyResult<Option<u64>> {
    let Some(state) = state(device)? else { return Ok(None); };
    if Phase::parse(&field(&state, "phase")?)? != Phase::Active
        || field(&state, "failed")?.extract::<bool>()?
    {
        return Ok(None);
    }
    let deadline = exact_int(&field(&state, "deadline")?)
        .ok_or_else(|| PyValueError::new_err("Invalid scene scaler deadline"))?;
    Ok(Some(now.max(deadline)))
}

#[pyfunction]
pub(crate) fn scene_scaler_tick(device: &Bound<'_, PyAny>, board: &Bound<'_, PyAny>) -> PyResult<bool> {
    let Some(state) = state(device)? else { return Ok(false); };
    if Phase::parse(&field(&state, "phase")?)? != Phase::Active { return Ok(true); }
    if field(&state, "failed")?.extract::<bool>()? {
        return Err(unsupported(device.py(), "Scene scaler active failure is frozen"));
    }
    let deadline = exact_int(&field(&state, "deadline")?)
        .ok_or_else(|| PyValueError::new_err("Invalid scene scaler deadline"))?;
    let now = exact_int(&board.getattr("virtual_ticks")?)
        .ok_or_else(|| PyValueError::new_err("Invalid scene scaler clock"))?;
    if now < deadline { return Ok(true); }
    let publish = || -> PyResult<()> {
        let origin = Core::parse(&field(&state, "origin")?)?;
        let route = Core::parse(&field(&state, "route")?)?;
        let live_route = Core::parse(&board.getattr("interrupts")?.call_method0("declared_ui_irq_target")?.getattr("value")?)?;
        if route.code() != live_route.code() {
            return Err(unsupported(device.py(), "Scene scaler IRQ67 route changed after start"));
        }
        board.call_method1("apply_declared_ui_conversion", (field(&state, "plan")?, origin.code()))?;
        state.set_item("plan", device.py().None())?;
        state.set_item("phase", Phase::Completed.name())?;
        board.call_method1("queue_irq", (route.code(), 67))?;
        state.set_item("deadline", device.py().None())?;
        Ok(())
    };
    if let Err(error) = publish() {
        state.set_item("phase", Phase::Active.name())?;
        state.set_item("failed", true)?;
        state.set_item("plan", device.py().None())?;
        return Err(error);
    }
    Ok(true)
}

#[pyfunction]
pub(crate) fn scene_scaler_validate(device: &Bound<'_, PyAny>, captured: &Bound<'_, PyDict>) -> PyResult<()> {
    if !field(captured, "scene_profile")?.eq(device.getattr("scene_profile")?)? {
        return Err(PyValueError::new_err("Incoming scene scaler profile differs"));
    }
    let saved = field(captured, "scene_state")?;
    if saved.is_none() { return Ok(()); }
    if !selected(device)? { return Err(PyValueError::new_err("Scene scaler state requires its named profile")); }
    let state = saved.cast::<PyDict>()?;
    let phase = Phase::parse(&field(state, "phase")?)?;
    let words = programming(state)?;
    let written = word(&field(state, "written")?).ok_or_else(|| PyValueError::new_err("Invalid scene scaler written-word mask"))?;
    let mask = word(&field(state, "mask")?).ok_or_else(|| PyValueError::new_err("Invalid scene scaler interrupt mask"))?;
    if state.len() != 11 || phase == Phase::Active || written & !COMPLETE_WORDS != 0
        || !field(state, "plan")?.is_none() || !field(state, "deadline")?.is_none()
        || field(state, "failed")?.extract::<bool>()? || !matches!(mask, 0 | 0x1001)
    {
        return Err(PyValueError::new_err("Scene scaler ACTIVE, failed or malformed capture is unsupported"));
    }
    field(state, "auxiliary")?.extract::<bool>()?;
    field(state, "cleared")?.extract::<bool>()?;
    if phase == Phase::Programming {
        if !field(state, "origin")?.is_none() || !field(state, "route")?.is_none() {
            return Err(PyValueError::new_err("Inactive scene scaler retains active metadata"));
        }
    } else {
        if written != COMPLETE_WORDS || !field(state, "auxiliary")?.extract::<bool>()?
            || !field(state, "cleared")?.extract::<bool>()?
        {
            return Err(PyValueError::new_err("Completed scene scaler has incomplete programming"));
        }
        Core::parse(&field(state, "origin")?)?;
        Core::parse(&field(state, "route")?)?;
        Plan::new(words)?.validate_bounds(device.getattr("board")?.getattr("ram")?.len()?)
            .map_err(PyValueError::new_err)?;
    }
    Ok(())
}

#[pyfunction]
pub(crate) fn scene_scaler_snapshot<'py>(device: &Bound<'py, PyAny>) -> PyResult<Option<Bound<'py, PyDict>>> {
    let Some(state) = state(device)? else { return Ok(None); };
    let py = device.py();
    let phase = Phase::parse(&field(&state, "phase")?)?;
    let registers = PyDict::new(py);
    for (offset, value) in OFFSETS.into_iter().zip(programming(&state)?) {
        registers.set_item(format!("{:#x}", BASE + u64::from(offset)), value)?;
    }
    let provenance = PyDict::new(py);
    provenance.set_item("profile", PROFILE)?;
    provenance.set_item("selection", device.getattr("scene_profile")?)?;
    provenance.set_item("contract", "Opaque 16-bit nearest samples; centered positive Q16 diagonal transform; clamped edges; row padding retained")?;
    provenance.set_item("completion", "Live source at one-tick deadline; tracked output before latch1 and one IRQ67 request")?;
    provenance.set_item("limits", ["Pixel representation, interpolation, border rule, status, timing and physical fidelity are unmeasured",
        "No firmware patch, precomputed recognition result or guest callback injection",
        "Non-diagonal transforms, nonzero windows, active capture and active retry refuse"])?;
    let snapshot = PyDict::new(py);
    snapshot.set_item("name", "DeclaredUIConversion")?;
    snapshot.set_item("base", "0xfff47000")?;
    snapshot.set_item("phase", phase.name())?;
    snapshot.set_item("busy", phase == Phase::Active)?;
    for name in ["failed", "deadline", "origin", "route", "written", "mask", "cleared"] {
        snapshot.set_item(name, field(&state, name)?)?;
    }
    snapshot.set_item("registers", registers)?;
    snapshot.set_item("provenance", provenance)?;
    Ok(Some(snapshot))
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Plan>()?;
    module.add_function(wrap_pyfunction!(scene_scaler_busy, module)?)?;
    module.add_function(wrap_pyfunction!(scene_scaler_read, module)?)?;
    module.add_function(wrap_pyfunction!(scene_scaler_write, module)?)?;
    module.add_function(wrap_pyfunction!(scene_scaler_next, module)?)?;
    module.add_function(wrap_pyfunction!(scene_scaler_tick, module)?)?;
    module.add_function(wrap_pyfunction!(scene_scaler_validate, module)?)?;
    module.add_function(wrap_pyfunction!(scene_scaler_snapshot, module)?)?;
    Ok(())
}
