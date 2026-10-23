//! The 0xfff63000 engine: the stock stop/reset handshake and one declared job
//! completion.
//!
//! Register roles are byte-read from the X-M1 1.50 drivers 0x0065a478
//! (stop/reset), 0x0065a58c (programming), 0x0065a548 (start) and the IRQ
//! handler 0x00d99570, reached from develop-task job 4 (0x00e8fee4 via
//! 0x00d98904/0x00d9882c/0x00d99714). The handler takes STATUS bit 4 as
//! success and stores POINTER - POINTER_BASE and WORD_014 in its job record;
//! 0x00e8fee4 then returns 0 and nothing reads either. The engine's function
//! is unbound; no memory is written.
//!
//! Model state lives in the device's instance `__dict__` as plain values,
//! which the board's in-memory and portable checkpoints capture and restore.
//! An empty dictionary is the state of a new device.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::pyclass::{PyTraverseError, PyVisit};
use pyo3::types::{PyDict, PyInt, PyList, PyModule, PyString};

pub(crate) const PROFILE: &str = "xm1-150-declared-fff63-job4-completion-unmeasured";
const ENGINE_BASE: u64 = 0xfff6_3000;
const ENGINE_SIZE: u64 = 0x12c;
/// IRQ descriptor 0x48 at 0x007c9484.
const ENGINE_IRQ: u8 = 0x4c;
const DEFAULT_COMPLETION_TICKS: u32 = 2;

const STATUS_DONE: u32 = 1 << 4;
const STATUS_STOPPED: u32 = 1 << 16;
const CONTROL_START: u32 = 1;
const CONTROL_RESET: u32 = 1 << 8;
const COMMAND_STOP: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Register {
    Control,
    Status,
    Mask,
    Word014,
    Scale,
    PointerBase,
    Length,
    Pointer,
    Address10c,
    Word110,
    Command,
    Word11c,
    CropEnable,
    CropOrigin,
    CropEnd,
}

impl Register {
    const ALL: [Self; 15] = [
        Self::Control,
        Self::Status,
        Self::Mask,
        Self::Word014,
        Self::Scale,
        Self::PointerBase,
        Self::Length,
        Self::Pointer,
        Self::Address10c,
        Self::Word110,
        Self::Command,
        Self::Word11c,
        Self::CropEnable,
        Self::CropOrigin,
        Self::CropEnd,
    ];

    fn offset(self) -> u64 {
        match self {
            Self::Control => 0x000,
            Self::Status => 0x004,
            Self::Mask => 0x008,
            Self::Word014 => 0x014,
            Self::Scale => 0x0a0,
            Self::PointerBase => 0x100,
            Self::Length => 0x104,
            Self::Pointer => 0x108,
            Self::Address10c => 0x10c,
            Self::Word110 => 0x110,
            Self::Command => 0x118,
            Self::Word11c => 0x11c,
            Self::CropEnable => 0x120,
            Self::CropOrigin => 0x124,
            Self::CropEnd => 0x128,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Control => "CONTROL",
            Self::Status => "STATUS",
            Self::Mask => "MASK",
            Self::Word014 => "WORD_014",
            Self::Scale => "SCALE",
            Self::PointerBase => "POINTER_BASE",
            Self::Length => "LENGTH",
            Self::Pointer => "POINTER",
            Self::Address10c => "ADDRESS_10C",
            Self::Word110 => "WORD_110",
            Self::Command => "COMMAND",
            Self::Word11c => "WORD_11C",
            Self::CropEnable => "CROP_ENABLE",
            Self::CropOrigin => "CROP_ORIGIN",
            Self::CropEnd => "CROP_END",
        }
    }

    fn at(address: u64) -> Option<Self> {
        let offset = address.checked_sub(ENGINE_BASE)?;
        Self::ALL.into_iter().find(|register| register.offset() == offset)
    }
}

/// The registers 0x0065a58c programs, each with the bits it can set.
const PROGRAMMED: [(Register, u32); 10] = [
    (Register::Mask, 0x01ff_0130),
    (Register::Scale, 0x3),
    (Register::PointerBase, u32::MAX),
    (Register::Length, u32::MAX),
    (Register::Address10c, u32::MAX),
    (Register::Word110, u32::MAX),
    (Register::Word11c, 0x3f),
    (Register::CropEnable, 0x1),
    (Register::CropOrigin, u32::MAX),
    (Register::CropEnd, u32::MAX),
];

fn programmed_slot(register: Register) -> Option<usize> {
    PROGRAMMED.iter().position(|(programmed, _)| *programmed == register)
}

fn programmed_by_name(name: &str) -> Option<usize> {
    PROGRAMMED.iter().position(|(register, _)| register.name() == name)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Cpu(u8);

impl Cpu {
    fn new(index: u64) -> Option<Self> {
        (index < 2).then_some(Self(index as u8))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Idle,
    Stopping,
    Stopped,
    Programming,
    Active,
    Completed,
}

impl Phase {
    const ALL: [Self; 6] =
        [Self::Idle, Self::Stopping, Self::Stopped, Self::Programming, Self::Active, Self::Completed];

    fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Stopping => "stop-acknowledged",
            Self::Stopped => "stopped",
            Self::Programming => "programming",
            Self::Active => "active",
            Self::Completed => "completed",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|phase| phase.name() == name)
    }
}

/// Virtual ticks from start to completion, 1..2^32.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct CompletionDelay(u32);

