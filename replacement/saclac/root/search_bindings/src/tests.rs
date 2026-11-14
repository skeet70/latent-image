//! Synthetic X-M1 boundary memory, constructed from the documented record and snapshot layouts.
//! No captured firmware, calibration or camera state is included.

use super::{stock, Bindings, RecordBinding, Session, State};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use xm1_kernel::{
    fake::{Call, Fake as KernelFake},
    Form, Kernel, TaskId,
};
use xm1_saclac_calc::adjust::{
    configuration::ControllerFlag,
    scan_drive::{AreaTiming, SampleAreas, SampleTiming},
};
use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, SearchControl};
use xm1_saclac_controllers::search::{
    FocusSearch, LensPosition, SearchDecision, SearchInput, SearchTerminal,
};
use xm1_saclac_focus_search::{
    contrast::settings::StepKind,
    contrast::{
        BridgeDrive, Context, ContrastSearch, Platform, ScanFrame, ScanQuery, StepQuery,
        TravelLimits,
    },
    intake::IntakePorts,
    live::{LaneEvent, LaneStatus, LiveSearch, LiveStrategy, Measurement},
    s1::S1Search,
};
use xm1_saclac_interface::search::{BridgeReport, BridgeStatus, LensClass};
use xm1_saclac_interface::stock::{self as interface, record_handle_from_a2};
use xm1_saclac_interface::{Completed, Completion, RecordHandle};
use xm1_saclac_peers::{EmulatorOnly, LensDriveRefused, PeerPorts};
use xm1_saclac_profile_records::stock::{
    layout::{slot_grant, slot_record, slot_serial, GRANT_READ, GRANT_WRITE},
    record::group_00,
    record_handle,
    shared::{group_15, group_20, group_26, group_27, group_37, group_41},
};
use xm1_saclac_profile_records::{
    AccessGroup, Bytes, Grants, Handle, MeasurementSelection, OpenMode, ProfileRecords,
    Header, Scope, Serial, SharedHeaderByte, Slot, StatisticsInUse, StockEntries, Tag,
};
use xm1_saclac_shared::stock::{Argument, Machine};

#[derive(Clone)]
struct Reply {
    value: u32,
    bytes: Vec<u8>,
}
#[derive(Default)]
struct Boundary {
    memory: BTreeMap<u32, u8>,
    adjustments: BTreeMap<u32, u8>,
    replies: BTreeMap<u32, Reply>,
    calls: Vec<(u32, Vec<u32>)>,
}
#[derive(Clone, Default)]
struct Fake(Rc<RefCell<Boundary>>);
impl Fake {
    fn scalar(&self, address: u32, value: u32) {
        self.0.borrow_mut().replies.insert(
            address,
            Reply {
                value,
                bytes: Vec::new(),
            },
        );
    }
    fn reply(&self, address: u32, bytes: Vec<u8>) {
        self.0
            .borrow_mut()
            .replies
            .insert(address, Reply { value: 0, bytes });
    }
    fn adjustment(&self, key: u32, value: u8) {
        self.0.borrow_mut().adjustments.insert(key, value);
    }
    fn half_adjustment(&self, key: u32, value: i16) {
        for (i, b) in value.to_le_bytes().into_iter().enumerate() {
            self.adjustment(key + i as u32, b);
        }
    }
    fn memory(&self, address: u32, bytes: &[u8]) {
        for (i, &byte) in bytes.iter().enumerate() {
            self.0.borrow_mut().memory.insert(address + i as u32, byte);
        }
    }
    fn execute(&self, address: u32, args: &[u32], buffer: &mut [u8]) -> u32 {
        let mut b = self.0.borrow_mut();
        if address == 0x69f8fc {
            return u32::from(b.adjustments.get(&args[0]).copied().unwrap_or(0));
        }
        b.calls.push((address, args.to_vec()));
        if address == 0x772f28 {
            return args[0] * 10;
        }
        if address == 0x749254 {
            return args[2];
        }
        let reply = b
            .replies
            .get(&address)
            .unwrap_or_else(|| panic!("unconfigured stock entry {address:#x} with {args:?}"))
            .clone();
        if !reply.bytes.is_empty() {
            buffer[..reply.bytes.len()].copy_from_slice(&reply.bytes);
        }
        reply.value
    }
    fn calls(&self) -> Vec<(u32, Vec<u32>)> {
        self.0.borrow().calls.clone()
    }
}
impl Machine for Fake {
    fn read_u8(&self, address: u32) -> u8 { self.0.borrow().memory.get(&address).copied().unwrap_or(0) }

