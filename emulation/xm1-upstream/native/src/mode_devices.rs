//! X-M1 1.50 idle face reset and FFF48 initialization under an unmeasured profile.
//! No processing result, DMA payload, interrupt or processing-start response is supplied.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyInt, PyModule};

pub(crate) const PROFILE: &str = "xm1-150-declared-mode-device-idle-unmeasured";
const FACE_BASE: u64 = 0xfff1_4000;
const ENGINE_BASE: u64 = 0xfff4_8000;

fn unsupported(py: Python<'_>, reason: &str) -> PyErr {
    match PyModule::import(py, "emulator.hardware").and_then(|module| module.getattr("UnsupportedDevice")) {
        Ok(class) => match class.call1((reason,)) {
            Ok(error) => PyErr::from_value(error),
            Err(error) => error,
        },
        Err(error) => error,
    }
}

fn exact_int(value: &Bound<'_, PyAny>) -> Option<u64> {
    value.is_exact_instance_of::<PyInt>().then(|| value.extract().ok()).flatten()
}

fn word(value: &Bound<'_, PyAny>) -> Option<u32> {
    exact_int(value).and_then(|value| value.try_into().ok())
}

struct Cpu(u8);

fn cpu(core: &Bound<'_, PyAny>) -> Option<Cpu> {
    exact_int(core).filter(|core| *core < 2).map(|core| Cpu(core as u8))
}

// 0x010648e8 writes reset 0/1; 0x01069e00 cancels without requiring a submitted job.
fn face_mode_access(
    device: &Bound<'_, PyAny>, core: &Bound<'_, PyAny>, address: u64, size: u64,
    value: Option<&Bound<'_, PyAny>>,
) -> PyResult<Option<u32>> {
    let py = device.py();
    let profile = device.getattr("mode_profile")?;
    if profile.is_none() {
        return Ok(None);
    }
    if !profile.eq(PROFILE)? {
        return Err(PyValueError::new_err("Unknown mode device profile"));
    }
    let state = device.getattr("__dict__")?.cast_into::<PyDict>()?;
    let pending = state.get_item("mode_reset_cpu")?.filter(|value| !value.is_none());
    if !device.getattr("request")?.is_none() || !device.getattr("stop")?.is_none() {
        if pending.is_none() {
            return Ok(None);
        }
        return Err(unsupported(py, "Face idle reset cannot overlap a processing request"));
    }
    let offset = address.checked_sub(FACE_BASE);
    let dma = offset.filter(|offset| (0x800..0xa00).contains(offset));
    if address != FACE_BASE && pending.is_none() && dma.is_none() {
        return Ok(None);
    }
    let Some(core) = cpu(core) else {
        return Err(unsupported(py, "Face idle control requires CPU 0 or 1"));
    };
    if size != 4 || address & 3 != 0 || value.is_some_and(|value| word(value).is_none()) {
        return Err(unsupported(py, "Face idle control requires an aligned unsigned word"));
    }
    let registers = device.getattr("registers")?.cast_into::<PyDict>()?;
    if address == FACE_BASE || pending.is_some() {
        let expected = pending.as_ref().map(|_| 1).unwrap_or(0);
        if address != FACE_BASE || value.and_then(word) != Some(expected)
            || pending.as_ref().is_some_and(|owner| exact_int(owner) != Some(core.0 as u64))
        {
            return Err(unsupported(py, "Face idle reset requires an uninterrupted same-CPU reset 0/1 pair"));
        }
        for channel in 0..4u64 {
            for offset in [0, 4, 0x10] {
                if registers.get_item(0x800 + channel * 0x80 + offset)?.as_ref().and_then(word) != Some(0) {
                    return Err(unsupported(py, "Face idle reset requires four inactive DMA channels"));
                }
            }
        }
        if expected == 0 {
            state.set_item("mode_reset_cpu", core.0)?;
        } else {
            let resets = state.get_item("mode_resets")?.map(|value| exact_int(&value))
                .unwrap_or(Some(0)).ok_or_else(|| PyValueError::new_err("Invalid face reset count"))?;
            let resets = resets.checked_add(1).ok_or_else(|| PyValueError::new_err("Face reset count overflow"))?;
            state.set_item("mode_resets", resets)?;
            state.set_item("mode_reset_cpu", py.None())?;
        }
        return Ok(Some(expected));
    }
    let offset = dma.ok_or_else(|| unsupported(py, "Face unobserved idle control"))?;
    face_idle_dma(device, &registers, offset, value)
}

