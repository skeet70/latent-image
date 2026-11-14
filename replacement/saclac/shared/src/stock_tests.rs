#![allow(unsafe_code)]

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use xm1_saclac_interface::stock::{
    callback_word, command_code, optional_callback, StockCallbackWord, PARAMETER_BLOCKS,
};
use xm1_saclac_interface::{ClientNotification, CommandKind, CompletionStatus, ReplyTarget};

use crate::commands::{ClientArguments, Clients, CommandMemory, StoredCommand};
use crate::display::{active_outputs, Display, DisplayRoute, Outputs};
use crate::image_registers::{RegisterGroup, RegisterSnapshot, WindowLimits};
use crate::lens::{
    notify_bounds, LensBounds, LensControl, LensNotices, LensProbe, Notice, ProbeState, Rendezvous,
};
use crate::lens_com::{LensComClient, RequestNumber};
use crate::settings::{
    AdjustmentKey, Adjustments, Aspect, CaptureMode, ChannelRemap, Conversions, RemapArguments, RemappedWord,
    WindowVariant,
};
use crate::stock::{Machine, Stock, StockConfiguration};

#[derive(Clone, Debug, PartialEq)]
enum Call {
    Read8(u32),
    Read16(u32),
    Read32(u32),
    Write8(u32, u8),
    Write32(u32, u32),
    Enter(u32, Vec<usize>),
}

struct State {
    memory: HashMap<u32, u8>,
    answers: HashMap<u32, VecDeque<u32>>,
    calls: Vec<Call>,
    route: u8,
    status: [u8; 60],
    record_at: Option<u32>,
    records: Vec<(u32, [u32; 4])>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            memory: HashMap::new(),
            answers: HashMap::new(),
            calls: Vec::new(),
            route: 0,
            status: [0; 60],
            record_at: None,
            records: Vec::new(),
        }
    }
}

impl State {
    fn byte(&self, address: u32) -> u8 { self.memory.get(&address).copied().unwrap_or(0) }

    fn word(&self, address: u32) -> u32 {
        u32::from_le_bytes([self.byte(address), self.byte(address + 1), self.byte(address + 2), self.byte(address + 3)])
    }

    fn write(&mut self, address: u32, value: u32, bytes: u32) {
        for offset in 0..bytes {
            self.memory.insert(address + offset, (value >> (8 * offset)) as u8);
        }
    }
}

#[derive(Clone, Default)]
struct Fake(Rc<RefCell<State>>);

impl Fake {
    fn set_word(&self, address: u32, value: u32) { self.0.borrow_mut().write(address, value, 4) }

    fn set_halfword(&self, address: u32, value: u16) { self.0.borrow_mut().write(address, u32::from(value), 2) }

    fn word(&self, address: u32) -> u32 { self.0.borrow().word(address) }

    fn answer(&self, address: u32, value: u32) { self.0.borrow_mut().answers.entry(address).or_default().push_back(value) }

    fn set_route(&self, route: u8) { self.0.borrow_mut().route = route }

    fn set_status(&self, status: [u8; 60]) { self.0.borrow_mut().status = status }

    fn expect_record(&self, address: u32) { self.0.borrow_mut().record_at = Some(address) }

    fn records(&self) -> Vec<(u32, [u32; 4])> { self.0.borrow().records.clone() }

    fn calls(&self) -> Vec<Call> { self.0.borrow().calls.clone() }

    fn take_calls(&self) -> Vec<Call> { std::mem::take(&mut self.0.borrow_mut().calls) }
}

impl Machine for Fake {
    fn read_u8(&self, address: u32) -> u8 {
        let mut state = self.0.borrow_mut();
        state.calls.push(Call::Read8(address));
        state.byte(address)
    }

    fn read_u16(&self, address: u32) -> u16 {
        let mut state = self.0.borrow_mut();
        state.calls.push(Call::Read16(address));
        u16::from_le_bytes([state.byte(address), state.byte(address + 1)])
    }

    fn read_u32(&self, address: u32) -> u32 {
        let mut state = self.0.borrow_mut();
        state.calls.push(Call::Read32(address));
        state.word(address)
    }

    fn write_u8(&mut self, address: u32, value: u8) {
        let mut state = self.0.borrow_mut();
        state.calls.push(Call::Write8(address, value));
        state.memory.insert(address, value);
    }

    fn write_u32(&mut self, address: u32, value: u32) {
        let mut state = self.0.borrow_mut();
        state.calls.push(Call::Write32(address, value));
        state.write(address, value, 4);
    }