    fn read_u16(&self, address: u32) -> u16 {
        let b = self.0.borrow();
        u16::from_le_bytes([
            b.memory.get(&address).copied().unwrap_or(0),
            b.memory.get(&(address + 1)).copied().unwrap_or(0),
        ])
    }
    fn read_u32(&self, address: u32) -> u32 {
        u32::from(self.read_u16(address)) | u32::from(self.read_u16(address + 2)) << 16
    }
    fn write_u8(&mut self, address: u32, value: u8) {
        self.memory(address, &[value]);
    }
    fn write_u32(&mut self, address: u32, value: u32) {
        self.memory(address, &value.to_le_bytes());
    }
    fn enter(&self, address: u32, args: &[usize]) -> u32 {
        self.execute(
            address,
            &args.iter().map(|&v| v as u32).collect::<Vec<_>>(),
            &mut [],
        )
    }
    fn enter_with(&self, address: u32, args: &[Argument], bytes: &mut [u8]) -> u32 {
        let words = args
            .iter()
            .map(|arg| match *arg {
                Argument::Word(v) => v,
                Argument::Buffer(offset) => 0xa0000000 + offset as u32,
            })
            .collect::<Vec<_>>();
        self.execute(address, &words, bytes)
    }
}
struct Entries {
    mode: u8,
    selection: MeasurementSelection,
}
impl StockEntries for Entries {
    fn measurement_selection(&mut self, _: RecordHandle) -> MeasurementSelection {
        self.selection
    }
    fn header_byte(&mut self, _: RecordHandle, byte: SharedHeaderByte) -> u8 {
        match byte {
            SharedHeaderByte::At31 => 6,
            SharedHeaderByte::At47 => self.mode,
            _ => 0,
        }
    }
    fn lens_designation(&mut self, _: RecordHandle) -> [u8; 28] {
        [0; 28]
    }
    fn replace_header(&mut self, _: RecordHandle, _: Header<'_>) {}
    fn open(&mut self, _: OpenMode, _: Grants) -> RecordHandle {
        record_handle_from_a2(u32::MAX)
    }
    fn close(&mut self, _: RecordHandle) {}
    fn initialize(&mut self) {}
}
struct Fixture {
    machine: Fake,
    records: ProfileRecords<Bytes>,
    entries: Entries,
    state: State,
    record: RecordHandle,
}
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
impl Fixture {
    fn new() -> Self {
        let mut bytes = Bytes::default();
        let slot = Slot::new(0).unwrap();
        bytes.registry[usize::from(slot_serial(slot))..][..4].copy_from_slice(&1u32.to_le_bytes());
        bytes.registry[usize::from(slot_record(slot))..][..4].copy_from_slice(&0u32.to_le_bytes());
        for group in 0..43 {
            bytes.registry[usize::from(slot_grant(slot, AccessGroup::new(group).unwrap()))] =
                GRANT_READ | GRANT_WRITE;
        }
        let record = record_handle(Handle {
            slot,
            serial: Serial::new(1).unwrap(),
            owner: true,
            writable: true,
            scope: Scope::ALL,
            tag: Tag(0),
        });
        let mut records = ProfileRecords::new(bytes);
        records
            .write(record, &group_00::AT_0000, 0x00030002)
            .unwrap();
        records
            .write_shared(record, &group_26::AT_0206, 500)
            .unwrap();
        records
            .write_shared(record, &group_26::AT_0208, 100)
            .unwrap();
        records.write_shared(record, &group_26::AT_0200, 3).unwrap();
        records
            .write_shared(record, &group_26::AT_0230, 11)
            .unwrap();
        records
            .write_shared(record, &group_41::AT_0970, 50)
            .unwrap();
        records
            .write_shared(record, &group_27::AT_0244, 500)
            .unwrap();
        records
            .write_shared(record, &group_27::AT_0248, 100)
            .unwrap();
        records
            .write_shared(record, &group_37::AT_085C, [8, 0, 0, 0, 0, 0, 0, 0])
            .unwrap();
        records
            .write_shared(
                record,
                &group_20::STATISTICS_IN_USE,
                StatisticsInUse {
                    primary: true,
                    search: false,
                    auxiliary: true,
                },
            )
            .unwrap();
        let machine = Fake::default();
        for (address, value) in [
            (0x76da84, (-30i16) as i32 as u32),
            (0x76d9c8, 0),
            (0x76f340, 0),
            (0x76dbb4, 0),
            (0x746054, 0),
            (0x76f360, 0),
            (0x76da38, 0),
            (0x76b8b4, 1),
            (0x76d9a8, 0),
            (0x76d9ec, 1),
            (0x748818, 100),
            (0x74888c, 100),
            (0x748398, 0),
            (0x74947c, 0),
            (0x7462e8, 0x8000),
            (0x1007e44, 10000),
            (0x1004ce0, 0),
            (0xee9ab0, 10000),
            (0xee99f0, 0),
            (0xee9928, 0),
            (0xf77ef8, 3),
            (0xf784f8, 10),
            (0xf78d24, 30),
            (0xf46c20, 0),
            (0xf47468, 0),
            (0xfeaacc, 0),
            (0x6b7830, 0x80008000),
            (0x6b7840, 0),
            (0x1110918, 0),
            (0x64744c, 0),
            (0xef74e4, 7),
        ] {
            machine.scalar(address, value);
        }
        for (address, values) in [
            (0x1005f60, vec![10000]),
            (0x1004d04, vec![0, 0, 0]),
            (0x1004d68, vec![10]),
            (0xf5206c, vec![7, 9]),
            (0xfea7bc, vec![6]),
            (0xf77c8c, vec![11]),
            (0x7257a8, vec![123]),
        ] {
            machine.reply(address, words(&values));
        }
        for (address, range) in [(0x74bd64, [-200i16, 90]), (0x74b05c, [-150i16, 60])] {
            machine.reply(
                address,
                range.into_iter().flat_map(|v| v.to_le_bytes()).collect(),
            );
        }
        machine.reply(0x7498e0, vec![0, 0]);
        machine.reply(0x747584, vec![0, 0]);
        machine.scalar(0x747aac, 0);
        machine.scalar(0x746120, 0);
        machine.scalar(0x7461a8, 0);
        machine.scalar(0x747408, 0);
        machine.scalar(0x749be4, 0);
        machine.scalar(0xee97f0, 100);
        let mut snapshot = vec![0; xm1_saclac_focus_search::stock::intake::FRAME_SNAPSHOT_BYTES];
        for (at, value) in [
            (0, 5),
            (4, 1),
            (8, 2),
            (12, 640),
            (16, 480),
            (0x34, 3),
            (0x38, 4),
            (0xdc, 5),
            (0xe0, 6),
            (0xec, 17),
            (0xf4, 3),
            (0xf8, 3),
        ] {
            stock::put_word(&mut snapshot, at, value);
        }
        stock::put_half(&mut snapshot, 0xfc, 50);
        stock::put_half(&mut snapshot, 0xfe, 50);
        machine.reply(0xfea7ac, snapshot);
        let mut s1 = vec![0; 96];
        for (at, value) in [
            (0, 640),
            (4, 480),
            (0x28, 2),
            (0x2c, 1),
            (0x30, 2),
            (0x34, 5),
            (0x38, 6),
            (0x3c, 3),
            (0x40, 4),
        ] {
            stock::put_word(&mut s1, at, value);
        }
        stock::put_half(&mut s1, 0x4c, 7);
        machine.reply(0xfeaadc, s1);
        machine.adjustment(0x3e0f82, 10);
        machine.adjustment(0x3e0f80, 3);
        machine.adjustment(0x887c, 1);
        machine.adjustment(0x87fd, 1);
        machine.adjustment(0x87df, 5);
        machine.adjustment(0x87eb, 6);
        machine.adjustment(0x8883, 7);
        machine.adjustment(0x8889, 30);
        machine.adjustment(0x880f, 8);
        machine.adjustment(0x87f3, 50);
        for key in [0x883f, 0x883d, 0x8848] {
            machine.adjustment(key, 10);
        }
        Self {
            machine,
            records,
            entries: Entries {
                mode: 4,
                selection: MeasurementSelection::Unselected,
            },
            state: State::default(),
            record,
        }
    }
}
static SAFETY_MARKER: u32 = 1;
fn kernel() -> Kernel<KernelFake> {
    Kernel::new(KernelFake::with_task(TaskId::new(43).unwrap()))
}
fn posts(kernel: &Kernel<KernelFake>) -> Vec<(u8, u16, Vec<u8>)> {
    kernel
        .backend()
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Post {
                id,
                form,
                header,
                payload,
            } => {
                assert_eq!(form, Form::Task);
                assert_eq!((header.sender, header.destination), (43, id.get()));
                Some((id.get(), header.operation, payload))
            }
            _ => None,
        })
        .collect()
}
fn report(status: BridgeStatus) -> BridgeReport {
    BridgeReport {
        status,
        position: -30,
        class: LensClass(2),
    }
}

