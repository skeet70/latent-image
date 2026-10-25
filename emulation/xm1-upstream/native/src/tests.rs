use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PyModule};

#[test]
fn breakpoint_changes_preserve_execution_on_pretranslated_loops() {
    Python::initialize();
    Python::attach(|py| {
        let module = PyModule::new(py, "_native").unwrap();
        super::_native(&module).unwrap();
        let library = std::env::var("X70_UNICORN_LIBRARY").expect("Pinned Unicorn test library");
        let machine = module.getattr("Machine").unwrap().call1((
            library, 0x20000, PyBytes::new(py, &[0; 0x4000]), false,
            Vec::<u64>::new(), vec![(0u32, 0u32), (0, 0)],
        )).unwrap();
        let callback = py.import("unittest.mock").unwrap().getattr("Mock").unwrap().call0().unwrap();
        machine.call_method1("set_handler", (&callback,)).unwrap();
        machine.call_method1("set_execution", ("fast",)).unwrap();
        // Synthetic A32: MOV r0,#0; ADD r0,r0,#1; B to the ADD.
        let program: Vec<u8> = [0xe3a00000u32, 0xe2800001, 0xeafffffd]
            .into_iter().flat_map(u32::to_le_bytes).collect();
        for core in 0..2 {
            callback.call_method0("reset_mock").unwrap();
            machine.call_method1("mem_write", (core, 0x10000, PyBytes::new(py, &program))).unwrap();
            machine.call_method1("reg_write", (core, 3, PyBytes::new(py, &0xd3u32.to_le_bytes()))).unwrap();
            machine.call_method1("emu_start", (core, 0x10000, 0x1f000, 0, 7)).unwrap();
            for (breakpoints, count, expected_register, hits) in [
                (vec![0x10004u64], 6, 6u32, 3usize),
                (vec![], 4, 8, 3),
                (vec![0x10008], 4, 10, 5),
                (vec![0x10008], 2, 11, 6),
            ] {
                machine.call_method1("set_breakpoints", (core, breakpoints)).unwrap();
                machine.call_method1("emu_start", (core, 0x10004, 0x1f000, 0, count)).unwrap();
                let register = machine.call_method1("reg_read", (core, 66)).unwrap();
                assert_eq!(register.cast::<PyBytes>().unwrap().as_bytes(), expected_register.to_le_bytes());
                assert_eq!(callback.getattr("call_count").unwrap().extract::<usize>().unwrap(), hits);
            }
            machine.call_method1("set_breakpoints", (core, Vec::<u64>::new())).unwrap();
        }
    });
}

