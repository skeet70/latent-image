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