#[test]
fn live_stop_drains_calc_then_releases_and_retries_a_deferred_release() {
    let mut f = Fixture::new();
    let k = kernel();
    let cx = k.task().unwrap();
    let mut peers = PeerPorts::new(&cx, EmulatorOnly::new(&SAFETY_MARKER).unwrap());
    let record = RecordBinding::new(f.record);
    let bindings = RefCell::new(Bindings::new(
        &mut f.machine,
        &mut f.records,
        &mut f.entries,
        &mut peers,
        &mut f.state,
    ));
    let mut ports = Session::new(&bindings, &record);
    let mut search = LiveSearch::new(ContrastSearch::new(ports), Context::new());
    assert_eq!(
        search.handle(LaneEvent::Initialize, &mut ports),
        LaneStatus::Complete
    );
    assert_eq!(
        search.handle(LaneEvent::Enable, &mut ports),
        LaneStatus::Complete
    );
    assert_eq!(
        bindings
            .borrow()
            .records
            .read_shared(record.0.get(), &group_20::STATISTICS_IN_USE)
            .unwrap(),
        StatisticsInUse {
            primary: true,
            search: true,
            auxiliary: true
        }
    );
    let status = bindings
        .borrow()
        .records
        .read_shared(record.0.get(), &group_15::FOCUS_CONFIRMATION)
        .unwrap();
    assert_eq!(stock::word(&status, 0), 0);
    assert_eq!(stock::word(&status, 4), 4);
    assert_eq!(stock::word(&status, 20), 2);
    assert_eq!(
        search.handle(LaneEvent::Work, &mut ports),
        LaneStatus::Pending
    );
    let sent = posts(&k);
    assert_eq!((sent[0].0, sent[0].1), (44, 0x3c09));
    assert_eq!(stock::word(&sent[0].2, 0), 0xf3c104);
    let request = &sent[0].2[4..];
    assert_eq!(
        (
            stock::word(request, 0x10),
            stock::word(request, 0x14),
            stock::word(request, 0x18),
            stock::word(request, 0x1c)
        ),
        (0, 4, 7, 9)
    );
    assert_eq!(
        (
            stock::half(request, 0x20) as i16,
            stock::half(request, 0x22) as i16
        ),
        (500, 100)
    );
    assert_eq!(stock::half(request, 0x6e), 7);
    assert_eq!(
        search.handle(LaneEvent::Stop, &mut ports),
        LaneStatus::Pending
    );
    assert_eq!(posts(&k).len(), 1);
    let payload = words(&[0, 0x90000000]);
    let event = ports
        .live_completion(Completed {
            completion: Completion::Calculation89,
            parameters: interface::parameters(&payload),
        })
        .unwrap();
    assert_eq!(search.handle(event, &mut ports), LaneStatus::Pending);
    let sent = posts(&k);
    assert_eq!((sent[1].0, sent[1].1), (47, 0x3f2e));
    assert_eq!(stock::word(&sent[1].2, 0), 0xf3c14c);
    assert_eq!(stock::half(&sent[1].2, 16), 0);
    assert_eq!(stock::word(&sent[1].2, 24), 0);
    assert_eq!(stock::word(&sent[1].2, 28), 2);
    assert_eq!(
        search.handle(
            LaneEvent::LensReported(report(BridgeStatus::Deferred)),
            &mut ports
        ),
        LaneStatus::Pending
    );
    assert_eq!(posts(&k)[2], sent[1]);
    assert_eq!(
        search.handle(
            LaneEvent::LensReported(report(BridgeStatus::Moved)),
            &mut ports
        ),
        LaneStatus::Complete
    );
    assert!(!bindings
        .borrow()
        .machine
        .calls()
        .iter()
        .any(|c| c.0 == 0xf46c20));
}