    fn enter(&self, address: u32, arguments: &[usize]) -> u32 {
        let mut state = self.0.borrow_mut();
        state.calls.push(Call::Enter(address, arguments.to_vec()));
        match address {
            0x6c_68c0 => {
                unsafe { *(arguments[0] as *mut u8) = state.route };
                0
            }
            0x6c_4bec => {
                unsafe { core::ptr::copy_nonoverlapping(state.status.as_ptr(), arguments[0] as *mut u8, state.status.len()) };
                0
            }
            _ if state.record_at == Some(address) => {
                let words = unsafe { *(arguments[0] as *const [u32; 4]) };
                state.records.push((address, words));
                0
            }
            _ => state.answers.get_mut(&address).and_then(VecDeque::pop_front).unwrap_or(0),
        }
    }
}

fn stock() -> (Fake, Stock<Fake>) {
    let fake = Fake::default();
    let stock = Stock::new(fake.clone());
    (fake, stock)
}

#[test]
fn parameter_words_are_read_from_the_interface_block_table() {
    let (fake, stock) = stock();
    for command in [
        StoredCommand::Command02,
        StoredCommand::Command03,
        StoredCommand::LensBringUp,
        StoredCommand::LensStop,
        StoredCommand::HalfPress,
        StoredCommand::Autofocus,
        StoredCommand::LensDrive,
        StoredCommand::Cancel,
        StoredCommand::Command10,
        StoredCommand::Command17,
        StoredCommand::Step,
        StoredCommand::Command2c,
        StoredCommand::Command2d,
    ] {
        let block = PARAMETER_BLOCKS[usize::from(command.code())];
        let words = block.capacity / 4;
        let last = block.address + 4 * (words as u32 - 1);
        fake.set_word(last, 0x5a5a_1234);
        assert_eq!(CommandMemory::word(&stock, command, words - 1), 0x5a5a_1234, "{command:?}");
        assert!(fake.calls().contains(&Call::Read32(last)), "{command:?}");
    }
}

/// 0xef9cc4, 0xef9ce0, 0xef9b50, 0xef9ca8, 0xef9cfc and 0xefa00c return these blocks; 0x1b's and
/// 0x2c's follow the table.
#[test]
fn the_lens_blocks_are_the_ones_their_getters_return() {
    for (command, address) in [
        (StoredCommand::LensDrive, 0x169_d1c8),
        (StoredCommand::Cancel, 0x169_d1dc),
        (StoredCommand::Command02, 0x169_d004),
        (StoredCommand::Autofocus, 0x169_d128),
        (StoredCommand::Step, 0x169_d28c),
        (StoredCommand::Command2c, 0x169_d360),
        (StoredCommand::Command10, 0x169_d1e4),
        (StoredCommand::Command2d, 0x169_d368),
    ] {
        let (fake, mut stock) = stock();
        CommandMemory::set_word(&mut stock, command, 1, 0x1234);
        assert_eq!(fake.calls(), [Call::Write32(address + 4, 0x1234)], "{command:?}");
    }
}

#[test]
fn the_pending_flag_is_one_byte_per_message_code() {
    let (fake, mut stock) = stock();
    CommandMemory::set_pending(&mut stock, ReplyTarget::Command(CommandKind::Autofocus), true);
    CommandMemory::set_pending(&mut stock, ReplyTarget::Command(CommandKind::Command2d), false);
    CommandMemory::set_pending(&mut stock, ReplyTarget::Notification(ClientNotification::Notification1b), false);
    let autofocus = 0x169_cf24 + u32::from(command_code(CommandKind::Autofocus));
    let command2d = 0x169_cf24 + u32::from(command_code(CommandKind::Command2d));
    assert_eq!(fake.calls(), [Call::Write8(autofocus, 1), Call::Write8(command2d, 0), Call::Write8(0x169_cf24 + 0x1b, 0)]);
}

#[test]
fn client_replies_reach_the_callback_word_in_order() {
    let (fake, mut stock) = stock();
    let callback = optional_callback(0x00c0_ffee).unwrap();
    Clients::enter(&mut stock, callback, ClientArguments::Reply { status: CompletionStatus(-3), words: [7, 9, 11, 13] });
    Clients::enter(&mut stock, callback, ClientArguments::Status(CompletionStatus(5)));
    assert_eq!(fake.calls(), [
        Call::Enter(0x00c0_ffee, vec![(-3i32) as usize, 7, 9, 11, 13]),
        Call::Enter(0x00c0_ffee, vec![5]),
    ]);
    assert_eq!(callback_word(callback), 0x00c0_ffee);
}