impl CompletionDelay {
    fn new(ticks: u64) -> Option<Self> {
        u32::try_from(ticks).ok().filter(|ticks| *ticks != 0).map(Self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Job {
    cpu: Cpu,
    irq_cpu: Cpu,
    started_tick: u64,
    due_tick: u64,
    programming: [u32; 10],
}

impl Job {
    fn word(&self, register: Register) -> Option<u32> {
        programmed_slot(register).map(|slot| self.programming[slot])
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Refusal(String);

pub(crate) trait Board {
    type Error: From<Refusal>;
    fn virtual_ticks(&self) -> Result<u64, Self::Error>;
    /// Per CPU, whether its interrupt controller enables IRQ 0x4c.
    fn irq_enabled(&self) -> Result<[bool; 2], Self::Error>;
    /// No controller holds IRQ 0x4c pending, active, injected or stacked.
    fn irq_quiescent(&self) -> Result<bool, Self::Error>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Engine {
    phase: Phase,
    status: u32,
    programming: [Option<u32>; 10],
    /// The most recently started job.
    job: Option<Job>,
    jobs: u64,
    last_rejected: Option<String>,
}

impl Default for Engine {
    fn default() -> Self {
        Self { phase: Phase::Idle, status: 0, programming: [None; 10], job: None, jobs: 0, last_rejected: None }
    }
}

impl Engine {
    fn refuse<E: From<Refusal>>(&mut self, reason: String) -> E {
        self.last_rejected = Some(reason.clone());
        Refusal(reason).into()
    }

    fn register<E: From<Refusal>>(&mut self, cpu: Option<Cpu>, address: u64, size: u64) -> Result<Register, E> {
        if cpu.is_none() || size != 4 || address & 3 != 0 {
            return Err(self.refuse("requires an aligned word from CPU 0 or 1".into()));
        }
        Register::at(address).ok_or_else(|| self.refuse(format!("unobserved register {address:#x}")))
    }

    fn route<B: Board>(&mut self, board: &B) -> Result<Cpu, B::Error> {
        match board.irq_enabled()? {
            [true, false] => Ok(Cpu(0)),
            [false, true] => Ok(Cpu(1)),
            _ => Err(self.refuse("IRQ76 requires exactly one enabled CPU owner".into())),
        }
    }

    fn busy(&self) -> bool {
        self.phase == Phase::Active
    }

    pub(crate) fn read<E: From<Refusal>>(&mut self, cpu: Option<Cpu>, address: u64, size: u64) -> Result<u32, E> {
        let register = self.register(cpu, address, size)?;
        match (register, self.phase, &self.job) {
            (Register::Status, ..) => Ok(self.status),
            (Register::Control, Phase::Active, _) => Ok(CONTROL_START),
            (Register::Control, ..) => Ok(0),
            (Register::Word014, Phase::Completed, _) => Ok(0),
            (Register::Pointer, Phase::Completed, Some(job)) => Ok(job.word(Register::PointerBase).unwrap_or_default()),
            _ => Err(self.refuse(format!("read of {} in phase {}", register.name(), self.phase.name()))),
        }
    }

    pub(crate) fn write<B: Board>(
        &mut self,
        board: &B,
        delay: CompletionDelay,
        cpu: Option<Cpu>,
        address: u64,
        size: u64,
        value: Option<u32>,
    ) -> Result<(), B::Error> {
        let register = self.register(cpu, address, size)?;
        let (Some(cpu), Some(value)) = (cpu, value) else {
            return Err(self.refuse("value is not an unsigned 32-bit word".into()));
        };
        if self.phase == Phase::Active {
            return Err(self.refuse(format!("write of {} while a job is active", register.name())));
        }
        match register {
            Register::Command => {
                let admitted = value == COMMAND_STOP
                    && match self.phase {
                        Phase::Idle | Phase::Completed => true,
                        // Cleanup 0x00d99a2c resets without programming or starting another job.
                        Phase::Programming => {
                            self.status == 0
                                && self.programming.iter().all(Option::is_none)
                                && board.irq_quiescent()?
                        }
                        _ => false,
                    };
                if !admitted {
                    return Err(self.refuse(format!("command {value:#x} in phase {}", self.phase.name())));
                }
                self.status |= STATUS_STOPPED;
                self.phase = Phase::Stopping;
                Ok(())
            }
            Register::Status => {
                if value & !self.status != 0 {
                    return Err(self.refuse(format!("status clear {value:#x} names unlatched bits")));
                }
                self.status &= !value;
                if self.phase == Phase::Stopping && self.status & STATUS_STOPPED == 0 {
                    self.phase = Phase::Stopped;
                }
                Ok(())
            }
            Register::Control => {
                if value == CONTROL_RESET && self.phase == Phase::Stopped && self.status == 0 {
                    self.phase = Phase::Programming;
                    self.programming = [None; 10];
                    return Ok(());
                }
                if value == CONTROL_START && self.phase == Phase::Programming {
                    return self.start(board, delay, cpu);
                }
                Err(self.refuse(format!("control {value:#x} in phase {}", self.phase.name())))
            }
            Register::Word014 => {
                if value != 0 || self.phase != Phase::Completed {
                    return Err(self.refuse(format!("WORD_014 write {value:#x} in phase {}", self.phase.name())));
                }
                Ok(())
            }
            _ => {
                let Some(slot) = programmed_slot(register).filter(|_| self.phase == Phase::Programming) else {
                    return Err(self.refuse(format!("write of {} in phase {}", register.name(), self.phase.name())));
                };
                if value & !PROGRAMMED[slot].1 != 0 {
                    return Err(self.refuse(format!("{} value {value:#x} exceeds its driver field", register.name())));
                }
                self.programming[slot] = Some(value);
                Ok(())
            }
        }
    }

    fn start<B: Board>(&mut self, board: &B, delay: CompletionDelay, cpu: Cpu) -> Result<(), B::Error> {
        let missing: Vec<&str> = PROGRAMMED
            .iter()
            .zip(&self.programming)
            .filter(|(_, value)| value.is_none())
            .map(|((register, _), _)| register.name())
            .collect();
        if !missing.is_empty() {
            return Err(self.refuse(format!("start without programming of {}", missing.join(", "))));
        }
        let programming = self.programming.map(Option::unwrap_or_default);
        if programming[0] & STATUS_DONE != 0 {
            return Err(self.refuse("start with the completion source masked".into()));
        }
        let irq_cpu = self.route(board)?;
        let now = board.virtual_ticks()?;
        let Some(due_tick) = now.checked_add(u64::from(delay.0)) else {
            return Err(self.refuse("completion tick overflows".into()));
        };
        self.job = Some(Job { cpu, irq_cpu, started_tick: now, due_tick, programming });
        self.phase = Phase::Active;
        Ok(())
    }

    fn next_event_tick(&self, now: u64) -> Option<u64> {
        match (self.phase, &self.job) {
            (Phase::Active, Some(job)) => Some(now.max(job.due_tick)),
            _ => None,
        }
    }

    /// The CPU that receives IRQ 0x4c when the active job completes at `now`.
    pub(crate) fn tick<B: Board>(&mut self, board: &B, now: u64) -> Result<Option<Cpu>, B::Error> {
        let irq_cpu = match (self.phase, &self.job) {
            (Phase::Active, Some(job)) if now >= job.due_tick => job.irq_cpu,
            _ => return Ok(None),
        };
        if self.route(board)? != irq_cpu {
            return Err(self.refuse("IRQ76 owner changed during the job".into()));
        }
        self.status |= STATUS_DONE;
        self.phase = Phase::Completed;
        self.jobs += 1;
        Ok(Some(irq_cpu))
    }
}

fn invalid_state(field: &str) -> PyErr {
    PyValueError::new_err(format!("FFF63 engine state field {field} differs"))
}

fn exact_int(value: &Bound<'_, PyAny>) -> Option<u64> {
    if value.is_exact_instance_of::<PyInt>() { value.extract().ok() } else { None }
}

fn exact_word(value: &Bound<'_, PyAny>) -> Option<u32> {
    exact_int(value).and_then(|value| u32::try_from(value).ok())
}

fn field<'py>(state: &Bound<'py, PyDict>, name: &str) -> PyResult<Bound<'py, PyAny>> {
    state.get_item(name)?.ok_or_else(|| invalid_state(name))
}

fn int_field(state: &Bound<'_, PyDict>, name: &str) -> PyResult<u64> {
    exact_int(&field(state, name)?).ok_or_else(|| invalid_state(name))
}

fn word_field(state: &Bound<'_, PyDict>, name: &str) -> PyResult<u32> {
    exact_word(&field(state, name)?).ok_or_else(|| invalid_state(name))
}

fn cpu_field(state: &Bound<'_, PyDict>, name: &str) -> PyResult<Cpu> {
    Cpu::new(int_field(state, name)?).ok_or_else(|| invalid_state(name))
}

fn programming_dict<'py>(py: Python<'py>, words: impl Iterator<Item = (Register, u32)>) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    for (register, value) in words {
        dict.set_item(register.name(), value)?;
    }
    Ok(dict)
}

fn programming_from(value: &Bound<'_, PyAny>) -> PyResult<[Option<u32>; 10]> {
    let dict = value.cast::<PyDict>().map_err(|_| invalid_state("programming"))?;
    let mut programming = [None; 10];
    for (name, word) in dict.iter() {
        let slot = name.cast::<PyString>().ok().and_then(|name| programmed_by_name(name.to_str().ok()?));
        let word = exact_word(&word);
        match (slot, word) {
            (Some(slot), Some(word)) if word & !PROGRAMMED[slot].1 == 0 => programming[slot] = Some(word),
            _ => return Err(invalid_state("programming")),
        }
    }
    Ok(programming)
}

fn phase_of(state: &Bound<'_, PyDict>) -> PyResult<Phase> {
    match state.get_item("phase")? {
        None if state.is_empty() => Ok(Phase::Idle),
        None => Err(invalid_state("phase")),
        Some(phase) => phase
            .cast::<PyString>()
            .ok()
            .and_then(|phase| Phase::parse(phase.to_str().ok()?))
            .ok_or_else(|| invalid_state("phase")),
    }
}

impl Engine {
    fn load(state: &Bound<'_, PyDict>) -> PyResult<Self> {
        if state.is_empty() {
            return Ok(Self::default());
        }
        let job = field(state, "job")?;
        let job = if job.is_none() {
            None
        } else {
            let job = job.cast_into::<PyDict>().map_err(|_| invalid_state("job"))?;
            let programming = programming_from(&field(&job, "programming")?)?;
            Some(Job {
                cpu: cpu_field(&job, "cpu")?,
                irq_cpu: cpu_field(&job, "irq_cpu")?,
                started_tick: int_field(&job, "started_tick")?,
                due_tick: int_field(&job, "due_tick")?,
                programming: if programming.iter().all(Option::is_some) {
                    programming.map(Option::unwrap_or_default)
                } else {
                    return Err(invalid_state("job"));
                },
            })
        };
        let last_rejected = field(state, "last_rejected")?;
        let engine = Self {
            phase: phase_of(state)?,
            status: word_field(state, "status")?,
            programming: programming_from(&field(state, "programming")?)?,
            job,
            jobs: int_field(state, "jobs")?,
            last_rejected: if last_rejected.is_none() {
                None
            } else {
                Some(last_rejected.extract().map_err(|_| invalid_state("last_rejected"))?)
            },
        };
        if matches!(engine.phase, Phase::Active | Phase::Completed) && engine.job.is_none() {
            return Err(invalid_state("job"));
        }
        Ok(engine)
    }

    fn job_dict<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyDict>>> {
        let Some(job) = &self.job else { return Ok(None) };
        let dict = PyDict::new(py);
        dict.set_item("cpu", job.cpu.0)?;
        dict.set_item("irq_cpu", job.irq_cpu.0)?;
        dict.set_item("started_tick", job.started_tick)?;
        dict.set_item("due_tick", job.due_tick)?;
        let words = PROGRAMMED.iter().zip(job.programming).map(|((register, _), value)| (*register, value));
        dict.set_item("programming", programming_dict(py, words)?)?;
        Ok(Some(dict))
    }

    fn store(&self, state: &Bound<'_, PyDict>) -> PyResult<()> {
        let py = state.py();
        state.set_item("phase", self.phase.name())?;
        state.set_item("status", self.status)?;
        let words = PROGRAMMED
            .iter()
            .zip(self.programming)
            .filter_map(|((register, _), value)| Some((*register, value?)));
        state.set_item("programming", programming_dict(py, words)?)?;
        state.set_item("job", self.job_dict(py)?)?;
        state.set_item("jobs", self.jobs)?;
        state.set_item("last_rejected", self.last_rejected.as_deref())?;
        Ok(())
    }
}

impl From<Refusal> for PyErr {
    fn from(refusal: Refusal) -> Self {
        Python::attach(|py| {
            let message = format!("FFF63 engine: {}", refusal.0);
            match PyModule::import(py, "emulator.hardware").and_then(|module| module.getattr("UnsupportedDevice")) {
                Ok(class) => match class.call1((message,)) {
                    Ok(error) => PyErr::from_value(error),
                    Err(error) => error,
                },
                Err(error) => error,
            }
        })
    }
}

struct PyBoard<'py>(Bound<'py, PyAny>);

impl<'py> PyBoard<'py> {
    fn banks(&self) -> PyResult<Bound<'py, PyList>> {
        let py = self.0.py();
        let banks = self
            .0
            .getattr(pyo3::intern!(py, "interrupts"))?
            .getattr(pyo3::intern!(py, "banks"))?
            .cast_into::<PyList>()?;
        if banks.len() != 2 {
            return Err(PyValueError::new_err("FFF63 requires both interrupt banks"));
        }
        Ok(banks)
    }
}