fn control<
    M: Machine,
    R: xm1_saclac_profile_records::Memory,
    E: StockEntries,
    P: xm1_saclac_interface::Calc + xm1_saclac_interface::ImgDc,
>(
    search: &mut S1Search,
    ports: &mut Session<'_, '_, M, R, E, P>,
    control: SearchControl,
    record: RecordHandle,
) -> ChildOutcome {
    search.handle(
        ChildEvent::Control {
            control,
            parameters: interface::parameters(&[]),
            record,
        },
        ports,
    )
}
#[test]
fn s1_frames_are_evaluated_once_and_cancel_drains_before_release() {
    let mut f = Fixture::new();
    let observer = f.machine.clone();
    let k = kernel();
    let cx = k.task().unwrap();
    let mut peers = PeerPorts::new(&cx, EmulatorOnly::new(&SAFETY_MARKER).unwrap());
    let record = RecordBinding::new(f.record);
    let bindings = RefCell::new(Bindings::new(
        &mut f.machine,
        &mut f.records,
        &mut f.entries,
        &mut peers,
        &mut f.state,
    ));
    let mut ports = Session::new(&bindings, &record);
    let mut search = S1Search::new();
    assert_eq!(
        control(
            &mut search,
            &mut ports,
            SearchControl::Initialize,
            record.0.get()
        ),
        ChildOutcome::Complete
    );
    assert_eq!(
        control(
            &mut search,
            &mut ports,
            SearchControl::Start,
            record.0.get()
        ),
        ChildOutcome::Pending
    );
    control(
        &mut search,
        &mut ports,
        SearchControl::Start,
        record.0.get(),
    );
    let sent = posts(&k);
    assert_eq!(sent.len(), 1);
    assert_eq!((sent[0].0, sent[0].1), (44, 0x3c09));
    assert_eq!(stock::word(&sent[0].2, 0), 0xf25758);
    let req = &sent[0].2[4..];
    assert_eq!((stock::half(req, 4), stock::half(req, 6)), (2, 3));
    assert_eq!(
        (stock::half(req, 0x20) as i16, stock::half(req, 0x22) as i16),
        (50, 430)
    );
    assert_eq!((stock::word(req, 0x30), stock::word(req, 0x34)), (1, 2));
    assert_eq!(stock::half(req, 0x6e), 7);
    assert_eq!(
        control(
            &mut search,
            &mut ports,
            SearchControl::Cancel,
            record.0.get()
        ),
        ChildOutcome::Pending
    );
    let address = 0x90000000;
    let result = words(&[0, 99, 17, 9]);
    observer.memory(address, &result);
    let payload = words(&[0, address]);
    assert_eq!(
        search.handle(
            ChildEvent::Completed {
                completed: Completed {
                    completion: Completion::Calculation9d,
                    parameters: interface::parameters(&payload)
                },
                record: record.0.get()
            },
            &mut ports
        ),
        ChildOutcome::Pending
    );
    let sent = posts(&k);
    assert_eq!((sent[1].0, sent[1].1), (47, 0x3f44));
    assert_eq!(sent[1].2, words(&[0xf25728, 0, 0, 0, 2, 0]));
    let reported = words(&[0, (-30i16) as u16 as u32, 2]);
    assert_eq!(
        search.handle(
            ChildEvent::Completed {
                completed: Completed {
                    completion: Completion::Image5a,
                    parameters: interface::parameters(&reported)
                },
                record: record.0.get()
            },
            &mut ports
        ),
        ChildOutcome::Complete
    );
    assert_eq!(observer.read_u32(address), 0);
    assert_eq!(
        control(
            &mut search,
            &mut ports,
            SearchControl::Resume,
            record.0.get()
        ),
        ChildOutcome::Complete
    );
    assert!(!observer.calls().iter().any(|c| c.0 == 0xf46c20));
}