#[test]
fn lens_probes_enter_their_entries_and_map_their_answers() {
    let (fake, mut stock) = stock();
    for (probe, address, answer, expected) in [
        (LensProbe::First, 0x76_d9a8, 2, ProbeState::Overridden),
        (LensProbe::Second, 0x76_da38, 1, ProbeState::Reported),
        (LensProbe::Third, 0x76_dba4, 0, ProbeState::Clear),
        (LensProbe::First, 0x76_d9a8, 3, ProbeState::Clear),
    ] {
        fake.answer(address, answer);
        assert_eq!(LensControl::probe(&mut stock, probe), expected);
        assert!(fake.calls().contains(&Call::Enter(address, Vec::new())), "{address:#x}");
    }
}

#[test]
fn lens_state_reads_its_runtime_words_and_entries() {
    let (fake, mut stock) = stock();
    fake.answer(0x76_da48, 0x80);
    assert_eq!(LensControl::value(&mut stock), 0x80);
    fake.set_word(0x94_2ab0 + 0x98, 0x1234_5678);
    assert_eq!(LensControl::step(&mut stock), 0x1234_5678);
    assert!(fake.calls().contains(&Call::Read32(0x94_2ab0 + 0x98)));
    fake.answer(0x76_b8b4, 1);
    assert!(LensControl::prefers_relative(&mut stock));
    fake.answer(0x77_2f28, 0x1234);
    assert_eq!(LensControl::zone_limit(&mut stock, 5), 0x1234);
    assert!(fake.calls().contains(&Call::Enter(0x77_2f28, vec![5])));
}

#[test]
fn lens_notices_read_and_clear_the_shared_block() {
    let (fake, mut stock) = stock();
    fake.set_word(0x95_4dbc + 0x4, 11);
    fake.set_word(0x95_4dbc + 0x8, 22);
    fake.set_word(0x95_4dbc + 0xc, 33);
    assert_eq!(LensNotices::notice(&stock, Notice::First), 11);
    assert_eq!(LensNotices::notice(&stock, Notice::Second), 22);
    assert_eq!(LensNotices::notice(&stock, Notice::Third), 33);
    fake.set_word(0x95_4dbc + 0x1c, 0x1234);
    fake.set_word(0x95_4dbc + 0x20, 1);
    assert!(LensNotices::notifier_enabled(&stock));
    assert_eq!(LensNotices::notifier(&stock), Some(StockCallbackWord(0x1234)));
    fake.set_word(0x95_4dbc + 0x1c, 0);
    assert_eq!(LensNotices::notifier(&stock), None);
    fake.take_calls();
    LensNotices::clear_notices(&mut stock);
    assert_eq!(fake.calls(), [Call::Write32(0x95_4dbc + 0x4, 0), Call::Write32(0x95_4dbc + 0x8, 0), Call::Write32(0x95_4dbc + 0xc, 0)]);
}

#[test]
fn the_bounds_notifier_receives_the_detail_then_the_bounds() {
    let (fake, mut stock) = stock();
    fake.set_word(0x95_4dbc + 0x1c, 0x00c0_ffee);
    fake.set_word(0x95_4dbc + 0x20, 1);
    fake.expect_record(0x00c0_ffee);
    notify_bounds(5, LensBounds([10, 20, 30]), &mut stock);
    assert_eq!(fake.records(), [(0x00c0_ffee, [5, 10, 20, 30])]);
}

#[test]
fn the_rendezvous_records_the_current_task_and_waits_on_event_two() {
    let (fake, mut stock) = stock();
    fake.answer(0x72_3cb8, 0x00ab_cdef);
    assert_eq!(Rendezvous::arm(&mut stock), StockCallbackWord(0xf4_6be0));
    assert_eq!(fake.word(0x16c_2214), 0x00ab_cdef);
    fake.take_calls();
    Rendezvous::wait(&mut stock);
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    let Call::Enter(address, arguments) = &calls[0] else { panic!("the wait entered no entry") };
    assert_eq!(*address, 0x72_43a8);
    assert_eq!(arguments[0], 2);
    assert_ne!(arguments[1], 0);
    assert_eq!(arguments[2], u32::MAX as usize);
}

