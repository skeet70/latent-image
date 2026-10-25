use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};

use crate::mode_devices::{exact_int, unsupported, word, PROFILE};

const BASE: u64 = 0xfff4_2000;
const RESET: [(u32, u32); 79] = [
    (0x000, 0), (0x004, 0), (0x008, 0), (0x00c, 0), (0x010, 0), (0x014, 0),
    (0x018, 0), (0x01c, 0), (0x020, 0), (0x024, 0), (0x028, 0), (0x02c, 0),
    (0x030, 0), (0x034, 0), (0x038, 0), (0x03c, 0), (0x040, 0), (0x044, 0),
    (0x048, 0), (0x04c, 0), (0x050, 0), (0x054, 0),
    (0x060, 0), (0x064, 0), (0x068, 0), (0x06c, 0), (0x070, 0), (0x074, 0), (0x078, 0),
    (0x080, 0), (0x084, 0), (0x088, 0), (0x08c, 0), (0x090, 0), (0x094, 0), (0x098, 0), (0x09c, 0),
    (0x104, 0), (0x10c, 0xf),
    (0x120, 0), (0x124, 0), (0x128, 0), (0x12c, 0), (0x130, 0), (0x134, 0),
    (0x138, 0), (0x13c, 0), (0x140, 0), (0x144, 0), (0x184, 0), (0x18c, 0xf),
    (0x1a0, 0), (0x1a4, 0), (0x1a8, 0), (0x1ac, 0), (0x1b0, 0), (0x1b4, 0),
    (0x1b8, 0), (0x1bc, 0), (0x1c0, 0), (0x1c4, 0), (0x1f4, 1), (0x1f8, 0),
    (0x800, 0xabab_abab), (0x804, 0xabab_abab), (0x808, 0xabab_abab), (0x80c, 0xabab_abab),
    (0x810, 0xabab_abab), (0x814, 0xabab_abab), (0x818, 0xabab_abab), (0x81c, 0xabab_abab),
    (0x820, 0xabab_abab), (0x824, 0xabab_abab), (0x828, 0xabab_abab), (0x82c, 0xabab_abab),
    (0x830, 0xabab_abab), (0x834, 0xabab_abab), (0x838, 0xabab_abab), (0x83c, 0xabab_abab),
];

#[derive(Clone, Copy)]
enum Cpu { Primary, Secondary }

impl Cpu {
    fn parse(value: &Bound<'_, PyAny>) -> Option<Self> {
        match exact_int(value)? { 0 => Some(Self::Primary), 1 => Some(Self::Secondary), _ => None }
    }
    fn number(self) -> u8 { match self { Self::Primary => 0, Self::Secondary => 1 } }
}

#[derive(Clone, Copy)]
struct ResetIndex(usize);

enum State { Cold, Resetting { next: ResetIndex, owner: Cpu }, Initialized }

impl State {
    fn decode(state: &Bound<'_, PyDict>) -> PyResult<Self> {
        if state.is_empty() { return Ok(Self::Cold); }
        let next = state.get_item("next")?.as_ref().and_then(exact_int);
        let owner = state.get_item("owner")?;
        match (next, owner) {
            (Some(0), Some(owner)) if owner.is_none() => Ok(Self::Cold),
            (Some(next), Some(owner)) if next == RESET.len() as u64 + 1 && owner.is_none() => Ok(Self::Initialized),
            (Some(next), Some(owner)) if (1..=RESET.len() as u64).contains(&next) => Cpu::parse(&owner)
                .map(|owner| Self::Resetting { next: ResetIndex(next as usize), owner })
                .ok_or_else(|| PyValueError::new_err("Invalid FFF420 reset owner")),
            _ => Err(PyValueError::new_err("Invalid FFF420 initialization state")),
        }
    }