#[test]
fn scan_adapter_keeps_step_size_not_frame_travel_as_the_selected_limit_lead() {
    let mut f = Fixture::new();
    let observer = f.machine.clone();
    observer.adjustment(0x87fd, 0);
    observer.half_adjustment(0x3e15a8, 10000);
    for channel in 0..3 {
        observer.half_adjustment(0x3e0a6b + 2 * channel, -200);
        observer.half_adjustment(0x3e09a2 + 2 * channel, 90);
    }
    let mut peers = ();
    let record = RecordBinding::new(f.record);
    let bindings = RefCell::new(Bindings::new(
        &mut f.machine,
        &mut f.records,
        &mut f.entries,
        &mut peers,
        &mut f.state,
    ));
    let mut ports = Session::new(&bindings, &record);
    let step = Platform::step(
        &mut ports,
        StepQuery {
            kind: StepKind::Direction,
            parameter: 3,
            contrast: 100,
            profile_class: 6,
            lens_mode: 0,
        },
    );
    assert_eq!((step.size, step.rate), (50, 5000));
    observer.half_adjustment(0x3e1716, 1000);
    observer.adjustment(0x9276, 100);
    Platform::request_scan_refresh(&mut ports, true);
    assert!(bindings.borrow().state.configuration.refresh());
    bindings.borrow_mut().state.set_sample_timing(SampleTiming {
        level: 0,
        source: 4,
        areas: SampleAreas::Primary(AreaTiming {
            exposure: 2,
            start: 0,
            gap: 0,
            rows: 7,
            count: 7,
        }),
    });
    let query = ScanQuery {
        watching: false,
        contrast: 100,
        lens_mode: 0,
        direction: 1,
        rate: 999,
        limits: TravelLimits {
            low: LensPosition(-200),
            high: LensPosition(90),
            inner_low: LensPosition(-150),
            inner_high: LensPosition(60),
            offset: 0,
        },
        frame: ScanFrame {
            parameter_index: 3,
            stage_halves: [0, 71],
            current_lens: 2,
            stage_last: 2,
        },
        moved: None,
        mark: 0,
    };
    let plan = Platform::plan_scan(&mut ports, query);
    assert_eq!(plan.window, [(-30i16) as u16, 90]);
    assert_eq!((plan.lead, plan.rate), (60, 1000));
    assert_eq!(plan.limits, [LensPosition(-200), LensPosition(90)]);
    let corrected = Platform::corrected_position(&mut ports, 1);
    assert_eq!(corrected.drive, LensPosition(-20));
    assert_eq!(corrected.frame, LensPosition(-25));
    assert!(observer
        .calls()
        .iter()
        .any(|(address, args)| *address == 0x1007e44 && args == &[71, 2]));
    observer.adjustment(0x87fd, 1);
    Platform::request_scan_refresh(&mut ports, true);
    assert!(!Platform::scan_refresh(&mut ports));
    bindings
        .borrow_mut()
        .state
        .configuration
        .set_controller_flag(ControllerFlag::First, true);
    assert!(bindings
        .borrow()
        .state
        .configuration
        .controller_flag(ControllerFlag::First));
}

#[test]
fn peer_lens_safety_refusal_faults_without_posting_a_drive() {
    let mut f = Fixture::new();
    let k = kernel();
    let cx = k.task().unwrap();
    let mut peers = PeerPorts::new(&cx, LensDriveRefused);
    let record = RecordBinding::new(f.record);
    let bindings = RefCell::new(Bindings::new(
        &mut f.machine,
        &mut f.records,
        &mut f.entries,
        &mut peers,
        &mut f.state,
    ));
    let mut ports = Session::new(&bindings, &record);
    let mut search = S1Search::new();
    control(
        &mut search,
        &mut ports,
        SearchControl::Initialize,
        record.0.get(),
    );
    control(
        &mut search,
        &mut ports,
        SearchControl::Start,
        record.0.get(),
    );
    control(
        &mut search,
        &mut ports,
        SearchControl::Cancel,
        record.0.get(),
    );
    let payload = words(&[0, 0x90000000]);
    search.handle(
        ChildEvent::Completed {
            completed: Completed {
                completion: Completion::Calculation9d,
                parameters: interface::parameters(&payload),
            },
            record: record.0.get(),
        },
        &mut ports,
    );
    assert_eq!(
        posts(&k)
            .iter()
            .map(|(_, operation, _)| *operation)
            .collect::<Vec<_>>(),
        [0x3c09]
    );
    assert_eq!(
        bindings
            .borrow()
            .machine
            .calls()
            .iter()
            .filter(|c| c.0 == 0xf46c20)
            .count(),
        1
    );
}