#[test]
fn adjustment_reads_enter_the_byte_entry_with_the_key() {
    let (fake, stock) = stock();
    fake.answer(0x69_f8fc, 0x7f);
    assert_eq!(Adjustments::byte(&stock, AdjustmentKey(0x1234)), 0x7f);
    assert!(fake.calls().contains(&Call::Enter(0x69_f8fc, vec![0x1234])));
    fake.take_calls();
    fake.answer(0x69_f8fc, 0x34);
    fake.answer(0x69_f8fc, 0x12);
    assert_eq!(Adjustments::lens_adjustment(&stock), 0x1234);
    assert_eq!(fake.calls(), [Call::Enter(0x69_f8fc, vec![0x3e_0243]), Call::Enter(0x69_f8fc, vec![0x3e_0244])]);
    fake.take_calls();
    fake.answer(0x69_f8fc, 1);
    assert!(Adjustments::admits_unselected_mode(&stock));
    assert_eq!(fake.calls(), [Call::Enter(0x69_f8fc, vec![0x887c])]);
}

#[test]
fn the_remap_forwards_seven_words_in_order() {
    let (fake, mut stock) = stock();
    fake.answer(0x74_947c, 0xabcd_1234);
    assert_eq!(ChannelRemap::remap(&mut stock, RemapArguments([1, 2, 3, 4, 5, 6, 7])), RemappedWord(0x1234));
    assert_eq!(fake.calls(), [Call::Enter(0x74_947c, vec![1, 2, 3, 4, 5, 6, 7])]);
}

#[test]
fn conversions_enter_the_selection_with_the_aspect_index() {
    let (fake, mut stock) = stock();
    for (aspect, index) in [(Aspect::ThreeFour, 0), (Aspect::TwoThree, 1), (Aspect::NineSixteen, 2), (Aspect::Square, 3)] {
        fake.answer(0x73_958c, 0x12);
        assert_eq!(Conversions::window_variant(&mut stock, CaptureMode::new(4).unwrap(), aspect), Some(WindowVariant(0x12)));
        assert!(fake.calls().contains(&Call::Enter(0x73_958c, vec![4, index])));
    }
    fake.answer(0x73_958c, 0xff);
    assert_eq!(Conversions::window_variant(&mut stock, CaptureMode::new(4).unwrap(), Aspect::Square), None);
    fake.answer(0x73_4540, 100);
    fake.answer(0x73_469c, 200);
    assert_eq!(Conversions::value(&mut stock, WindowVariant(7), Aspect::TwoThree), 100);
    assert_eq!(Conversions::alternate(&mut stock, WindowVariant(7), Aspect::TwoThree), 200);
    assert!(fake.calls().contains(&Call::Enter(0x73_4540, vec![7, 1])));
    assert!(fake.calls().contains(&Call::Enter(0x73_469c, vec![7, 1])));
}

/// The fifteen table and count pairs at 0x693df8..0x694060.
#[test]
fn each_group_restores_through_its_descriptor_table() {
    let (fake, mut stock) = stock();
    let tables: [(u32, u32); 15] = [
        (0x82_158c, 3),
        (0x82_15b0, 9),
        (0x82_161c, 2),
        (0x82_1634, 3),
        (0x82_1658, 2),
        (0x82_1670, 6),
        (0x82_16b8, 3),
        (0x82_16dc, 1),
        (0x82_16e8, 2),
        (0x82_1700, 2),
        (0x82_1718, 7),
        (0x82_176c, 2),
        (0x82_1784, 1),
        (0x82_1790, 10),
        (0x82_1808, 3),
    ];
    for (group, (descriptors, count)) in RegisterGroup::ALL.into_iter().zip(tables) {
        RegisterSnapshot::restore(&mut stock, group);
        assert!(
            fake.calls().contains(&Call::Enter(0x6d_eb08, vec![0x14c_6b24, descriptors as usize, count as usize])),
            "{group:?}"
        );
    }
}

#[test]
fn window_limits_read_the_configuration_halfwords() {
    let (fake, stock) = stock();
    let configuration = StockConfiguration::from_pipeline(0x2000);
    fake.set_halfword(0x203e, 0x1234);
    fake.set_halfword(0x2040, 0x0567);
    fake.set_halfword(0x2042, 0xffff);
    assert_eq!(RegisterSnapshot::window_limits(&stock, configuration), WindowLimits([0x1234, 0x0567, -1]));
    assert!(fake.calls().contains(&Call::Read16(0x203e)));
    assert!(fake.calls().contains(&Call::Read16(0x2040)));
    assert!(fake.calls().contains(&Call::Read16(0x2042)));
}