#[test]
fn simultaneous_phase_clocks_preserve_edge_order_and_pending_routing() {
    Python::initialize();
    Python::attach(|py| {
        let namespace = py.import("types").unwrap().getattr("SimpleNamespace").unwrap();
        let mock = py.import("unittest.mock").unwrap().getattr("Mock").unwrap();
        let route = mock.call0().unwrap();
        route.setattr("return_value", py.None()).unwrap();
        let queue = mock.call0().unwrap();
        let record = mock.call0().unwrap();
        let board = namespace.call0().unwrap();
        board.setattr("virtual_ticks", 0).unwrap();
        board.setattr("queue_irq", &queue).unwrap();
        board.setattr("record", &record).unwrap();
        let device = namespace.call0().unwrap();
        let registers = PyDict::new(py);
        for (offset, value) in [(0, 1), (0x38c, 0x203), (0x24, 20)] {
            registers.set_item(offset, value).unwrap();
        }
        device.setattr("registers", &registers).unwrap();
        device.setattr("busy", false).unwrap();
        device.setattr("event_ticks", 10).unwrap();
        device.setattr("phase_clock_due", PyDict::new(py)).unwrap();
        device.setattr("phase_clock_pending", PyDict::new(py)).unwrap();
        device.setattr("event_arms", 0).unwrap();
        device.setattr("event_assertions", 0).unwrap();
        device.setattr("event_enable_generation", 3).unwrap();
        device.setattr("event_history", PyList::empty(py)).unwrap();
        device.setattr("_event_route", &route).unwrap();
        let edge = py.import("dataclasses").unwrap().getattr("make_dataclass").unwrap().call1((
            "PhaseEdge", vec!["cpu", "status_mask", "asserted_tick", "generation"],
        )).unwrap();
        let names = PyDict::new(py);
        names.set_item("PRIMARY", 0).unwrap();
        names.set_item("SECONDARY", 1).unwrap();
        let sources = py.import("enum").unwrap().getattr("IntEnum").unwrap()
            .call1(("PhaseSource", names)).unwrap();
        let module = PyModule::new(py, "_native").unwrap();
        super::_native(&module).unwrap();
        let tick = module.getattr("tick_xm1_clock").unwrap();
        tick.call1((&device, &board, &edge, &sources)).unwrap();
        let due = device.getattr("phase_clock_due").unwrap();
        for key in due.cast::<PyDict>().unwrap().keys().iter() {
            assert!(key.get_type().is(&sources));
        }
        board.setattr("virtual_ticks", 20).unwrap();
        tick.call1((&device, &board, &edge, &sources)).unwrap();
        let history = device.getattr("event_history").unwrap();
        let history = history.cast::<PyList>().unwrap();
        let emitted: Vec<(u8, u64)> = history.iter().map(|event| (
            event.get_item("source").unwrap().extract().unwrap(),
            event.get_item("asserted_tick").unwrap().extract().unwrap(),
        )).collect();
        assert_eq!(emitted, [(0, 20), (1, 20)]);
        assert_eq!(registers.get_item(0x384).unwrap().unwrap().extract::<u32>().unwrap(), 0x203);
        assert_eq!(queue.getattr("call_count").unwrap().extract::<usize>().unwrap(), 0);
        route.setattr("return_value", 0).unwrap();
        board.setattr("virtual_ticks", 21).unwrap();
        tick.call1((&device, &board, &edge, &sources)).unwrap();
        let calls = queue.getattr("call_args_list").unwrap();
        let routed: Vec<(u8, u8)> = calls.cast::<PyList>().unwrap().iter()
            .map(|call| call.getattr("args").unwrap().extract().unwrap()).collect();
        assert_eq!(routed, [(0, 0x27), (0, 0x29)]);
        assert_eq!(device.getattr("event_assertions").unwrap().extract::<u64>().unwrap(), 2);
    });
}

#[test]
fn fixed_scene_frame_pins_every_render_until_cleared() {
    Python::initialize();
    Python::attach(|py| {
        let module = PyModule::new(py, "_native").unwrap();
        super::_native(&module).unwrap();
        let fixed = module.getattr("virtual_scene_fixed_frame").unwrap();
        let yycbcr = module.getattr("virtual_scene_yycbcr").unwrap();
        let histogram = module.getattr("virtual_scene_histogram").unwrap();
        let statistics = module.getattr("virtual_scene_xm1_statistics").unwrap();
        let configuration = PyList::new(py, [0u64; 58]).unwrap();
        let render = |frame: u64| -> Vec<u8> {
            yycbcr.call1((128, 64, py.None(), frame, 256, 192, 0, 0, 256, 192)).unwrap()
                .cast::<PyBytes>().unwrap().as_bytes().to_vec()
        };
        let bins = |frame: u64| -> Vec<u8> {
            histogram.call1((256, 192, 0, 0, 256, 192, frame)).unwrap()
                .cast::<PyBytes>().unwrap().as_bytes().to_vec()
        };
        let measured = |frame: u64| -> Vec<Option<Vec<u8>>> {
            statistics.call1((frame, &configuration, false)).unwrap().extract().unwrap()
        };
        let animated = (render(7), bins(7), measured(7));
        assert_ne!(animated.0, render(100));
        assert_ne!(animated.1, bins(100));
        assert_ne!(animated.2, measured(100));
        assert!(fixed.call1((0,)).is_err());
        assert!(fixed.call1((true,)).is_err());
        assert_eq!(fixed.call1((7,)).unwrap().extract::<Option<u64>>().unwrap(), Some(7));
        for passed in [0, 7, 100, u64::MAX] {
            assert_eq!(render(passed), animated.0);
            assert_eq!(bins(passed), animated.1);
            assert_eq!(measured(passed), animated.2);
        }
        assert_eq!(fixed.call1((py.None(),)).unwrap().extract::<Option<u64>>().unwrap(), None);
        assert_eq!(render(7), animated.0);
        assert_ne!(render(100), animated.0);
    });
}