fn holds_engine_line(value: &Bound<'_, PyAny>) -> PyResult<bool> {
    if value.is_none() {
        return Ok(false);
    }
    let line = value.extract::<u8>()?;
    if line >= 128 {
        return Err(PyValueError::new_err("Interrupt line exceeds controller range"));
    }
    Ok(line == ENGINE_IRQ)
}

impl Board for PyBoard<'_> {
    type Error = PyErr;

    fn virtual_ticks(&self) -> PyResult<u64> {
        self.0.getattr(pyo3::intern!(self.0.py(), "virtual_ticks"))?.extract()
    }

    fn irq_enabled(&self) -> PyResult<[bool; 2]> {
        let py = self.0.py();
        let banks = self.banks()?;
        let mut enabled = [false; 2];
        for (cpu, bank) in banks.iter().enumerate() {
            let mask = bank.getattr(pyo3::intern!(py, "enabled"))?.extract::<u128>()?;
            enabled[cpu] = mask & (1 << ENGINE_IRQ) != 0;
        }
        Ok(enabled)
    }

    fn irq_quiescent(&self) -> PyResult<bool> {
        let py = self.0.py();
        for bank in self.banks()?.iter() {
            let pending = bank.getattr(pyo3::intern!(py, "pending"))?.extract::<u128>()?;
            if pending & (1 << ENGINE_IRQ) != 0 {
                return Ok(false);
            }
            for name in [pyo3::intern!(py, "active"), pyo3::intern!(py, "injected")] {
                if holds_engine_line(&bank.getattr(name)?)? {
                    return Ok(false);
                }
            }
            let stack = bank.getattr(pyo3::intern!(py, "active_stack"))?;
            for entry in stack.cast::<PyList>()?.iter() {
                if holds_engine_line(&entry.get_item(pyo3::intern!(py, "line"))?)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

fn access_cpu(core: &Bound<'_, PyAny>) -> Option<Cpu> {
    exact_int(core).and_then(Cpu::new)
}

/// Board device for the 0xfff63000 engine (`fff63_profile` of `make_board`).
#[pyclass(dict, unsendable, module = "x70_native._native", name = "FFF63Engine")]
pub(crate) struct Device {
    board: Option<Py<PyAny>>,
    delay: CompletionDelay,
}

impl Device {
    fn board<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.board
            .as_ref()
            .map(|board| board.bind(py).clone())
            .ok_or_else(|| PyValueError::new_err("FFF63 engine has no board"))
    }

    fn state<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyDict>> {
        Ok(slf.getattr(pyo3::intern!(slf.py(), "__dict__"))?.cast_into::<PyDict>()?)
    }
}

#[pymethods]
impl Device {
    #[classattr]
    const BASE: u64 = ENGINE_BASE;
    #[classattr]
    const SIZE: u64 = ENGINE_SIZE;
    #[classattr]
    const IRQ: u8 = ENGINE_IRQ;

    #[classattr]
    fn quiet_ticks_defer() -> bool {
        true
    }

    #[new]
    #[pyo3(signature = (board, *, profile, completion_ticks = None))]
    fn new(
        board: Bound<'_, PyAny>,
        profile: &Bound<'_, PyAny>,
        completion_ticks: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        if !profile.extract::<String>().is_ok_and(|profile| profile == PROFILE) {
            return Err(PyValueError::new_err("FFF63 engine requires an explicit named profile"));
        }
        let delay = match completion_ticks {
            None => CompletionDelay(DEFAULT_COMPLETION_TICKS),
            Some(ticks) => exact_int(ticks).and_then(CompletionDelay::new).ok_or_else(|| {
                PyValueError::new_err("FFF63 completion delay must be a positive virtual tick count")
            })?,
        };
        let record = PyDict::new(board.py());
        record.set_item("base", format!("{ENGINE_BASE:#x}"))?;
        record.set_item("profile", PROFILE)?;
        record.set_item("evidence", "65A478/65A58C/65A548/D99570 byte-read register roles")?;
        record.set_item(
            "limits",
            "Stop/reset/start sequence of develop job 4 only; no pixels, output bytes or timing measurement",
        )?;
        board.call_method("record", ("FFF63 engine declared job completion",), Some(&record))?;
        Ok(Self { board: Some(board.unbind()), delay })
    }

    #[getter]
    fn busy(slf: &Bound<'_, Self>) -> PyResult<bool> {
        Ok(phase_of(&Self::state(slf)?)? == Phase::Active)
    }

    fn contains(&self, address: &Bound<'_, PyAny>, _size: &Bound<'_, PyAny>) -> bool {
        address.extract::<u64>().is_ok_and(|address| (ENGINE_BASE..ENGINE_BASE + ENGINE_SIZE).contains(&address))
    }

    fn mmio_ranges(&self) -> ((u64, u64),) {
        ((ENGINE_BASE, ENGINE_BASE + ENGINE_SIZE),)
    }

    fn read(slf: &Bound<'_, Self>, core: &Bound<'_, PyAny>, address: u64, size: u64) -> PyResult<u32> {
        let state = Self::state(slf)?;
        let mut engine = Engine::load(&state)?;
        let result = engine.read::<PyErr>(access_cpu(core), address, size);
        engine.store(&state)?;
        result
    }

    fn write(
        slf: &Bound<'_, Self>,
        core: &Bound<'_, PyAny>,
        address: u64,
        size: u64,
        value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let (board, delay) = {
            let device = slf.borrow();
            (PyBoard(device.board(slf.py())?), device.delay)
        };
        let state = Self::state(slf)?;
        let mut engine = Engine::load(&state)?;
        let result = engine.write(&board, delay, access_cpu(core), address, size, exact_word(value));
        engine.store(&state)?;
        result
    }

    fn next_event_tick(slf: &Bound<'_, Self>, now: u64) -> PyResult<Option<u64>> {
        let state = Self::state(slf)?;
        if phase_of(&state)? != Phase::Active {
            return Ok(None);
        }
        Ok(Engine::load(&state)?.next_event_tick(now))
    }

    fn tick(slf: &Bound<'_, Self>, board: &Bound<'_, PyAny>) -> PyResult<()> {
        let py = slf.py();
        let state = Self::state(slf)?;
        if phase_of(&state)? != Phase::Active {
            return Ok(());
        }
        let now: u64 = board.getattr(pyo3::intern!(py, "virtual_ticks"))?.extract()?;
        let owner = PyBoard(slf.borrow().board(py)?);
        let mut engine = Engine::load(&state)?;
        let result = engine.tick(&owner, now);
        engine.store(&state)?;
        let Some(cpu) = result? else { return Ok(()) };
        board.call_method1(pyo3::intern!(py, "queue_irq"), (cpu.0, ENGINE_IRQ))?;
        let record = PyDict::new(py);
        record.set_item("irq", ENGINE_IRQ)?;
        record.set_item("cpu", cpu.0)?;
        record.set_item("tick", now)?;
        let programming = PyDict::new(py);
        if let Some(job) = &engine.job {
            for ((register, _), value) in PROGRAMMED.iter().zip(job.programming) {
                programming.set_item(register.name(), format!("{value:#x}"))?;
            }
        }
        record.set_item("programming", programming)?;
        record.set_item("limits", "STATUS bit4 and IRQ76 only; no memory written")?;
        board.call_method(pyo3::intern!(py, "record"), ("FFF63 engine declared completion",), Some(&record))?;
        Ok(())
    }

    fn snapshot<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyDict>> {
        let py = slf.py();
        let engine = Engine::load(&Self::state(slf)?)?;
        let snapshot = PyDict::new(py);
        snapshot.set_item("name", "FFF63Engine")?;
        snapshot.set_item("base", format!("{ENGINE_BASE:#x}"))?;
        snapshot.set_item("phase", engine.phase.name())?;
        snapshot.set_item("status", format!("{:#x}", engine.status))?;
        snapshot.set_item("busy", engine.busy())?;
        snapshot.set_item("jobs", engine.jobs)?;
        snapshot.set_item("last_job", engine.job_dict(py)?)?;
        snapshot.set_item("last_rejected", engine.last_rejected.as_deref())?;
        let provenance = PyDict::new(py);
        provenance.set_item("profile", PROFILE)?;
        provenance.set_item(
            "evidence",
            [
                "byte-read: 65A478 writes COMMAND 1, polls STATUS bit16, writes the read STATUS back, \
                 writes CONTROL 0x100 and polls CONTROL bit8 clear",
                "byte-read: 65A548 sets CONTROL bit0; D99570 clears STATUS by writing it back and treats bit4 \
                 as success, bit17 and bit24 as other outcomes",
                "byte-read: IRQ descriptor 0x48 at 0x007C9484 is line 0x4C, priority 10",
            ],
        )?;
        provenance.set_item(
            "limits",
            [
                "Stop acknowledge, reset self-clear and completion are immediate or one declared delay, not measured",
                "Only the develop job 4 stop/program/start/complete sequence; COMMAND 4 continuation and STATUS \
                 bit17/24 are refused",
                "No memory written; POINTER reads POINTER_BASE and WORD_014 reads 0",
                "Unobserved registers, values and orders refuse",
            ],
        )?;
        snapshot.set_item("provenance", provenance)?;
        Ok(snapshot)
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.board)
    }

    fn __clear__(&mut self) {
        self.board = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::sync::PyOnceLock;

    const POINTER_BASE: u32 = 0x4b8d_a820;
    /// Words 0x0065a58c stored on the M dial turn (emulated, candidate-20261018-d).
    const PROGRAMMING: [(u64, u32); 10] = [
        (0x008, 0x00fd_0020),
        (0x0a0, 0),
        (0x100, POINTER_BASE),
        (0x104, 0x1b_9000),
        (0x10c, 0x4b88_f820),
        (0x110, 0x2_1f0c),
        (0x11c, 0),
        (0x120, 0),
        (0x124, 0),
        (0x128, 0),
    ];

    struct FakeBoard {
        now: u64,
        enabled: [bool; 2],
        quiescent: bool,
    }

    impl Board for FakeBoard {
        type Error = Refusal;

        fn virtual_ticks(&self) -> Result<u64, Refusal> {
            Ok(self.now)
        }

        fn irq_enabled(&self) -> Result<[bool; 2], Refusal> {
            Ok(self.enabled)
        }

        fn irq_quiescent(&self) -> Result<bool, Refusal> {
            Ok(self.quiescent)
        }
    }

    struct Bench {
        engine: Engine,
        board: FakeBoard,
    }

    impl Bench {
        fn new(enabled: [bool; 2]) -> Self {
            Self { engine: Engine::default(), board: FakeBoard { now: 0, enabled, quiescent: true } }
        }

        fn read(&mut self, offset: u64) -> Result<u32, Refusal> {
            self.engine.read(Some(Cpu(0)), ENGINE_BASE + offset, 4)
        }

        fn write(&mut self, offset: u64, value: u32) -> Result<(), Refusal> {
            self.engine.write(&self.board, CompletionDelay(2), Some(Cpu(0)), ENGINE_BASE + offset, 4, Some(value))
        }

        /// The 0x0065a478 sequence.
        fn stop_and_reset(&mut self) {
            self.write(0x118, 1).unwrap();
            let status = self.read(0x004).unwrap();
            assert_ne!(status & 1 << 16, 0);
            self.write(0x004, status).unwrap();
            self.write(0x000, 0x100).unwrap();
            assert_eq!(self.read(0x000).unwrap() & 0x100, 0);
        }

        fn start_job(&mut self) -> Result<(), Refusal> {
            self.stop_and_reset();
            for (offset, value) in PROGRAMMING {
                self.write(offset, value)?;
            }
            let control = self.read(0x000)?;
            self.write(0x000, control | 1)
        }

        fn run_until_completion(&mut self) -> Cpu {
            for now in self.board.now..self.board.now + 100 {
                if let Some(cpu) = self.engine.tick(&self.board, now).unwrap() {
                    return cpu;
                }
            }
            panic!("job did not complete");
        }
    }

    #[test]
    fn stop_poll_sees_bit16_and_write_back_clears_it() {
        let mut bench = Bench::new([true, false]);
        bench.write(0x118, 1).unwrap();
        let status = bench.read(0x004).unwrap();
        assert_eq!(status, 1 << 16);
        bench.write(0x004, status).unwrap();
        assert_eq!(bench.read(0x004).unwrap(), 0);
    }

    #[test]
    fn job_completes_after_the_declared_delay_with_success_status_and_irq76_to_the_enabled_cpu() {
        let mut bench = Bench::new([true, false]);
        bench.board.now = 100;
        bench.start_job().unwrap();
        assert!(bench.engine.busy());
        assert_eq!(bench.read(0x000).unwrap() & 1, 1);
        assert_eq!(bench.engine.next_event_tick(100), Some(102));
        assert_eq!(bench.engine.tick(&bench.board, 101).unwrap(), None);
        assert_eq!(bench.engine.tick(&bench.board, 102).unwrap(), Some(Cpu(0)));
        assert!(!bench.engine.busy());
        assert_eq!(bench.engine.next_event_tick(102), None);
        let status = bench.read(0x004).unwrap();
        assert_eq!(status & 0x0102_0010, 0x10);
        // The 0x00d99570 success arm.
        bench.write(0x004, status).unwrap();
        let word = bench.read(0x014).unwrap();
        bench.write(0x014, word).unwrap();
        assert_eq!(bench.read(0x108).unwrap(), POINTER_BASE);
        // 0x00d99a24 stops the engine again.
        bench.stop_and_reset();
    }

    #[test]
    fn irq_goes_to_cpu1_when_cpu1_owns_the_line() {
        let mut bench = Bench::new([false, true]);
        bench.start_job().unwrap();
        assert_eq!(bench.run_until_completion(), Cpu(1));
    }

    #[test]
    fn completion_refuses_when_the_irq76_owner_changed() {
        let mut bench = Bench::new([true, false]);
        bench.start_job().unwrap();
        bench.board.enabled = [false, true];
        assert!(bench.engine.tick(&bench.board, 2).is_err());
        assert!(bench.engine.busy());
    }

    #[test]
    fn registers_refuse_writes_while_active() {
        let mut bench = Bench::new([true, false]);
        bench.start_job().unwrap();
        for (offset, value) in [(0x118, 1), (0x004, 0), (0x000, 0x100), (0x10c, 0x4200_0000)] {
            assert!(bench.write(offset, value).is_err(), "write {offset:#x}");
        }
        assert!(bench.engine.busy());
    }

    #[test]
    fn start_refuses_without_complete_programming() {
        let mut bench = Bench::new([true, false]);
        bench.stop_and_reset();
        bench.write(0x10c, 0x4200_0000).unwrap();
        assert!(bench.write(0x000, 1).is_err());
        assert!(!bench.engine.busy());
    }

    #[test]
    fn start_refuses_a_masked_completion_source() {
        let mut bench = Bench::new([true, false]);
        bench.stop_and_reset();
        for (offset, value) in PROGRAMMING {
            bench.write(offset, if offset == 0x008 { value | 0x10 } else { value }).unwrap();
        }
        assert!(bench.write(0x000, 1).is_err());
    }

    #[test]
    fn start_refuses_without_a_single_irq76_owner() {
        for enabled in [[false, false], [true, true]] {
            assert!(Bench::new(enabled).start_job().is_err(), "{enabled:?}");
        }
    }

    #[test]
    fn completion_tick_overflow_refuses() {
        let mut bench = Bench::new([true, false]);
        bench.board.now = u64::MAX - 1;
        assert!(bench.start_job().is_err());
    }

    #[test]
    fn programming_refuses_before_reset_and_beyond_driver_fields() {
        let mut bench = Bench::new([true, false]);
        assert!(bench.write(0x10c, 0x4200_0000).is_err());
        bench.stop_and_reset();
        for (offset, value) in [(0x008, 0x0200_0000), (0x0a0, 4), (0x11c, 0x40), (0x120, 2), (0x108, 0)] {
            assert!(bench.write(offset, value).is_err(), "write {offset:#x}");
        }
    }

    #[test]
    fn unobserved_commands_registers_clears_and_accesses_refuse() {
        let mut bench = Bench::new([true, false]);
        assert!(bench.write(0x118, 4).is_err());
        assert!(bench.read(0x018).is_err());
        assert!(bench.read(0x010).is_err());
        assert!(bench.read(0x12c).is_err());
        assert!(bench.write(0x004, 0x10).is_err());
        assert!(bench.engine.read::<Refusal>(Some(Cpu(0)), ENGINE_BASE + 4, 2).is_err());
        assert!(bench.engine.read::<Refusal>(Some(Cpu(0)), ENGINE_BASE + 6, 4).is_err());
        assert!(bench.engine.read::<Refusal>(None, ENGINE_BASE + 4, 4).is_err());
        let board = FakeBoard { now: 0, enabled: [true, false], quiescent: true };
        assert!(bench.engine.write(&board, CompletionDelay(2), Some(Cpu(0)), ENGINE_BASE + 0x118, 4, None).is_err());
    }

    #[test]
    fn quiescent_cleanup_reset_admits_another_stop_acknowledgement() {
        let mut bench = Bench::new([true, false]);
        bench.stop_and_reset();
        bench.board.quiescent = false;
        assert!(bench.write(0x118, 1).is_err());
        bench.board.quiescent = true;
        bench.stop_and_reset();
        bench.write(0x10c, 0x4200_0000).unwrap();
        assert!(bench.write(0x118, 1).is_err());
    }

    #[test]
    fn stop_refuses_while_acknowledged_or_stopped() {
        let mut bench = Bench::new([true, false]);
        bench.write(0x118, 1).unwrap();
        assert!(bench.write(0x118, 1).is_err());
        let status = bench.read(0x004).unwrap();
        bench.write(0x004, status).unwrap();
        assert!(bench.write(0x118, 1).is_err());
    }

    static HARDWARE: PyOnceLock<Py<PyModule>> = PyOnceLock::new();

    /// `emulator.hardware.UnsupportedDevice`, from a stand-in module when the
    /// emulator package is absent.
    fn unsupported_device(py: Python<'_>) -> Bound<'_, PyAny> {
        let module = HARDWARE.get_or_init(py, || {
            let module = PyModule::new(py, "emulator.hardware").unwrap();
            py.run(c"class UnsupportedDevice(RuntimeError):\n    pass\n", Some(&module.dict()), None).unwrap();
            module.unbind()
        });
        let modules = py.import("sys").unwrap().getattr("modules").unwrap();
        modules.call_method1("setdefault", ("emulator", PyModule::new(py, "emulator").unwrap())).unwrap();
        let module = modules.call_method1("setdefault", ("emulator.hardware", module)).unwrap();
        module.getattr("UnsupportedDevice").unwrap()
    }

    fn python_board(py: Python<'_>, enabled: [bool; 2]) -> Bound<'_, PyAny> {
        let namespace = py.import("types").unwrap().getattr("SimpleNamespace").unwrap();
        let mock = py.import("unittest.mock").unwrap().getattr("Mock").unwrap();
        let banks = PyList::empty(py);
        for owner in enabled {
            let bank = namespace.call0().unwrap();
            bank.setattr("enabled", if owner { 1u128 << 0x4c } else { 0 }).unwrap();
            bank.setattr("pending", 0).unwrap();
            bank.setattr("active", py.None()).unwrap();
            bank.setattr("injected", py.None()).unwrap();
            bank.setattr("active_stack", PyList::empty(py)).unwrap();
            banks.append(bank).unwrap();
        }
        let interrupts = namespace.call0().unwrap();
        interrupts.setattr("banks", banks).unwrap();
        let board = namespace.call0().unwrap();
        board.setattr("interrupts", interrupts).unwrap();
        board.setattr("virtual_ticks", 0).unwrap();
        board.setattr("queue_irq", mock.call0().unwrap()).unwrap();
        board.setattr("record", mock.call0().unwrap()).unwrap();
        board
    }

    fn python_engine<'py>(board: &Bound<'py, PyAny>, completion_ticks: Option<Bound<'py, PyAny>>) -> PyResult<Bound<'py, PyAny>> {
        let py = board.py();
        let options = PyDict::new(py);
        options.set_item("profile", PROFILE)?;
        if let Some(ticks) = completion_ticks {
            options.set_item("completion_ticks", ticks)?;
        }
        py.get_type::<Device>().call((board,), Some(&options))
    }

    fn python_write(device: &Bound<'_, PyAny>, offset: u64, value: u32) -> PyResult<()> {
        device.call_method1("write", (0, ENGINE_BASE + offset, 4, value)).map(drop)
    }

    fn python_read(device: &Bound<'_, PyAny>, offset: u64) -> PyResult<u32> {
        device.call_method1("read", (0, ENGINE_BASE + offset, 4))?.extract()
    }

    fn python_start(device: &Bound<'_, PyAny>) -> PyResult<()> {
        python_write(device, 0x118, 1)?;
        let status = python_read(device, 0x004)?;
        python_write(device, 0x004, status)?;
        python_write(device, 0x000, 0x100)?;
        for (offset, value) in PROGRAMMING {
            python_write(device, offset, value)?;
        }
        python_write(device, 0x000, 1)
    }

    fn snapshot_field<'py>(device: &Bound<'py, PyAny>, name: &str) -> Bound<'py, PyAny> {
        device.call_method0("snapshot").unwrap().get_item(name).unwrap()
    }

    #[test]
    fn board_protocol_runs_the_job_and_raises_unsupported_device() {
        Python::initialize();
        Python::attach(|py| {
            let unsupported = unsupported_device(py);
            let board = python_board(py, [false, true]);
            let device = python_engine(&board, None).unwrap();
            let record = board.getattr("record").unwrap();
            assert_eq!(record.getattr("call_count").unwrap().extract::<usize>().unwrap(), 1);
            let ranges: Vec<(u64, u64)> = device.call_method0("mmio_ranges").unwrap().extract().unwrap();
            assert_eq!(ranges, [(0xfff6_3000, 0xfff6_312c)]);
            for (address, inside) in [(0xfff6_3128u64, true), (0xfff6_312c, false), (0xfff6_2ffc, false)] {
                assert_eq!(device.call_method1("contains", (address, 4)).unwrap().extract::<bool>().unwrap(), inside);
            }
            assert!(device.getattr("quiet_ticks_defer").unwrap().extract::<bool>().unwrap());
            python_start(&device).unwrap();
            assert!(device.getattr("busy").unwrap().extract::<bool>().unwrap());
            assert_eq!(device.call_method1("next_event_tick", (0,)).unwrap().extract::<Option<u64>>().unwrap(), Some(2));
            device.call_method1("tick", (&board,)).unwrap();
            let queue = board.getattr("queue_irq").unwrap();
            assert_eq!(queue.getattr("call_count").unwrap().extract::<usize>().unwrap(), 0);
            board.setattr("virtual_ticks", 2).unwrap();
            device.call_method1("tick", (&board,)).unwrap();
            let raised: Vec<(u8, u8)> = queue.getattr("call_args_list").unwrap().cast::<PyList>().unwrap().iter()
                .map(|call| call.getattr("args").unwrap().extract().unwrap()).collect();
            assert_eq!(raised, [(1, 0x4c)]);
            assert_eq!(record.getattr("call_count").unwrap().extract::<usize>().unwrap(), 2);
            assert!(!device.getattr("busy").unwrap().extract::<bool>().unwrap());
            assert_eq!(snapshot_field(&device, "phase").extract::<String>().unwrap(), "completed");
            assert_eq!(snapshot_field(&device, "jobs").extract::<u64>().unwrap(), 1);
            assert!(snapshot_field(&device, "last_rejected").is_none());
            let refusal = python_write(&device, 0x118, 4).unwrap_err();
            assert!(refusal.value(py).is_instance(&unsupported).unwrap(), "{refusal}");
            assert!(!snapshot_field(&device, "last_rejected").is_none());
            let refusal = device.call_method1("read", (true, ENGINE_BASE + 4, 4)).unwrap_err();
            assert!(refusal.value(py).is_instance(&unsupported).unwrap(), "{refusal}");
        });
    }

    #[test]
    fn cleanup_stop_requires_irq76_quiescence_in_both_controllers() {
        Python::initialize();
        Python::attach(|py| {
            let unsupported = unsupported_device(py);
            fn reset<'py>(board: &Bound<'py, PyAny>) -> Bound<'py, PyAny> {
                let device = python_engine(board, None).unwrap();
                python_write(&device, 0x118, 1).unwrap();
                python_write(&device, 0x004, 1 << 16).unwrap();
                python_write(&device, 0x000, 0x100).unwrap();
                device
            }
            for bank_index in 0..2 {
                for field in ["pending", "active", "injected", "active_stack"] {
                    let board = python_board(py, [true, false]);
                    let device = reset(&board);
                    let bank = board.getattr("interrupts").unwrap().getattr("banks").unwrap().get_item(bank_index).unwrap();
                    match field {
                        "pending" => bank.setattr(field, 1u128 << 76).unwrap(),
                        "active_stack" => {
                            let ancestor = PyDict::new(py);
                            ancestor.set_item("line", 76).unwrap();
                            bank.getattr(field).unwrap().cast::<PyList>().unwrap().append(ancestor).unwrap();
                        }
                        _ => bank.setattr(field, 76).unwrap(),
                    }
                    let refusal = python_write(&device, 0x118, 1).unwrap_err();
                    assert!(refusal.value(py).is_instance(&unsupported).unwrap(), "bank {bank_index} {field}: {refusal}");
                }
            }
            let board = python_board(py, [true, false]);
            let device = reset(&board);
            board.getattr("interrupts").unwrap().getattr("banks").unwrap().get_item(0).unwrap()
                .setattr("pending", 1u128 << 67).unwrap();
            python_write(&device, 0x118, 1).unwrap();
            assert_eq!(python_read(&device, 0x004).unwrap(), 1 << 16);
        });
    }

    #[test]
    fn instance_dictionary_carries_state_into_a_fresh_device() {
        Python::initialize();
        Python::attach(|py| {
            let deepcopy = py.import("copy").unwrap().getattr("deepcopy").unwrap();
            let restore = |device: &Bound<'_, PyAny>, state: &Bound<'_, PyAny>| {
                let dict = device.getattr("__dict__").unwrap();
                dict.call_method0("clear").unwrap();
                dict.call_method1("update", (deepcopy.call1((state,)).unwrap(),)).unwrap();
            };
            let board = python_board(py, [true, false]);
            let original = python_engine(&board, None).unwrap();
            python_start(&original).unwrap();
            let active = deepcopy.call1((original.getattr("__dict__").unwrap(),)).unwrap();
            let resumed = python_engine(&board, None).unwrap();
            restore(&resumed, &active);
            assert!(resumed.getattr("busy").unwrap().extract::<bool>().unwrap());
            board.setattr("virtual_ticks", 2).unwrap();
            resumed.call_method1("tick", (&board,)).unwrap();
            assert_eq!(python_read(&resumed, 0x108).unwrap(), POINTER_BASE);
            original.call_method1("tick", (&board,)).unwrap();
            assert!(original.call_method0("snapshot").unwrap().eq(resumed.call_method0("snapshot").unwrap()).unwrap());
            let blank = PyDict::new(py);
            restore(&resumed, &blank);
            assert_eq!(snapshot_field(&resumed, "phase").extract::<String>().unwrap(), "idle");
            assert_eq!(snapshot_field(&resumed, "jobs").extract::<u64>().unwrap(), 0);
            let corrupt = deepcopy.call1((original.getattr("__dict__").unwrap(),)).unwrap();
            corrupt.set_item("phase", "running").unwrap();
            restore(&resumed, &corrupt);
            assert!(python_read(&resumed, 0x004).unwrap_err().is_instance_of::<PyValueError>(py));
            let orphan = deepcopy.call1((original.getattr("__dict__").unwrap(),)).unwrap();
            orphan.set_item("job", py.None()).unwrap();
            restore(&resumed, &orphan);
            assert!(python_read(&resumed, 0x004).unwrap_err().is_instance_of::<PyValueError>(py));
        });
    }

    #[test]
    fn construction_requires_the_named_profile_and_a_positive_delay() {
        Python::initialize();
        Python::attach(|py| {
            let board = python_board(py, [true, false]);
            for profile in [None, Some("other")] {
                let options = PyDict::new(py);
                options.set_item("profile", profile).unwrap();
                let error = py.get_type::<Device>().call((&board,), Some(&options)).unwrap_err();
                assert!(error.is_instance_of::<PyValueError>(py));
            }
            let builtins = py.import("builtins").unwrap();
            for ticks in [0i64, -1, 1 << 32] {
                let error = python_engine(&board, Some(ticks.into_pyobject(py).unwrap().into_any())).unwrap_err();
                assert!(error.is_instance_of::<PyValueError>(py), "{ticks}");
            }
            let error = python_engine(&board, Some(builtins.getattr("True").unwrap())).unwrap_err();
            assert!(error.is_instance_of::<PyValueError>(py));
            let device = python_engine(&board, Some(1u32.into_pyobject(py).unwrap().into_any())).unwrap();
            python_start(&device).unwrap();
            assert_eq!(device.call_method1("next_event_tick", (0,)).unwrap().extract::<Option<u64>>().unwrap(), Some(1));
        });
    }
}
