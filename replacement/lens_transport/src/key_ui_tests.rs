extern crate std;

use super::*;
use std::collections::VecDeque;
use std::vec::Vec;

struct Ingress {
    main: VecDeque<NotificationMessage>,
    loadtest: VecDeque<NotificationMessage>,
    test_mode: LoadtestState,
    status: PublicationStatus,
    traces: Vec<PublicationTrace>,
    counted: Vec<KeyEvent>,
    initialized: usize,
    selection: Option<StabilizationSelection>,
}

impl Ingress {
    fn new() -> Self {
        Self {
            main: VecDeque::new(), loadtest: VecDeque::new(), test_mode: LoadtestState::Inactive,
            status: PublicationStatus(0), traces: Vec::new(), counted: Vec::new(),
            initialized: 0, selection: None,
        }
    }
}

impl PublicationServices for Ingress {
    fn current_task(&mut self) -> PublicationTask { PublicationTask(99) }
    fn loadtest_state(&mut self) -> LoadtestState { self.test_mode }
    fn send_notification(&mut self, message: NotificationMessage) -> PublicationStatus {
        if self.status == PublicationStatus(0) {
            match message.destination {
                UiQueue::Main => self.main.push_back(message),
                UiQueue::Loadtest => self.loadtest.push_back(message),
            }
        }
        self.status
    }
    fn trace_publication(&mut self, trace: PublicationTrace) { self.traces.push(trace); }
    fn count_key(&mut self, key: KeyEvent) { self.counted.push(key); }
    fn initialize_ui_notification_source(&mut self) { self.initialized += 1; }
}

impl StabilizationServices for Ingress {
    fn begin_stabilization_mode(&mut self, selection: StabilizationSelection) { self.selection = Some(selection); }
}

fn state() -> NotificationState { NotificationState { update_count: 0, expected_update_count: 0 } }

#[test]
fn input_publications_preserve_all_payload_bytes_and_sender() {
    let mut io = Ingress::new();
    let mut state = state();
    let mut data = [0; 68];
    for (i, byte) in data.iter_mut().enumerate() { *byte = i as u8; }
    data[..2].copy_from_slice(&(-17i16).to_le_bytes());
    let event = KeyPublication { key: KeyEvent(0x424), argument: 0x8765_4321, data };
    Publisher { services: &mut io, state: &mut state }.publish_key(event);
    let packet = io.main.pop_front().unwrap();
    assert_eq!(packet.sender, PublicationTask(99));
    assert_eq!(packet.publication, event);
    let bytes = packet.payload();
    assert_eq!(&bytes[..8], &[0x24, 4, 0, 0, 0x21, 0x43, 0x65, 0x87]);
    assert_eq!(&bytes[8..10], &[0xef, 0xff]);
    assert_eq!(&bytes[10..], &data[2..]);
}

#[test]
fn only_active_loadtest_and_a_masked_event_select_queue_seven() {
    for mode in [LoadtestState::Inactive, LoadtestState::Active] {
        for (code, masked) in [(0x38, false), (0x420, false), (0x424, false), (0x91d, true), (0x100, true), (0x800, true)] {
            let mut io = Ingress::new();
            io.test_mode = mode;
            let mut state = state();
            let status = Publisher { services: &mut io, state: &mut state }.release(KeyEvent(code), 7);
            assert_eq!(status, PublicationStatus(0));
            let expected_loadtest = mode == LoadtestState::Active && masked;
            assert_eq!(io.loadtest.len(), usize::from(expected_loadtest));
            assert_eq!(io.main.len(), usize::from(!expected_loadtest));
            let packet = io.main.pop_front().or_else(|| io.loadtest.pop_front()).unwrap();
            assert_eq!(packet.publication.argument, 7);
            assert_eq!(packet.publication.data, [0; 68]);
        }
    }
}

#[test]
fn key_diagnostics_apply_to_both_publication_forms() {
    let mut io = Ingress::new();
    let mut state = state();
    let mut publisher = Publisher { services: &mut io, state: &mut state };
    publisher.release(KeyEvent(7), 0);
    publisher.publish(KeyPublication { key: KeyEvent(8), argument: 0, data: [3; 68] });
    for key in [0x2a, 0x2b, 0x2c, 0x2d, 0x2e] { publisher.release(KeyEvent(key), 0); }
    assert_eq!(io.traces, [PublicationTrace::PrimaryPress, PublicationTrace::FullPress]);
    assert_eq!(io.counted, [KeyEvent(0x2b), KeyEvent(0x2c), KeyEvent(0x2d)]);
    assert_eq!(io.main.len(), 7);
}