    fn encode(self, state: &Bound<'_, PyDict>) -> PyResult<()> {
        match self {
            Self::Cold => { state.set_item("next", 0)?; state.set_item("owner", state.py().None())?; }
            Self::Initialized => { state.set_item("next", RESET.len() + 1)?; state.set_item("owner", state.py().None())?; }
            Self::Resetting { next, owner } => {
                state.set_item("next", next.0)?;
                state.set_item("owner", owner.number())?;
            }
        }
        Ok(())
    }
}

#[pyclass(dict, unsendable, module = "x70_native._native", name = "FFF42InitControl")]
pub(crate) struct Device;

#[pymethods]
impl Device {
    #[classattr]
    const BASE: u64 = BASE;

    #[new]
    #[pyo3(signature = (_board, *, profile))]
    fn new(_board: &Bound<'_, PyAny>, profile: &str) -> PyResult<Self> {
        if profile != PROFILE { return Err(PyValueError::new_err("FFF420 requires the named mode device profile")); }
        Ok(Self)
    }

    fn contains(&self, address: u64, _size: u64) -> bool { (BASE..BASE + 0x1000).contains(&address) }
    fn mmio_ranges(&self) -> ((u64, u64),) { ((BASE, BASE + 0x1000),) }

    fn read(&self, _core: &Bound<'_, PyAny>, _address: u64, _size: u64, py: Python<'_>) -> PyResult<u32> {
        Err(unsupported(py, "FFF420 read semantics are unmodeled"))
    }

    fn write(slf: &Bound<'_, Self>, core: &Bound<'_, PyAny>, address: u64, size: u64, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let owner = Cpu::parse(core).ok_or_else(|| unsupported(slf.py(), "FFF420 requires CPU0 or CPU1"))?;
        let value = word(value).ok_or_else(|| unsupported(slf.py(), "FFF420 requires an unsigned word"))?;
        let offset = address.checked_sub(BASE).filter(|offset| *offset < 0x1000 && offset & 3 == 0)
            .ok_or_else(|| unsupported(slf.py(), "FFF420 requires an aligned device address"))? as u32;
        if size != 4 { return Err(unsupported(slf.py(), "FFF420 requires word access")); }
        let fields = slf.getattr("__dict__")?.cast_into::<PyDict>()?;
        let state = State::decode(&fields)?;
        let next = match state {
            State::Cold | State::Initialized if offset == 0 && value == 0 =>
                State::Resetting { next: ResetIndex(1), owner },
            State::Resetting { next, owner: saved } if saved.number() == owner.number() => {
                if next.0 == RESET.len() && (offset, value) == (0, 0x10000) {
                    State::Initialized
                } else if RESET.get(next.0).is_some_and(|expected| *expected == (offset, value)) {
                    State::Resetting { next: ResetIndex(next.0 + 1), owner }
                } else { return Err(unsupported(slf.py(), "FFF420 initialization differs from the recovered transaction")); }
            }
            _ => return Err(unsupported(slf.py(), "FFF420 accepts only its complete same-CPU initialization; processing is unmodeled")),
        };
        next.encode(&fields)
    }

    fn snapshot<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyDict>> {
        let fields = slf.getattr("__dict__")?.cast_into::<PyDict>()?;
        let state = State::decode(&fields)?;
        let result = PyDict::new(slf.py());
        let (count, initialized) = match state {
            State::Cold => (0, false), State::Resetting { next, .. } => (next.0, false),
            State::Initialized => (RESET.len(), true),
        };
        let desired = PyDict::new(slf.py());
        for (offset, value) in &RESET[..count] { desired.set_item(*offset, *value)?; }
        if initialized { desired.set_item(0, 0x10000)?; }
        result.set_item("name", "FFF42InitControl")?;
        result.set_item("base", "0xfff42000")?;
        result.set_item("profile", PROFILE)?;
        result.set_item("initialized", initialized)?;
        result.set_item("desired_registers", desired)?;
        result.set_item("limits", "X-M1 0068164c..00681aa8 reset and 006815ac literal10000 retained; readback, processing, status, output and IRQ remain unsupported")?;
        Ok(result)
    }
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> { module.add_class::<Device>() }

#[cfg(test)]
mod tests {
    use pyo3::prelude::*;
    use pyo3::types::{PyDict, PyModule};