#[pyclass(unsendable)]
struct SampledKeySession {
    #[pyo3(get)]
    board: Py<PyAny>,
    #[pyo3(get)]
    execution_error: Option<Py<PyAny>>,
    machine: Py<PyAny>,
    changes_at: u64,
    level: u8,
}

#[pymethods]
impl SampledKeySession {
    fn _advance(&self, py: Python<'_>, limit: u64) -> PyResult<()> {
        let board = self.board.bind(py);
        let next = board.getattr("instructions")?.extract::<u64>()? + limit;
        board.setattr("instructions", next)?;
        if next >= self.changes_at {
            self.machine.bind(py).call_method1(
                "mem_write",
                (0, 0x0082_82d9u64, PyBytes::new(py, &[self.level])),
            )?;
        }
        Ok(())
    }
}

#[test]
fn key_release_requires_the_debounced_edge_before_its_cap() {
    Python::initialize();
    Python::attach(|py| {
        let module = PyModule::new(py, "_native").unwrap();
        super::_native(&module).unwrap();
        let library = std::env::var("X70_UNICORN_LIBRARY").expect("Pinned Unicorn test library");
        let machine = module.getattr("Machine").unwrap().call1((
            library, 0x0100_0000, PyBytes::new(py, &[0; 0x4000]), false,
            Vec::<u64>::new(), vec![(0u32, 0u32), (0, 0)],
        )).unwrap();
        let board = py.import("types").unwrap().getattr("SimpleNamespace").unwrap().call0().unwrap();
        board.setattr("instructions", 0u64).unwrap();
        board.setattr("failure", py.None()).unwrap();
        let session = Py::new(py, SampledKeySession {
            board: board.clone().unbind(),
            execution_error: None,
            machine: machine.clone().unbind(),
            changes_at: 400_000,
            level: 0,
        }).unwrap();
        machine.call_method1(
            "mem_write", (0, 0x0082_82d9u64, PyBytes::new(py, &[1])),
        ).unwrap();
        let latched = machine.call_method1("release_menu_key", (&session, "down", 8_000_000u64))
            .unwrap().extract::<Option<bool>>().unwrap();
        assert_eq!(latched, Some(true));
        assert!(machine.call_method0("menu_keys_released").unwrap().extract::<bool>().unwrap());
        let released_at = board.getattr("instructions").unwrap().extract::<u64>().unwrap();
        assert!((400_000..650_000).contains(&released_at));
        assert_eq!(
            machine.call_method1("release_menu_key", (&session, "down", 500_000u64))
                .unwrap().extract::<Option<bool>>().unwrap(),
            Some(false),
        );
        session.borrow_mut(py).changes_at = u64::MAX;
        machine.call_method1(
            "mem_write", (0, 0x0082_82d9u64, PyBytes::new(py, &[1])),
        ).unwrap();
        let before_expired_release = board.getattr("instructions").unwrap().extract::<u64>().unwrap();
        let refused = machine.call_method1("release_menu_key", (&session, "down", 500_000u64))
            .unwrap_err();
        assert!(refused.is_instance_of::<pyo3::exceptions::PyValueError>(py));
        assert_eq!(
            board.getattr("instructions").unwrap().extract::<u64>().unwrap(),
            before_expired_release + 500_000,
        );
    });
}