#[test]
fn the_snapshot_words_are_read_and_written_at_their_offsets() {
    let (fake, mut stock) = stock();
    fake.set_word(0x14c_6b24 + 0x3c, 0x1111_2222);
    fake.set_word(0x14c_6b24 + 0x40, 0x3333_4444);
    assert_eq!(RegisterSnapshot::window_words(&stock), [0x1111_2222, 0x3333_4444]);
    fake.take_calls();
    RegisterSnapshot::set_window_words(&mut stock, [0x5555_6666, 0x7777_8888]);
    assert_eq!(fake.calls(), [
        Call::Write32(0x14c_6b24 + 0x3c, 0x5555_6666),
        Call::Write32(0x14c_6b24 + 0x40, 0x7777_8888),
    ]);
}

#[test]
fn group_two_packs_its_limits_and_every_other_group_enters_its_packer() {
    let (fake, mut stock) = stock();
    let configuration = StockConfiguration::from_pipeline(0x2000);
    fake.set_halfword(0x203e, 1);
    fake.set_halfword(0x2040, 2);
    fake.set_halfword(0x2042, 3);
    RegisterSnapshot::pack(&mut stock, RegisterGroup::Group2, configuration);
    assert_eq!(fake.word(0x14c_6b24 + 0x3c), 0x0002_0001);
    assert_eq!(fake.word(0x14c_6b24 + 0x40), 3);
    assert!(!fake.calls().iter().any(|call| matches!(call, Call::Enter(..))));
    let packers: [u32; 14] = [
        0x69_13a8, 0x69_191c, 0x69_1f74, 0x69_2190, 0x69_23d0, 0x69_26e0, 0x69_2a88, 0x69_2b8c, 0x69_2ce8, 0x69_2e14,
        0x69_322c, 0x69_3390, 0x69_34dc, 0x69_3aec,
    ];
    let others: Vec<_> = RegisterGroup::ALL.into_iter().filter(|group| *group != RegisterGroup::Group2).collect();
    for (group, packer) in others.into_iter().zip(packers) {
        fake.take_calls();
        RegisterSnapshot::pack(&mut stock, group, configuration);
        assert_eq!(fake.calls(), [Call::Enter(packer, vec![0x14c_6b24, 0x2000])], "{group:?}");
    }
}

#[test]
fn lenscom_requests_remember_the_client_and_report_a_refused_post() {
    let (fake, mut stock) = stock();
    let number = RequestNumber::new(3).unwrap();
    LensComClient::remember(&mut stock, Some(StockCallbackWord(0x00c0_ffee)), number);
    assert_eq!(fake.word(0x8e_1f3c + 0x8), 0x00c0_ffee);
    assert_eq!(fake.word(0x8e_1f3c + 0xc), 3);
    assert_eq!(LensComClient::completion(&stock), StockCallbackWord(0x6b_6f9c));
    LensComClient::complete(&mut stock, StockCallbackWord(0x00c0_ffee), number);
    assert!(fake.calls().contains(&Call::Enter(0x00c0_ffee, vec![3])));
    LensComClient::report_unposted(&mut stock);
    assert!(fake.calls().contains(&Call::Enter(0x6b_92cc, vec![1, 0x27, 0])));
}

#[test]
fn display_route_gate_and_outputs_come_from_the_driver_entries() {
    let (fake, stock) = stock();
    fake.set_route(3);
    assert_eq!(Display::route(&stock), DisplayRoute(3));
    assert!(fake.calls().iter().any(|call| matches!(call, Call::Enter(0x6c_68c0, _))));
    let mut status = [0u8; 60];
    status[16] = 1;
    status[19] = 1;
    fake.set_status(status);
    assert!(!Display::gate_open(&stock));
    assert_eq!(Display::outputs(&stock), Outputs(0b1001));
    status[7] = 1;
    fake.set_status(status);
    assert!(Display::gate_open(&stock));
    fake.set_route(2);
    assert_eq!(active_outputs(&stock), Outputs(0b1001));
    status[7] = 0;
    fake.set_status(status);
    assert_eq!(active_outputs(&stock), Outputs(0));
    assert!(fake.calls().iter().all(|call| matches!(call, Call::Enter(0x6c_4bec, _) | Call::Enter(0x6c_68c0, _))));
}