#[test]
fn update_count_suppresses_only_a_mismatched_main_queue_update() {
    let mut io = Ingress::new();
    let mut state = NotificationState { update_count: 10, expected_update_count: 10 };
    let event = KeyPublication { key: KeyEvent(0x82e), argument: 3, data: [0; 68] };
    assert_eq!(Publisher { services: &mut io, state: &mut state }.notify(event), PublicationStatus(0));
    assert_eq!(state.update_count, 11);
    io.status = PublicationStatus(-9);
    assert_eq!(Publisher { services: &mut io, state: &mut state }.notify(event), PublicationStatus(0));
    assert_eq!(io.main.len(), 1);
    assert_eq!(io.traces, [PublicationTrace::UpdateSuppressed]);
    io.test_mode = LoadtestState::Active;
    assert_eq!(Publisher { services: &mut io, state: &mut state }.notify(event), PublicationStatus(-9));
    assert_eq!(state.update_count, 11);
}

#[test]
fn update_count_wraps_and_notification_status_is_preserved() {
    let mut io = Ingress::new();
    io.status = PublicationStatus(-17);
    let mut state = NotificationState { update_count: u32::MAX, expected_update_count: u32::MAX };
    let mut publisher = Publisher { services: &mut io, state: &mut state };
    assert_eq!(publisher.release(KeyEvent(0x82e), 0), PublicationStatus(-17));
    assert_eq!(publisher.state.update_count, 0);
    assert_eq!(publisher.release(KeyEvent(0x91b), 0), PublicationStatus(-17));
    assert_eq!(io.initialized, 1);
    assert!(io.main.is_empty());
}

#[test]
fn stabilization_change_requires_registration_and_persists_across_notifications() {
    let mut io = Ingress::new();
    let mut state = state();
    let mut callbacks = StabilizationCallbacks::new();
    let mut publisher = Publisher { services: &mut io, state: &mut state };
    callbacks.changed(&mut publisher);
    assert!(publisher.services.main.is_empty());
    callbacks.register(StabilizationMode::Mode2, None, Some(UiCallback::StabilizationChanged), &mut publisher);
    assert!(publisher.services.main.is_empty());
    assert_eq!(publisher.services.selection, Some(StabilizationSelection::Second));
    callbacks.changed(&mut publisher);
    callbacks.register(StabilizationMode::Mode3, None, None, &mut publisher);
    callbacks.changed(&mut publisher);
    assert_eq!(publisher.services.main.len(), 2);
    for message in &publisher.services.main {
        assert_eq!(message.publication, KeyPublication { key: KeyEvent(0x91d), argument: 0, data: [0; 68] });
    }
    callbacks.reset();
    callbacks.changed(&mut publisher);
    assert_eq!(publisher.services.main.len(), 2);
}

#[test]
fn mode_completion_is_deferred_only_for_modes_one_to_three() {
    for raw in 0..10 {
        let mut io = Ingress::new();
        let mut state = state();
        let mut callbacks = StabilizationCallbacks::new();
        let mut publisher = Publisher { services: &mut io, state: &mut state };
        let mode = StabilizationMode::try_from(raw).unwrap();
        callbacks.register(mode, Some(UiCallback::StabilizationChanged), Some(UiCallback::StabilizationChanged), &mut publisher);
        let deferred = (1..=3).contains(&raw);
        assert_eq!(publisher.services.main.len(), usize::from(!deferred));
        if deferred {
            callbacks.mode_completed(&mut publisher);
            callbacks.mode_completed(&mut publisher);
            assert_eq!(publisher.services.main.len(), 2);
        } else {
            callbacks.changed(&mut publisher);
            assert_eq!(publisher.services.main.len(), 1);
        }
    }
    assert_eq!(StabilizationMode::try_from(10), Err(PublicationStatus(-255)));
    assert_eq!(StabilizationMode::try_from(u32::MAX), Err(PublicationStatus(-255)));
}