    fn device(py: Python<'_>) -> Bound<'_, PyAny> {
        let module = PyModule::new(py, "_native").unwrap();
        crate::_native(&module).unwrap();
        let options = PyDict::new(py);
        options.set_item("profile", "xm1-150-declared-mode-devices-unmeasured").unwrap();
        module.getattr("FFF42InitControl").unwrap().call((py.None(),), Some(&options)).unwrap()
    }

    fn initialize(device: &Bound<'_, PyAny>) {
        let mut inputs = Vec::new();
        for (first, last) in [(0, 0x54), (0x60, 0x78), (0x80, 0x9c)] {
            inputs.extend((first..=last).step_by(4).map(|offset| (offset, 0u32)));
        }
        inputs.extend([(0x104, 0), (0x10c, 15)]);
        inputs.extend((0x120..=0x144).step_by(4).map(|offset| (offset, 0)));
        inputs.extend([(0x184, 0), (0x18c, 15)]);
        inputs.extend((0x1a0..=0x1c4).step_by(4).map(|offset| (offset, 0)));
        inputs.extend([(0x1f4, 1), (0x1f8, 0)]);
        inputs.extend((0x800..=0x83c).step_by(4).map(|offset| (offset, 0xabab_abab)));
        for (offset, value) in inputs {
            device.call_method1("write", (0, 0xfff4_2000u64 + offset, 4, value)).unwrap();
        }
        device.call_method1("write", (0, 0xfff4_2000u64, 4, 0x10000)).unwrap();
    }

    #[test]
    fn complete_stock_initialization_retains_desired_words_and_can_repeat() {
        Python::initialize();
        Python::attach(|py| {
            let device = device(py);
            for _ in 0..2 {
                initialize(&device);
                let report = device.call_method0("snapshot").unwrap();
                assert!(report.get_item("initialized").unwrap().extract::<bool>().unwrap());
                let desired = report.get_item("desired_registers").unwrap();
                assert_eq!(desired.get_item(0).unwrap().extract::<u32>().unwrap(), 0x10000);
                assert_eq!(desired.get_item(0x10c).unwrap().extract::<u32>().unwrap(), 15);
                assert_eq!(desired.get_item(0x18c).unwrap().extract::<u32>().unwrap(), 15);
                assert_eq!(desired.get_item(0x1f4).unwrap().extract::<u32>().unwrap(), 1);
                assert_eq!(desired.get_item(0x83c).unwrap().extract::<u32>().unwrap(), 0xabab_abab);
            }
        });
    }

    #[test]
    fn initialization_refuses_wrong_order_owner_width_and_boolean_data() {
        Python::initialize();
        Python::attach(|py| {
            let device = device(py);
            device.call_method1("write", (0, 0xfff4_2000u64, 4, 0)).unwrap();
            assert!(device.call_method1("write", (0, 0xfff4_2008u64, 4, 0)).is_err());
            assert!(device.call_method1("write", (1, 0xfff4_2004u64, 4, 0)).is_err());
            assert!(device.call_method1("write", (0, 0xfff4_2004u64, 2, 0)).is_err());
            assert!(device.call_method1("write", (0, 0xfff4_2004u64, 4, false)).is_err());
            device.call_method1("write", (0, 0xfff4_2004u64, 4, 0)).unwrap();
            assert!(!device.call_method0("snapshot").unwrap().get_item("initialized").unwrap().extract::<bool>().unwrap());
        });
    }

    #[test]
    fn initialized_device_refuses_unmodeled_processing_and_reads() {
        Python::initialize();
        Python::attach(|py| {
            let device = device(py);
            initialize(&device);
            assert!(device.call_method1("write", (0, 0xfff4_2000u64, 4, 1)).is_err());
            assert!(device.call_method1("read", (0, 0xfff4_2104u64, 4)).is_err());
            assert!(device.call_method0("snapshot").unwrap().get_item("initialized").unwrap().extract::<bool>().unwrap());
        });
    }
}