#[derive(Clone, Copy)]
enum DmaPhase {
    Initial, ClearEnable, PollIdle, PollEnable, ClearCommand, ConfigRead,
    ConfigWrite, ConfigControl, ConfigCount, ConfigExtra, Configured,
}

impl DmaPhase {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "initial" => Self::Initial,
            "clear-enable" => Self::ClearEnable,
            "poll-idle" => Self::PollIdle,
            "poll-enable" => Self::PollEnable,
            "clear-command" => Self::ClearCommand,
            "config-read" => Self::ConfigRead,
            "config-write" => Self::ConfigWrite,
            "config-control" => Self::ConfigControl,
            "config-count" => Self::ConfigCount,
            "config-extra" => Self::ConfigExtra,
            "configured" => Self::Configured,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::ClearEnable => "clear-enable",
            Self::PollIdle => "poll-idle",
            Self::PollEnable => "poll-enable",
            Self::ClearCommand => "clear-command",
            Self::ConfigRead => "config-read",
            Self::ConfigWrite => "config-write",
            Self::ConfigControl => "config-control",
            Self::ConfigCount => "config-count",
            Self::ConfigExtra => "config-extra",
            Self::Configured => "configured",
        }
    }
}

// 0x01070dcc clears/polls inactive channels; 0x01071298 programs their retained configuration.
fn face_idle_dma(
    device: &Bound<'_, PyAny>, registers: &Bound<'_, PyDict>, offset: u64,
    written: Option<&Bound<'_, PyAny>>,
) -> PyResult<Option<u32>> {
    let channel = ((offset - 0x800) / 0x80) as usize;
    let local = (offset - 0x800) % 0x80;
    let states = device.getattr("dma_states")?;
    let saved = states.get_item(channel)?;
    let phase = DmaPhase::parse(saved.cast::<pyo3::types::PyString>()?.to_str()?)
        .ok_or_else(|| unsupported(device.py(), "Face DMA phase is not inactive"))?;
    let old = registers.get_item(offset)?.as_ref().and_then(word)
        .ok_or_else(|| unsupported(device.py(), "Face unobserved DMA word"))?;
    let value = written.and_then(word).unwrap_or(old);
    use DmaPhase::*;
    let next = match (phase, written.is_some(), local, value) {
        (Initial | Configured, false, 4, 0) => Some(ClearEnable),
        (ClearEnable, true, 4, 0) => Some(PollIdle),
        (PollIdle, false, 0x10, 0) => Some(PollEnable),
        (PollEnable, false, 4, 0) => Some(ClearCommand),
        (ClearCommand, true, 0, 0) => Some(ConfigRead),
        (ConfigRead | Configured, false, 0x20, _) => Some(ConfigWrite),
        (ConfigWrite, true, 0x20, value)
            if value & !0x0070_3000 == 0 && value & 0xfffff == 0x3000
                && (value >> 20) & 7 <= 4 => Some(ConfigControl),
        (ConfigControl, true, 4, 0) => Some(ConfigCount),
        (ConfigCount, true, 0x28, 0x100) => Some(ConfigExtra),
        (ConfigExtra, true, 0x44, 0 | 1) => Some(Configured),
        _ => None,
    }.ok_or_else(|| unsupported(device.py(), "Face access outside recovered inactive DMA setup"))?;
    if written.is_some() {
        registers.set_item(offset, value)?;
    }
    states.set_item(channel, next.name())?;
    Ok(Some(value))
}

#[pyfunction]
pub(crate) fn face_mode_write(
    device: &Bound<'_, PyAny>, core: &Bound<'_, PyAny>, address: u64, size: u64, value: &Bound<'_, PyAny>,
) -> PyResult<bool> {
    Ok(face_mode_access(device, core, address, size, Some(value))?.is_some())
}

#[pyfunction]
pub(crate) fn face_mode_read(
    device: &Bound<'_, PyAny>, core: &Bound<'_, PyAny>, address: u64, size: u64,
) -> PyResult<Option<u32>> {
    face_mode_access(device, core, address, size, None)
}

