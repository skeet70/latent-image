use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

#[derive(Clone, Copy)]
enum PhaseSource {
    Primary,
    Secondary,
}

impl PhaseSource {
    fn index(self) -> u8 {
        match self { Self::Primary => 0, Self::Secondary => 1 }
    }

    fn irq(self) -> u8 {
        match self { Self::Primary => 0x27, Self::Secondary => 0x29 }
    }

    fn acknowledge(self) -> u8 {
        match self { Self::Primary => 1, Self::Secondary => 0 }
    }
}

struct VirtualTick(u64);
struct ClockPeriod(u64);

enum ClockCounter {
    Arms,
    Assertions,
}

fn increment(device: &Bound<'_, PyAny>, counter: ClockCounter) -> PyResult<()> {
    let name = match counter {
        ClockCounter::Arms => pyo3::intern!(device.py(), "event_arms"),
        ClockCounter::Assertions => pyo3::intern!(device.py(), "event_assertions"),
    };
    device.setattr(name, device.getattr(name)?.extract::<u64>()? + 1)
}

#[pyfunction]
pub(crate) fn tick_xm1_clock(
    device: &Bound<'_, PyAny>,
    board: &Bound<'_, PyAny>,
    edge_type: &Bound<'_, PyAny>,
    source_type: &Bound<'_, PyAny>,
) -> PyResult<()> {
    let py = board.py();
    let registers = device.getattr(pyo3::intern!(py, "registers"))?;
    let registers = registers.cast::<PyDict>()?;
    let register = |offset| -> PyResult<u64> {
        registers.get_item(offset)?.map_or(Ok(0), |value| value.extract())
    };
    let mask = register(0x38c)?;
    let enabled = !device.getattr(pyo3::intern!(py, "busy"))?.is_truthy()? && register(0)? & 1 != 0;
    let flags = (if mask & 0x200 != 0 { 2 } else { 0 })
        | (if mask & 2 != 0 { 0x200 } else { 0 });
    let count = register(0x24)?;
    let now = VirtualTick(board.getattr(pyo3::intern!(py, "virtual_ticks"))?.extract()?);
    let due = device.getattr(pyo3::intern!(py, "phase_clock_due"))?;
    let due = due.cast::<PyDict>()?;
    let pending = device.getattr(pyo3::intern!(py, "phase_clock_pending"))?;
    let pending = pending.cast::<PyDict>()?;
    for source in [PhaseSource::Primary, PhaseSource::Secondary] {
        let configured = enabled && match source {
            PhaseSource::Primary => mask & 1 != 0,
            PhaseSource::Secondary => flags != 0 && count != 0,
        };
        if !configured && due.contains(source.index())? {
            due.del_item(source.index())?;
        }
    }
    for source in [PhaseSource::Primary, PhaseSource::Secondary] {
        let configuration = match source {
            PhaseSource::Primary if enabled && mask & 1 != 0 =>
                Some((1, ClockPeriod(device.getattr(pyo3::intern!(py, "event_ticks"))?.extract()?))),
            PhaseSource::Secondary if enabled && flags != 0 && count != 0 =>
                Some((flags, ClockPeriod(count))),
            _ => None,
        };
        let Some((flags, period)) = configuration else {
            continue;
        };
        let edge = pending.get_item(source.index())?;
        if let Some(edge) = &edge {
            if edge.getattr(pyo3::intern!(py, "cpu"))?.is_none() {
                let owner = device.call_method1("_event_route", (source.irq(),))?;
                if !owner.is_none() {
                    edge.setattr("cpu", &owner)?;
                    board.call_method1("queue_irq", (&owner, source.irq()))?;
                    let record = PyDict::new(board.py());
                    record.set_item("source", source.index())?;
                    record.set_item("cpu", owner)?;
                    record.set_item("line", source.irq())?;
                    board.call_method("record", ("X-M1 pending phase-clock IRQ routed",), Some(&record))?;
                }
            }
        }
        let Some(deadline) = due.get_item(source.index())? else {
            due.set_item(source_type.call1((source.index(),))?, now.0 + period.0)?;
            increment(device, ClockCounter::Arms)?;
            continue;
        };
        let deadline = VirtualTick(deadline.extract()?);
        if now.0 < deadline.0 { continue; }
        due.set_item(source.index(), deadline.0 + ((now.0 - deadline.0) / period.0 + 1) * period.0)?;
        increment(device, ClockCounter::Arms)?;
        if edge.is_some() { continue; }
        let owner = device.call_method1("_event_route", (source.irq(),))?;
        let generation = device.getattr("event_enable_generation")?;
        let edge = edge_type.call1((&owner, flags, now.0, &generation))?;
        let source_value = source_type.call1((source.index(),))?;
        pending.set_item(&source_value, edge)?;
        registers.set_item(0x384, register(0x384)? | flags)?;
        device.setattr("event_pending", true)?;
        device.setattr("event_source", source_value)?;
        device.setattr("event_irq", source.irq())?;
        device.setattr("event_ack_selector", source.acknowledge())?;
        increment(device, ClockCounter::Assertions)?;
        let record = PyDict::new(board.py());
        record.set_item("profile", "xm1-150-configured-phase-clock-unmeasured")?;
        record.set_item("source", source.index())?;
        record.set_item("physical_irq", format!("{:#x}", source.irq()))?;
        record.set_item("cpu", &owner)?;
        record.set_item("generation", generation)?;
        record.set_item("asserted_tick", now.0)?;
        record.set_item("state", "asserted")?;
        record.set_item("status_mask", format!("{flags:#x}"))?;
        record.set_item("period_ticks", period.0)?;
        record.set_item("limits", "Independent external clocks; source1 period follows retained FFF10024 count under a declared 1:1 tick law; source0 period, physical waveform and pixel/clock ratio are unmeasured")?;
        device.setattr("last_event", &record)?;
        let history = device.getattr("event_history")?;
        let history = history.cast::<PyList>()?;
        history.append(record.copy()?)?;
        device.setattr("event_history", history.get_slice(history.len().saturating_sub(32), history.len()))?;
        if !owner.is_none() {
            board.call_method1("queue_irq", (&owner, source.irq()))?;
        }
        board.call_method("record", ("X-M1 configured phase-clock edge",), Some(&record))?;
    }
    let earliest = due.iter().try_fold(None, |earliest: Option<u64>, (_, value)| {
        let tick = value.extract::<u64>()?;
        Ok::<_, PyErr>(Some(earliest.map_or(tick, |old| old.min(tick))))
    })?;
    device.setattr(pyo3::intern!(py, "event_due"), earliest)?;
    Ok(())
}
