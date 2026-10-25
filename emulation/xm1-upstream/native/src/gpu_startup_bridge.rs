use crate::gpu_startup::{AckRegister, CoreId, Device, PathWords, RamBytes, VirtualTick};
use pyo3::prelude::*;
use pyo3::pyclass::{PyTraverseError, PyVisit};
use pyo3::types::{PyDict, PyList, PyModule, PyString};

pub(crate) const PROFILE: &str = "xm1-150-init-no-draw";

struct FrameBytes(u64);

fn refused(py: Python<'_>, message: impl Into<String>) -> PyErr {
    let message = message.into();
    match PyModule::import(py, "emulator.hardware")
        .and_then(|module| module.getattr("UnsupportedDevice"))
        .and_then(|class| class.call1((message,)))
    {
        Ok(value) => PyErr::from_value(value),
        Err(error) => error,
    }
}

fn selected(device: &Bound<'_, PyAny>) -> PyResult<bool> {
    let profile = device.getattr("clear_profile")?;
    Ok(!profile.is_none() && profile.extract::<&str>()? == PROFILE)
}

#[derive(Clone, Copy)]
enum Stream {
    Path,
    Fill,
}

impl Stream {
    fn name(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Fill => "fill",
        }
    }
}

fn captured_words<'py>(capture: &Bound<'py, PyDict>, bank: Stream) -> PyResult<Bound<'py, PyList>> {
    let py = capture.py();
    let streams = capture.get_item("streams")?
        .ok_or_else(|| refused(py, "GPU startup capture has no streams"))?;
    let bank = bank.name();
    let stream = streams.get_item(bank)?;
    if stream.contains("capture_error")? || stream.get_item("truncated")?.is_truthy()? {
        return Err(refused(py, "GPU startup requires a complete command capture"));
    }
    let source = stream.get_item("words")?;
    source.cast_into::<PyList>().map_err(Into::into)
}

fn words<const N: usize>(source: &Bound<'_, PyList>, bank: Stream) -> PyResult<[u32; N]> {
    let py = source.py();
    let bank = bank.name();
    if source.len() != N {
        return Err(refused(py, format!("GPU startup {bank} word count {} != {N}", source.len())));
    }
    let mut result = [0; N];
    for (slot, value) in result.iter_mut().zip(source.iter()) {
        let text = value.cast::<PyString>()?;
        let text = text.to_str()?;
        *slot = u32::from_str_radix(text.strip_prefix("0x").unwrap_or(text), 16)
            .map_err(|_| refused(py, "GPU startup capture contains an invalid command word"))?;
    }
    Ok(result)
}

fn path_words(capture: &Bound<'_, PyDict>) -> PyResult<PathWords> {
    let source = captured_words(capture, Stream::Path)?;
    match source.len() {
        16 => Ok(PathWords::Initialized(words(&source, Stream::Path)?)),
        32 => Ok(PathWords::FirstGlobal(words(&source, Stream::Path)?)),
        length => Err(refused(capture.py(), format!("GPU startup path word count {length} is not 16 or 32"))),
    }
}

#[pyclass(unsendable)]
struct NativeDevice {
    inner: Device,
}

fn native<'py>(device: &Bound<'py, PyAny>) -> PyResult<Bound<'py, NativeDevice>> {
    if !device.hasattr("_startup_native")? {
        device.setattr("_startup_native", Py::new(device.py(), NativeDevice { inner: Device::new() })?)?;
    }
    device.getattr("_startup_native")?.cast_into::<NativeDevice>().map_err(Into::into)
}

enum Counter {
    Started,
    Completed,
    Interrupts,
}

fn increment(device: &Bound<'_, PyAny>, counter: Counter) -> PyResult<()> {
    let name = match counter {
        Counter::Started => "jobs_started",
        Counter::Completed => "jobs_completed",
        Counter::Interrupts => "irqs_raised",
    };
    device.setattr(name, device.getattr(name)?.extract::<u64>()? + 1)
}

#[pyclass(unsendable)]
struct StartupCompletion {
    device: Py<PyAny>,
    board: Py<PyAny>,
    native: Py<NativeDevice>,
    capture: Py<PyDict>,
}

#[pymethods]
impl StartupCompletion {
    fn __call__(&self, py: Python<'_>) -> PyResult<()> {
        let device = self.device.bind(py);
        let now = VirtualTick(self.board.bind(py).getattr("virtual_ticks")?.extract()?);
        let completion = self.native.borrow_mut(py).inner.complete(now)
            .map_err(|error| refused(py, format!("GPU startup completion: {error:?}")))?
            .ok_or_else(|| refused(py, "GPU startup callback has no due completion"))?;
        let registers = device.getattr("regs")?;
        let registers = registers.cast::<PyDict>()?;
        let status = registers.get_item(0x34)?.map_or(Ok(0), |value| value.extract::<u32>())?;
        registers.set_item(0x34, status | completion.status)?;
        device.setattr("busy", false)?;
        increment(device, Counter::Completed)?;
        let capture = self.capture.bind(py);
        capture.set_item("completed", true)?;
        device.call_method("record", ("X-M1 GPU startup init completed",), Some(capture))?;
        increment(device, Counter::Interrupts)?;
        device.getattr("raise_irq")?.call1((completion.core.0, completion.irq))?;
        Ok(())
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.device)?;
        visit.call(&self.board)?;
        visit.call(&self.native)?;
        visit.call(&self.capture)
    }
}