// Masks follow the software field construction in 0x006689d4..0x00669a88.
fn programming_mask(offset: u64) -> Option<u32> {
    match offset {
        0x004 => Some(0x77),
        0x010 | 0x018 | 0x020 | 0x028 | 0x030 | 0x054 => Some(0x3fff_ffff),
        0x014 | 0x01c | 0x024 | 0x02c | 0x034 | 0x058 => Some(0x7fff),
        0x038 | 0x048 => Some(0x1fff_1fff),
        0x03c => Some(0x03ff_03ff),
        0x040 => Some(0x003f_003f),
        0x044 => Some(0x07ff_07ff),
        0x04c => Some(0x1f1f_ffff),
        0x050 => Some(0x0011_7777),
        0x0d0 => Some(0x71ff_71ff),
        0x0d4 => Some(0x7700_7700),
        0x060 | 0x064 | 0x070..=0x0b8 => Some(0),
        0x100..=0x4fc => Some(0),
        _ => None,
    }
}

fn programming_valid(offset: u64, value: u32) -> bool {
    match offset {
        0x014 | 0x01c | 0x024 | 0x02c | 0x034 | 0x058 => value <= 0x7ffc,
        0x038 | 0x048 => value & 0x1fff <= 0x1ffc,
        0x03c => value & 0x3ff <= 0x3fc && (value >> 16) & 0x3ff <= 0x3fc,
        0x040 => value & 0x3f <= 0x3c,
        0x044 => value & 0x7ff <= 0x7fc,
        0x0d4 => [8, 12, 24, 28].into_iter().all(|shift| (value >> shift) & 7 <= 4),
        _ => true,
    }
}

#[pyclass(dict, unsendable, module = "x70_native._native", name = "FFF48Engine")]
pub(crate) struct Device;

impl Device {
    fn state<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyDict>> {
        slf.getattr("__dict__")?.cast_into::<PyDict>().map_err(Into::into)
    }

    fn access(slf: &Bound<'_, Self>, core: &Bound<'_, PyAny>, address: u64, size: u64) -> PyResult<u64> {
        if cpu(core).is_none() || size != 4 || address & 3 != 0 {
            return Err(unsupported(slf.py(), "FFF48 requires an aligned word from CPU 0 or 1"));
        }
        address.checked_sub(ENGINE_BASE).filter(|offset| *offset < 0x1000)
            .ok_or_else(|| unsupported(slf.py(), "FFF48 address outside device"))
    }
}

#[pymethods]
impl Device {
    #[classattr]
    const BASE: u64 = ENGINE_BASE;

    #[new]
    #[pyo3(signature = (_board, *, profile))]
    fn new(_board: &Bound<'_, PyAny>, profile: &str) -> PyResult<Self> {
        if profile != PROFILE {
            return Err(PyValueError::new_err("FFF48 requires the named idle mode device profile"));
        }
        Ok(Self)
    }

    fn contains(&self, address: u64, _size: u64) -> bool {
        (ENGINE_BASE..ENGINE_BASE + 0x1000).contains(&address)
    }

    fn mmio_ranges(&self) -> ((u64, u64),) {
        ((ENGINE_BASE, ENGINE_BASE + 0x1000),)
    }

    fn read(slf: &Bound<'_, Self>, core: &Bound<'_, PyAny>, address: u64, size: u64) -> PyResult<u32> {
        let offset = Self::access(slf, core, address, size)?;
        let state = Self::state(slf)?;
        match offset {
            0 => Ok(1),
            4 => state.get_item("selector")?.map(|value| word(&value))
                .unwrap_or(Some(0)).ok_or_else(|| PyValueError::new_err("Invalid FFF48 selector")),
            0xc0 => Ok(0),
            _ => Err(unsupported(slf.py(), "FFF48 unobserved register read")),
        }
    }

    fn write(
        slf: &Bound<'_, Self>, core: &Bound<'_, PyAny>, address: u64, size: u64, value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let offset = Self::access(slf, core, address, size)?;
        let value = word(value).ok_or_else(|| unsupported(slf.py(), "FFF48 requires an unsigned word"))?;
        let state = Self::state(slf)?;
        match offset {
            0 if value == 0 => {
                state.set_item("stopped", true)?;
                Ok(())
            }
            0 => Err(unsupported(slf.py(), "FFF48 processing start is unmodeled")),
            0xc4 if value & !0x331 == 0 => Ok(()),
            _ => {
                let stopped = state.get_item("stopped")?.map(|value| value.extract::<bool>()).transpose()?.unwrap_or(false);
                if !stopped {
                    return Err(unsupported(slf.py(), "FFF48 programming requires the stop handshake"));
                }
                if offset == 0xc8 && value & !0x331 == 0 {
                    state.set_item("interrupt_mask", value)?;
                    return Ok(());
                }
                let mask = programming_mask(offset).ok_or_else(|| unsupported(slf.py(), "FFF48 unobserved register write"))?;
                if value & !mask != 0 || !programming_valid(offset, value) {
                    return Err(unsupported(slf.py(), "FFF48 value outside the recovered setter domain"));
                }
                let words = match state.get_item("registers")? {
                    Some(words) => words.cast_into::<PyDict>()?,
                    None => {
                        let words = PyDict::new(slf.py());
                        state.set_item("registers", &words)?;
                        words
                    }
                };
                words.set_item(offset, value)?;
                if offset == 4 {
                    state.set_item("selector", value)?;
                }
                Ok(())
            }
        }
    }