#[test]
fn range_outputs_and_pinned_updates_preserve_unrelated_record_bytes() {
    let mut f = Fixture::new();
    let mut peers = ();
    let record = RecordBinding::new(f.record);
    let bindings = RefCell::new(Bindings::new(
        &mut f.machine,
        &mut f.records,
        &mut f.entries,
        &mut peers,
        &mut f.state,
    ));
    let mut ports = Session::new(&bindings, &record);
    let limits = xm1_saclac_focus_search::contrast::travel_limits(&mut ports, false).unwrap();
    assert_eq!(
        (
            limits.low.0,
            limits.high.0,
            limits.inner_low.0,
            limits.inner_high.0
        ),
        (-200, 90, -150, 60)
    );
    let mut bytes = [0x5a; 76];
    bindings
        .borrow_mut()
        .records
        .write_shared(
            record.0.get(),
            &xm1_saclac_profile_records::stock::shared::group_33::AT_07A0,
            bytes,
        )
        .unwrap();
    Platform::publish_pinned_status(
        &mut ports,
        xm1_saclac_focus_search::contrast::PinnedStatus {
            level: 123,
            mode: 4,
            position: LensPosition(-12),
            reported: LensPosition(37),
        },
    );
    stock::put_word(&mut bytes, 64, 123);
    stock::put_word(&mut bytes, 68, 4);
    stock::put_half(&mut bytes, 72, (-12i16) as u16);
    assert_eq!(
        bindings
            .borrow()
            .records
            .read_shared(
                record.0.get(),
                &xm1_saclac_profile_records::stock::shared::group_33::AT_07A0
            )
            .unwrap(),
        bytes
    );
    assert_eq!(bindings.borrow().machine.read_u16(0x16cb404), 37);
}

#[test]
fn focus_log_wrap_keeps_phase_words_that_a_return_does_not_replace() {
    use xm1_saclac_focus_search::contrast::{FocusLog, FocusLogEntry, LoggedSample};
    let mut f = Fixture::new();
    let observer = f.machine.clone();
    observer.memory(0xf49358, &0xb0000000u32.to_le_bytes());
    observer.memory(0x16c2228, &300u32.to_le_bytes());
    observer.memory(0xb0000000 + 300 * 108 + 60, &words(&[11, 22, 33, 44, 55]));
    let mut peers = ();
    let record = RecordBinding::new(f.record);
    let bindings = RefCell::new(Bindings::new(
        &mut f.machine,
        &mut f.records,
        &mut f.entries,
        &mut peers,
        &mut f.state,
    ));
    let mut ports = Session::new(&bindings, &record);
    let log = FocusLog {
        entry: FocusLogEntry {
            phase: 16,
            combined: 123,
            phase_words: None,
            ..FocusLogEntry::default()
        },
        scan: [LoggedSample {
            focus_value: 7,
            contrast: 10,
            position: -5,
        }; 7],
        extent: [LoggedSample::default(); 13],
    };
    Platform::append_focus_log(&mut ports, &log);
    assert_eq!(observer.read_u32(0x16c2228), 0);
    let newest = Platform::newest_focus_log(&mut ports);
    assert_eq!(
        (newest.phase, newest.combined, newest.phase_words),
        (16, 123, Some([11, 22, 33, 44, 55]))
    );
    assert_eq!(
        (
            observer.read_u16(0xb0000000 + 0x7efc),
            observer.read_u32(0xb0000000 + 0x7f0c),
            observer.read_u16(0xb0000000 + 0x7f28) as i16
        ),
        (7, 10, -5)
    );
    let next = FocusLog {
        entry: FocusLogEntry {
            phase: 13,
            phase_words: Some([1, 2, 3, 4, 5]),
            ..FocusLogEntry::default()
        },
        ..log
    };
    Platform::append_focus_log(&mut ports, &next);
    assert_eq!(observer.read_u32(0x16c2228), 1);
    assert_eq!(
        Platform::newest_focus_log(&mut ports).phase_words,
        Some([1, 2, 3, 4, 5])
    );
}