#[pyfunction]
pub(crate) fn gpu_startup_ack(device: &Bound<'_, PyAny>, offset: u32, value: u32) -> PyResult<()> {
    if !selected(device)? || !device.hasattr("_startup_native")? {
        return Ok(());
    }
    let register = match offset {
        0x1800 => AckRegister::Path,
        0x1020 => AckRegister::Fill,
        _ => return Ok(()),
    };
    let native = native(device)?;
    if native.borrow().inner.status() != 0 {
        native.borrow_mut().inner.acknowledge(register, value)
            .map_err(|error| refused(device.py(), format!("GPU startup acknowledgment: {error:?}")))?;
    } else if device.getattr("busy")?.is_truthy()? {
        return Err(refused(device.py(), "GPU startup abort before completion is unsupported"));
    }
    Ok(())
}

#[pyfunction]
pub(crate) fn gpu_startup_submit(
    device: &Bound<'_, PyAny>,
    cpu: u8,
    offset: u32,
    trigger: u32,
    capture: &Bound<'_, PyDict>,
) -> PyResult<bool> {
    if !selected(device)? {
        return Ok(false);
    }
    let py = device.py();
    let submission = match (offset, trigger) {
        (0x10d0, 0x10001) => Stream::Path,
        (0x10e0, 1) => Stream::Fill,
        _ => return Err(refused(py, "GPU startup trigger is outside the paired initialization protocol")),
    };
    let ram = RamBytes(device.getattr("ram_bytes")?.extract()?);
    let path = path_words(capture)?;
    let path = path.as_slice();
    let frame_bytes = FrameBytes(u64::from(path[3] >> 16) * u64::from(path[3] & 0xffff) * 4);
    let frame_limit = FrameBytes(device.getattr("max_clear_bytes")?.extract()?);
    if frame_bytes.0 > frame_limit.0 {
        return Err(refused(py, "GPU startup frame exceeds the configured byte bound"));
    }
    let native = native(device)?;
    if matches!(submission, Stream::Path) {
        native.borrow_mut().inner.start_path(CoreId(cpu), path, ram)
            .map_err(|error| refused(py, format!("GPU startup path: {error:?}")))?;
        let pending = PyDict::new(py);
        pending.set_item("cpu", cpu)?;
        pending.set_item("profile", PROFILE)?;
        device.setattr("_command_pending", pending)?;
        capture.set_item("accepted", true)?;
        capture.set_item("stage", "state-only path validated; awaiting separate fill submission")?;
        device.call_method("record", ("X-M1 GPU startup path validated",), Some(capture))?;
        return Ok(true);
    }
    let fill = words::<592>(&captured_words(capture, Stream::Fill)?, Stream::Fill)?;
    let targets = device.getattr("irq_targets")?;
    if targets.is_none() {
        return Err(refused(py, "GPU startup requires firmware-enabled IRQ routing"));
    }
    let targets = targets.call1((0x3c,))?;
    let targets = targets.cast::<PyList>()?;
    if targets.len() != 1 {
        return Err(refused(py, "GPU startup IRQ has missing or ambiguous firmware-enabled bank"));
    }
    let irq_core = CoreId(targets.get_item(0)?.extract()?);
    let schedule = device.getattr("schedule")?;
    let board = schedule.getattr("__self__")?;
    let now = VirtualTick(board.getattr("virtual_ticks")?.extract()?);
    let due = native.borrow_mut().inner.start_fill(CoreId(cpu), path, &fill, irq_core, now, ram)
        .map_err(|error| refused(py, format!("GPU startup fill: {error:?}")))?;
    let delay = due.0 - now.0;
    device.setattr("_command_pending", py.None())?;
    device.getattr("_clear_ack_offsets")?.call_method0("clear")?;
    device.setattr("busy", true)?;
    increment(device, Counter::Started)?;
    capture.set_item("operation", "initialize")?;
    capture.set_item("profile", PROFILE)?;
    capture.set_item("accepted", true)?;
    capture.set_item("changed_bytes", 0)?;
    capture.set_item("irq_cpu", irq_core.0)?;
    capture.set_item("latency_ticks", delay)?;
    capture.set_item("pixel_semantics", "No geometry or pixel reads/writes; bounded register initialization only")?;
    capture.set_item("completion_semantics", "Declared two-tick bit31 and physical IRQ3c; actual ISR acknowledges both zero stores and wakes submitting task")?;
    device.call_method("record", ("X-M1 GPU startup init started",), Some(capture))?;
    let callback = Py::new(py, StartupCompletion {
        device: device.clone().unbind(),
        board: board.unbind(),
        native: native.unbind(),
        capture: capture.clone().unbind(),
    })?;
    schedule.call1((delay, callback))?;
    Ok(true)
}