    fn snapshot<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyDict>> {
        let result = Self::state(slf)?.copy()?;
        result.set_item("name", "FFF48Engine")?;
        result.set_item("base", format!("{ENGINE_BASE:#x}"))?;
        result.set_item("profile", PROFILE)?;
        result.set_item("control", 1)?;
        result.set_item("status", 0)?;
        result.set_item("limits", "Declared idle control=1 and status=0; stop/ack/passive software-domain programming only; no start, DMA, output, completion or IRQ")?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    const EMULATOR: &str = match option_env!("XM1_EMULATOR_PYTHON_PATH") {
        Some(path) => path,
        None => concat!(env!("CARGO_MANIFEST_DIR"), "/../emulator"),
    };

    fn source(name: &str) -> CString {
        CString::new(std::fs::read(std::path::Path::new(EMULATOR).join(name)).unwrap()).unwrap()
    }

    fn modules(py: Python<'_>) -> Bound<'_, PyAny> {
        let modules = py.import("sys").unwrap().getattr("modules").unwrap();
        let emulator = modules.call_method1("setdefault", ("emulator", PyModule::new(py, "emulator").unwrap())).unwrap();
        emulator.setattr("__path__", [EMULATOR]).unwrap();
        let hardware = PyModule::new(py, "emulator.hardware").unwrap();
        py.run(&source("hardware.py"), Some(&hardware.dict()), None).unwrap();
        let hardware = modules.call_method1("setdefault", ("emulator.hardware", hardware)).unwrap();
        modules.call_method1("setdefault", ("x70_native", PyModule::new(py, "x70_native").unwrap())).unwrap();
        let native = modules.call_method1("setdefault", ("x70_native._native", PyModule::new(py, "x70_native._native").unwrap())).unwrap();
        let native = native.cast::<PyModule>().unwrap();
        native.add_function(wrap_pyfunction!(face_mode_read, native).unwrap()).unwrap();
        native.add_function(wrap_pyfunction!(face_mode_write, native).unwrap()).unwrap();
        hardware.getattr("UnsupportedDevice").unwrap()
    }

    fn board(py: Python<'_>) -> Bound<'_, PyAny> {
        let board = py.import("types").unwrap().getattr("SimpleNamespace").unwrap().call0().unwrap();
        let record = py.import("unittest.mock").unwrap().getattr("Mock").unwrap().call0().unwrap();
        board.setattr("record", record).unwrap();
        board
    }

    fn face(py: Python<'_>, enabled: bool) -> Bound<'_, PyAny> {
        modules(py);
        let source = source("face_analysis_devices.py");
        let module = PyModule::from_code(py, &source, c"face_analysis_devices.py", c"emulator.face_analysis_devices").unwrap();
        let options = PyDict::new(py);
        if enabled {
            options.set_item("mode_profile", PROFILE).unwrap();
        }
        module.getattr("FaceAnalysisControl").unwrap().call((board(py),), Some(&options)).unwrap()
    }

    fn engine(py: Python<'_>) -> Bound<'_, PyAny> {
        modules(py);
        let options = PyDict::new(py);
        options.set_item("profile", PROFILE).unwrap();
        py.get_type::<Device>().call((board(py),), Some(&options)).unwrap()
    }

    fn write(device: &Bound<'_, PyAny>, base: u64, offset: u64, value: u32) -> PyResult<()> {
        device.call_method1("write", (0, base + offset, 4, value)).map(drop)
    }

    fn read(device: &Bound<'_, PyAny>, base: u64, offset: u64) -> PyResult<u32> {
        device.call_method1("read", (0, base + offset, 4))?.extract()
    }

    fn snapshot<'py>(device: &Bound<'py, PyAny>) -> Bound<'py, PyAny> {
        device.call_method0("snapshot").unwrap()
    }

    fn inactive_channel(device: &Bound<'_, PyAny>, channel: u64) {
        let offset = 0x800 + channel * 0x80;
        assert_eq!(read(device, FACE_BASE, offset + 4).unwrap(), 0);
        write(device, FACE_BASE, offset + 4, 0).unwrap();
        assert_eq!(read(device, FACE_BASE, offset + 0x10).unwrap(), 0);
        assert_eq!(read(device, FACE_BASE, offset + 4).unwrap(), 0);
        write(device, FACE_BASE, offset, 0).unwrap();
        assert_eq!(read(device, FACE_BASE, offset + 0x20).unwrap(), 0);
        write(device, FACE_BASE, offset + 0x20, 0x10_3000).unwrap();
        write(device, FACE_BASE, offset + 4, 0).unwrap();
        write(device, FACE_BASE, offset + 0x28, 0x100).unwrap();
        write(device, FACE_BASE, offset + 0x44, 1).unwrap();
    }

    #[test]
    fn face_idle_cancel_preserves_configuration_without_producing_a_result() {
        Python::initialize();
        Python::attach(|py| {
            let device = face(py, true);
            write(&device, FACE_BASE, 0x20, 0x0106_f3e8).unwrap();
            for channel in 0..4 {
                inactive_channel(&device, channel);
            }
            for _ in 0..2 {
                write(&device, FACE_BASE, 0, 0).unwrap();
                write(&device, FACE_BASE, 0, 1).unwrap();
            }
            let result = snapshot(&device);
            assert_eq!(result.get_item("mode_resets").unwrap().extract::<u64>().unwrap(), 2);
            assert_eq!(result.get_item("registers").unwrap().get_item("0xfff14020").unwrap().extract::<String>().unwrap(), "0x106f3e8");
            assert!(result.get_item("request").unwrap().is_none());
            assert!(!result.get_item("awaiting_result").unwrap().extract::<bool>().unwrap());
            assert!(read(&device, FACE_BASE, 0x574).is_err());
        });
    }

    #[test]
    fn configured_face_dma_can_be_reprogrammed_without_starting_it() {
        Python::initialize();
        Python::attach(|py| {
            let device = face(py, true);
            inactive_channel(&device, 1);
            assert_eq!(read(&device, FACE_BASE, 0x8a0).unwrap(), 0x10_3000);
            write(&device, FACE_BASE, 0x8a0, 0x40_3000).unwrap();
            write(&device, FACE_BASE, 0x884, 0).unwrap();
            write(&device, FACE_BASE, 0x8a8, 0x100).unwrap();
            write(&device, FACE_BASE, 0x8c4, 1).unwrap();
            let result = snapshot(&device);
            assert_eq!(result.get_item("registers").unwrap().get_item("0xfff148a0").unwrap().extract::<String>().unwrap(), "0x403000");
            assert!(result.get_item("request").unwrap().is_none());
            assert!(write(&device, FACE_BASE, 0x884, 1).is_err());
        });
    }

    #[test]
    fn face_reset_refuses_wrong_order_owner_width_and_active_dma() {
        Python::initialize();
        Python::attach(|py| {
            let unsupported = modules(py);
            let disabled = face(py, false);
            assert!(write(&disabled, FACE_BASE, 0, 0).unwrap_err().value(py).is_instance(&unsupported).unwrap());
            let device = face(py, true);
            assert!(write(&device, FACE_BASE, 0, 1).is_err());
            write(&device, FACE_BASE, 0, 0).unwrap();
            assert!(write(&device, FACE_BASE, 0, 0).is_err());
            assert!(device.call_method1("write", (1, FACE_BASE, 4, 1)).is_err());
            assert!(device.call_method1("write", (0, FACE_BASE, 2, 1)).is_err());
            assert!(read(&device, FACE_BASE, 0x804).is_err());
            write(&device, FACE_BASE, 0, 1).unwrap();
            assert_eq!(snapshot(&device).get_item("mode_resets").unwrap().extract::<u64>().unwrap(), 1);
            assert_eq!(read(&device, FACE_BASE, 0x804).unwrap(), 0);
            assert!(write(&device, FACE_BASE, 0x804, 1).is_err());
            assert!(read(&device, FACE_BASE, 0x808).is_err());
        });
    }

    #[test]
    fn face_reset_checkpoint_resumes_the_same_cpu_pair() {
        Python::initialize();
        Python::attach(|py| {
            let device = face(py, true);
            write(&device, FACE_BASE, 0, 0).unwrap();
            let state = py.import("copy").unwrap().getattr("deepcopy").unwrap().call1((device.getattr("__dict__").unwrap(),)).unwrap();
            let resumed = face(py, true);
            resumed.getattr("__dict__").unwrap().call_method1("update", (state,)).unwrap();
            write(&resumed, FACE_BASE, 0, 1).unwrap();
            write(&device, FACE_BASE, 0, 1).unwrap();
            assert!(snapshot(&device).eq(snapshot(&resumed)).unwrap());
        });
    }

    #[test]
    fn adv_stop_poll_ack_and_passive_programming_leave_status_idle() {
        Python::initialize();
        Python::attach(|py| {
            let device = engine(py);
            let control = read(&device, ENGINE_BASE, 0).unwrap();
            write(&device, ENGINE_BASE, 0, control & !1).unwrap();
            assert_eq!(read(&device, ENGINE_BASE, 0).unwrap(), 1);
            write(&device, ENGINE_BASE, 0xc4, 0x331).unwrap();
            write(&device, ENGINE_BASE, 4, 0x20).unwrap();
            write(&device, ENGINE_BASE, 0x10, 0x021a_1800).unwrap();
            write(&device, ENGINE_BASE, 0x14, 0x200).unwrap();
            write(&device, ENGINE_BASE, 0xc8, 0x331).unwrap();
            assert_eq!(read(&device, ENGINE_BASE, 4).unwrap(), 0x20);
            assert_eq!(read(&device, ENGINE_BASE, 0xc0).unwrap(), 0);
            write(&device, ENGINE_BASE, 0xc4, 0x331).unwrap();
            assert_eq!(read(&device, ENGINE_BASE, 0xc0).unwrap(), 0);
            assert_eq!(snapshot(&device).get_item("control").unwrap().extract::<u32>().unwrap(), 1);
        });
    }

    #[test]
    fn adv_refuses_starts_outputs_unknown_registers_and_invalid_setter_values() {
        Python::initialize();
        Python::attach(|py| {
            let unsupported = modules(py);
            let device = engine(py);
            assert!(write(&device, ENGINE_BASE, 4, 0x20).is_err());
            write(&device, ENGINE_BASE, 0, 0).unwrap();
            for (offset, value) in [
                (0, 1), (4, 8), (0xc4, 2), (0xc8, 2), (0x14, 0x8000),
                (0x14, 0x7ffd), (0x38, 0x1ffd), (0x3c, 0x03fd_0000),
                (0x40, 0x3d), (0x44, 0x7fd), (0xd4, 0x500), (0xfc, 0), (0x500, 0),
            ] {
                let error = write(&device, ENGINE_BASE, offset, value).unwrap_err();
                assert!(error.value(py).is_instance(&unsupported).unwrap());
            }
            for offset in [0x8, 0x10, 0x100, 0x1000] {
                assert!(read(&device, ENGINE_BASE, offset).is_err());
            }
            for args in [(2, ENGINE_BASE, 4), (0, ENGINE_BASE, 1), (0, ENGINE_BASE + 1, 4)] {
                assert!(device.call_method1("read", args).is_err());
            }
            assert!(device.call_method1("write", (0, ENGINE_BASE, 4, true)).is_err());
            assert_eq!(read(&device, ENGINE_BASE, 0xc0).unwrap(), 0);
        });
    }

    #[test]
    fn adv_checkpoint_restores_selector_and_programmed_words() {
        Python::initialize();
        Python::attach(|py| {
            let device = engine(py);
            write(&device, ENGINE_BASE, 0, 0).unwrap();
            write(&device, ENGINE_BASE, 4, 0x30).unwrap();
            let state = py.import("copy").unwrap().getattr("deepcopy").unwrap().call1((device.getattr("__dict__").unwrap(),)).unwrap();
            let resumed = engine(py);
            resumed.getattr("__dict__").unwrap().call_method1("update", (state,)).unwrap();
            assert_eq!(read(&resumed, ENGINE_BASE, 4).unwrap(), 0x30);
            assert!(snapshot(&device).eq(snapshot(&resumed)).unwrap());
            assert_eq!(resumed.call_method0("mmio_ranges").unwrap().extract::<Vec<(u64, u64)>>().unwrap(), [(ENGINE_BASE, ENGINE_BASE + 0x1000)]);
        });
    }
}