#[test]
fn live_and_s1_interleave_over_one_service_borrow_and_drain_independently() {
    let mut f = Fixture::new();
    let k = kernel();
    let cx = k.task().unwrap();
    let mut peers = PeerPorts::new(&cx, EmulatorOnly::new(&SAFETY_MARKER).unwrap());
    let live_record = RecordBinding::new(f.record);
    let s1_record = RecordBinding::new(f.record);
    let bindings = RefCell::new(Bindings::new(
        &mut f.machine,
        &mut f.records,
        &mut f.entries,
        &mut peers,
        &mut f.state,
    ));
    let mut live_ports = Session::new(&bindings, &live_record);
    let mut s1_ports = Session::new(&bindings, &s1_record);
    let mut live = LiveSearch::new(ContrastSearch::new(live_ports), Context::new());
    let mut s1 = S1Search::new();
    assert_eq!(
        live.handle(LaneEvent::Initialize, &mut live_ports),
        LaneStatus::Complete
    );
    assert_eq!(
        live.handle(LaneEvent::Enable, &mut live_ports),
        LaneStatus::Complete
    );
    assert_eq!(
        control(
            &mut s1,
            &mut s1_ports,
            SearchControl::Initialize,
            s1_record.0.get()
        ),
        ChildOutcome::Complete
    );
    assert_eq!(
        control(
            &mut s1,
            &mut s1_ports,
            SearchControl::Start,
            s1_record.0.get()
        ),
        ChildOutcome::Pending
    );
    assert_eq!(
        live.handle(LaneEvent::Work, &mut live_ports),
        LaneStatus::Pending
    );
    assert_eq!(
        live.handle(LaneEvent::Stop, &mut live_ports),
        LaneStatus::Pending
    );
    assert_eq!(
        control(
            &mut s1,
            &mut s1_ports,
            SearchControl::Cancel,
            s1_record.0.get()
        ),
        ChildOutcome::Pending
    );
    let evaluated = words(&[0, 0x90000000]);
    let live_event = live_ports
        .live_completion(Completed {
            completion: Completion::Calculation89,
            parameters: interface::parameters(&evaluated),
        })
        .unwrap();
    assert_eq!(
        live.handle(live_event, &mut live_ports),
        LaneStatus::Pending
    );
    assert_eq!(
        s1.handle(
            ChildEvent::Completed {
                completed: Completed {
                    completion: Completion::Calculation9d,
                    parameters: interface::parameters(&evaluated),
                },
                record: s1_record.0.get(),
            },
            &mut s1_ports
        ),
        ChildOutcome::Pending
    );
    let traffic: Vec<_> = posts(&k)
        .into_iter()
        .map(|(queue, operation, bytes)| (queue, operation, stock::word(&bytes, 0)))
        .collect();
    assert_eq!(
        traffic,
        [
            (44, 0x3c09, 0xf25758),
            (44, 0x3c09, 0xf3c104),
            (47, 0x3f2e, 0xf3c14c),
            (47, 0x3f44, 0xf25728),
        ]
    );
    assert_eq!(
        live.handle(
            LaneEvent::LensReported(report(BridgeStatus::Moved)),
            &mut live_ports
        ),
        LaneStatus::Complete
    );
    let reported = words(&[0, (-30i16) as u16 as u32, 2]);
    assert_eq!(
        s1.handle(
            ChildEvent::Completed {
                completed: Completed {
                    completion: Completion::Image5a,
                    parameters: interface::parameters(&reported),
                },
                record: s1_record.0.get(),
            },
            &mut s1_ports
        ),
        ChildOutcome::Complete
    );
    assert!(!bindings
        .borrow()
        .machine
        .calls()
        .iter()
        .any(|call| call.0 == 0xf46c20));
    println!("shared-service smoke: traffic={traffic:x?}; live=Complete; S1=Complete; faults=0");
}

#[test]
fn live_evaluation_publishes_lens_row_before_step_in_the_shot_status() {
    let mut f = Fixture::new();
    let k = kernel();
    let cx = k.task().unwrap();
    let mut peers = PeerPorts::new(&cx, EmulatorOnly::new(&SAFETY_MARKER).unwrap());
    let record = RecordBinding::new(f.record);
    let bindings = RefCell::new(Bindings::new(
        &mut f.machine,
        &mut f.records,
        &mut f.entries,
        &mut peers,
        &mut f.state,
    ));
    let mut ports = Session::new(&bindings, &record);
    let mut live = LiveSearch::new(ContrastSearch::new(ports), Context::new());
    live.handle(LaneEvent::Initialize, &mut ports);
    live.handle(LaneEvent::Enable, &mut ports);
    live.handle(LaneEvent::Work, &mut ports);
    let evaluation = xm1_saclac_interface::search::Evaluation {
        pairs: [[100, 20]; 3],
        settled: 1,
        ..xm1_saclac_interface::search::Evaluation::default()
    };
    assert_eq!(
        live.handle(LaneEvent::Evaluated(evaluation), &mut ports),
        LaneStatus::Idle
    );
    let status = bindings
        .borrow()
        .records
        .read_shared(record.0.get(), &group_15::FOCUS_CONFIRMATION)
        .unwrap();
    assert_eq!(stock::word(&status, 0), 1);
    assert_eq!((stock::word(&status, 12), stock::word(&status, 16)), (0, 3));
}

struct TerminalCandidate<P> {
    stock: ContrastSearch<P>,
    outcome: SearchTerminal,
}

struct TerminalContext {
    stock: Context,
    notifications: Vec<SearchTerminal>,
    samples: usize,
    resumes: usize,
}

impl<P: Platform + IntakePorts> FocusSearch for TerminalCandidate<P> {
    type Context = TerminalContext;
    type Sample = Measurement<xm1_saclac_interface::search::Evaluation>;
    type Drive = BridgeDrive;

    fn advance(
        &mut self,
        context: &mut TerminalContext,
        input: SearchInput<Self::Sample>,
    ) -> SearchDecision<BridgeDrive> {
        match input {
            SearchInput::Sample(_) => {
                context.samples += 1;
                SearchDecision::Terminal(self.outcome)
            }
            SearchInput::Cancel => SearchDecision::Terminal(self.outcome),
            SearchInput::Resume => {
                context.resumes += 1;
                self.stock.advance(&mut context.stock, SearchInput::Resume)
            }
            SearchInput::Initialize
            | SearchInput::Begin
            | SearchInput::Suspend
            | SearchInput::ResumePending => self.stock.advance(&mut context.stock, input),
        }
    }
}

impl<P: Platform + IntakePorts> LiveStrategy for TerminalCandidate<P> {
    type Evaluation = xm1_saclac_interface::search::Evaluation;
    type Record = xm1_saclac_interface::search::SearchRecord;

    fn ready(&mut self, context: &TerminalContext, bridge_busy: bool) -> bool {
        self.stock.ready(&context.stock, bridge_busy)
    }
    fn take_in(&mut self, context: &mut TerminalContext) -> Self::Record {
        self.stock.take_in(&mut context.stock)
    }
    fn lens_reported(&mut self, context: &mut TerminalContext, report: BridgeReport) {
        self.stock.lens_reported(&mut context.stock, report);
    }
    fn release(&mut self, context: &TerminalContext) -> BridgeDrive {
        self.stock.release(&context.stock)
    }
    fn accepts_begin(&self, context: &TerminalContext) -> bool {
        self.stock.accepts_begin(&context.stock)
    }
    fn finished(&mut self, context: &mut TerminalContext, outcome: SearchTerminal) {
        context.notifications.push(outcome);
    }
}

#[test]
fn binding_preserves_terminal_outcomes_quiescence_resume_and_stop_drain() {
    for outcome in [
        SearchTerminal::Focused(LensPosition(-37)),
        SearchTerminal::Failed,
        SearchTerminal::Cancelled,
    ] {
        let mut f = Fixture::new();
        let observer = f.machine.clone();
        let k = kernel();
        let cx = k.task().unwrap();
        let mut peers = PeerPorts::new(&cx, EmulatorOnly::new(&SAFETY_MARKER).unwrap());
        let record = RecordBinding::new(f.record);
        let bindings = RefCell::new(Bindings::new(
            &mut f.machine,
            &mut f.records,
            &mut f.entries,
            &mut peers,
            &mut f.state,
        ));
        let mut ports = Session::new(&bindings, &record);
        let candidate = TerminalCandidate {
            stock: ContrastSearch::new(ports),
            outcome,
        };
        let context = TerminalContext {
            stock: Context::new(),
            notifications: Vec::new(),
            samples: 0,
            resumes: 0,
        };
        let mut lane = LiveSearch::new(candidate, context);
        assert_eq!(
            ports.handle_live(&mut lane, LaneEvent::Initialize),
            LaneStatus::Complete
        );
        assert_eq!(
            ports.handle_live(&mut lane, LaneEvent::Enable),
            LaneStatus::Complete
        );
        assert_eq!(
            ports.handle_live(&mut lane, LaneEvent::Work),
            LaneStatus::Pending
        );
        let evaluated = words(&[0, 0x90000000]);
        let event = ports
            .live_completion(Completed {
                completion: Completion::Calculation89,
                parameters: interface::parameters(&evaluated),
            })
            .unwrap();
        assert_eq!(
            ports.handle_live(&mut lane, event),
            LaneStatus::Terminal(outcome)
        );
        assert_eq!(lane.context().notifications, [outcome]);
        assert_eq!(lane.context().samples, 1);
        let calls = observer.calls();
        let requests = posts(&k);
        assert_eq!((requests[0].0, requests[0].1), (44, 0x3c09));
        for _ in 0..2 {
            assert_eq!(
                ports.handle_live(&mut lane, LaneEvent::Work),
                LaneStatus::Terminal(outcome)
            );
            assert_eq!(
                ports.handle_live(&mut lane, LaneEvent::Suspend),
                LaneStatus::Complete
            );
        }
        assert_eq!(observer.calls(), calls);
        assert_eq!(posts(&k), requests);
        assert_eq!(lane.context().notifications, [outcome]);
        assert_eq!(
            ports.handle_live(&mut lane, LaneEvent::Resume),
            LaneStatus::Complete
        );
        assert_eq!(lane.context().resumes, 1);
        assert_eq!(
            ports.handle_live(&mut lane, LaneEvent::Work),
            LaneStatus::Pending
        );
        assert_eq!(
            ports.handle_live(&mut lane, LaneEvent::Stop),
            LaneStatus::Terminal(outcome)
        );
        assert_eq!(lane.context().notifications, [outcome, outcome]);
        assert_eq!(
            ports.handle_live(&mut lane, LaneEvent::Work),
            LaneStatus::Idle
        );
        assert_eq!(
            posts(&k).iter().map(|p| (p.0, p.1)).collect::<Vec<_>>(),
            [(44, 0x3c09), (44, 0x3c09)]
        );
        let event = ports
            .live_completion(Completed {
                completion: Completion::Calculation89,
                parameters: interface::parameters(&evaluated),
            })
            .unwrap();
        assert_eq!(ports.handle_live(&mut lane, event), LaneStatus::Pending);
        assert_eq!(lane.context().samples, 1);
        let release = &posts(&k)[2];
        assert_eq!((release.0, release.1), (47, 0x3f2e));
        assert_eq!(stock::word(&release.2, 0), 0xf3c14c);
        assert_eq!(stock::half(&release.2, 16), 0);
        assert_eq!(stock::word(&release.2, 24), 0);
        let moved = words(&[0, u32::from((-30i16) as u16), 2]);
        let event = ports
            .live_completion(Completed {
                completion: Completion::Image46,
                parameters: interface::parameters(&moved),
            })
            .unwrap();
        assert_eq!(ports.handle_live(&mut lane, event), LaneStatus::Complete);
        assert_eq!(lane.context().notifications, [outcome, outcome]);
        assert!(!observer.calls().iter().any(|c| c.0 == 0xf46c20));
        println!(
            "terminal binding smoke: {outcome:?}; resumed; stop calc drained; release completed"
        );
    }
}
