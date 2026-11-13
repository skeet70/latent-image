use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use xm1_saclac_interface::stock::{callback_word, StockCallbackWord};
use xm1_saclac_interface::{ClientCallback, CommandKind, CompletionStatus};

use crate::commands::*;
use crate::lens::{LensBounds, LensControl, LensNotices, LensProbe, Notice, ProbeState};
use crate::settings::{Channel, Channels};

#[derive(Clone, Debug, PartialEq)]
enum Event {
    Pending(CommandKind, bool),
    Entered(u32, ClientArguments),
    NoticesCleared,
}

type Log = Rc<RefCell<Vec<Event>>>;

#[derive(Default)]
struct Memory {
    words: HashMap<(StoredCommand, usize), u32>,
    log: Log,
}

impl Memory {
    fn with(words: &[(StoredCommand, usize, u32)]) -> Self {
        Self { words: words.iter().map(|(command, index, word)| ((*command, *index), *word)).collect(), log: Log::default() }
    }
}

impl CommandMemory for Memory {
    fn word(&self, command: StoredCommand, index: usize) -> u32 { self.words.get(&(command, index)).copied().unwrap_or(0) }

    fn set_word(&mut self, command: StoredCommand, index: usize, value: u32) { self.words.insert((command, index), value); }

    fn set_pending(&mut self, command: CommandKind, pending: bool) { self.log.borrow_mut().push(Event::Pending(command, pending)) }
}

struct Recorder(Log);

impl Clients for Recorder {
    fn enter(&mut self, callback: ClientCallback, arguments: ClientArguments) {
        self.0.borrow_mut().push(Event::Entered(callback_word(callback), arguments))
    }
}

struct Configured(i32);

impl Channels for Configured {
    fn is_valid(&self, channel: Channel) -> bool { (0..self.0).contains(&channel.0) }
}

const CALLBACK: u32 = 0x00c0_ffee;

fn commands(words: &[(StoredCommand, usize, u32)]) -> (Commands<Memory>, Recorder, Log) {
    let memory = Memory::with(words);
    let log = memory.log.clone();
    (Commands::new(memory), Recorder(log.clone()), log)
}

fn reply(status: i32, channel: i32) -> LensDriveReply { LensDriveReply { status: CompletionStatus(status), channel: Channel(channel), words: [7, 9] } }

#[test]
fn stored_parameters_are_read_inside_their_block_only() {
    let (commands, ..) = commands(&[(StoredCommand::HalfPress, 15, 0x55), (StoredCommand::LensDrive, 4, 0x66)]);
    assert_eq!(commands.word(StoredCommand::HalfPress, 15), Some(0x55));
    assert_eq!(commands.word(StoredCommand::LensDrive, 4), Some(0x66));
    assert_eq!(commands.word(StoredCommand::LensDrive, 5), None);
    assert_eq!(commands.word(StoredCommand::Command03, 6), None);
}

#[test]
fn stored_parameters_are_written_inside_their_block_only() {
    let (mut commands, ..) = commands(&[]);
    assert!(commands.set_word(StoredCommand::LensDrive, 3, 0x77));
    assert!(!commands.set_word(StoredCommand::Cancel, 2, 0x88));
    assert_eq!(commands.word(StoredCommand::LensDrive, 3), Some(0x77));
    assert_eq!(commands.memory().words.get(&(StoredCommand::Cancel, 2)), None);
}

#[test]
fn each_command_reads_the_words_its_parameter_block_holds() {
    for (command, words) in [
        (StoredCommand::Command02, 7),
        (StoredCommand::Command03, 6),
        (StoredCommand::LensBringUp, 6),
        (StoredCommand::LensStop, 3),
        (StoredCommand::HalfPress, 16),
        (StoredCommand::Autofocus, 40),
        (StoredCommand::LensDrive, 5),
        (StoredCommand::Cancel, 2),
        (StoredCommand::Command17, 10),
        (StoredCommand::Step, 2),
        (StoredCommand::Command2c, 2),
    ] {
        let (commands, ..) = commands(&[]);
        assert_eq!(command.words(), words, "{command:?}");
        assert_eq!(commands.word(command, words - 1), Some(0), "{command:?}");
        assert_eq!(commands.word(command, words), None, "{command:?}");
    }
}

#[test]
fn only_final_statuses_end_a_command() {
    for status in [0, -1, -255, 20, 21] {
        assert!(ends_command(CompletionStatus(status)), "{status}");
    }
    for status in [1, 2, 19, 22, 0x7fff_ffff] {
        assert!(!ends_command(CompletionStatus(status)), "{status}");
    }
}

#[test]
fn a_lens_drive_reply_clears_the_pending_flag_then_enters_the_client() {
    let (mut commands, mut clients, log) = commands(&[(StoredCommand::LensDrive, 0, CALLBACK), (StoredCommand::LensDrive, 2, 0x1234)]);
    commands.reply_lens_drive(reply(0, 3), &Configured(4), &mut clients);
    assert_eq!(*log.borrow(), [
        Event::Pending(CommandKind::LensDrive, false),
        Event::Entered(CALLBACK, ClientArguments::Reply { status: CompletionStatus(0), words: [3, 0x1234, 7, 9] }),
    ]);
}

#[test]
fn a_progress_reply_leaves_the_command_pending() {
    let (mut commands, mut clients, log) = commands(&[(StoredCommand::LensDrive, 0, CALLBACK)]);
    commands.reply_lens_drive(reply(5, 0), &Configured(1), &mut clients);
    assert!(matches!(log.borrow().as_slice(), [Event::Entered(CALLBACK, _)]));
}

#[test]
fn an_autofocus_progress_reply_leaves_the_command_pending() {
    let (mut commands, mut clients, log) = commands(&[(StoredCommand::Autofocus, 0, CALLBACK), (StoredCommand::Autofocus, 39, 0xabcd)]);
    let mut notices = Notices { words: [0; 3], log: log.clone() };
    commands.reply_autofocus_lens(CompletionStatus(5), &mut Lens { states: [ProbeState::Clear; 3], value: 0x80 }, &mut notices, &mut clients);
    assert!(matches!(log.borrow().as_slice(), [Event::NoticesCleared, Event::Entered(CALLBACK, _)]));
}

#[test]
fn an_unconfigured_channel_replaces_the_clients_word() {
    for channel in [-1, 4, 100] {
        let (mut commands, mut clients, log) = commands(&[(StoredCommand::LensDrive, 0, CALLBACK), (StoredCommand::LensDrive, 2, 0x1234)]);
        commands.reply_lens_drive(reply(-3, channel), &Configured(4), &mut clients);
        let words = [channel as u32, UNCONFIGURED_CHANNEL, 7, 9];
        assert_eq!(log.borrow().last(), Some(&Event::Entered(CALLBACK, ClientArguments::Reply { status: CompletionStatus(-3), words })));
    }
    assert_eq!(UNCONFIGURED_CHANNEL, 0x0080_0000);
}

#[test]
fn a_command_without_a_callback_is_still_finished() {
    let (mut commands, mut clients, log) = commands(&[]);
    commands.reply_lens_drive(reply(21, 0), &Configured(1), &mut clients);
    commands.reply_command17(CompletionStatus(20), &mut clients);
    assert_eq!(*log.borrow(), [Event::Pending(CommandKind::LensDrive, false), Event::Pending(CommandKind::Command17, false)]);
}

#[test]
fn command17_replies_carry_the_status_alone() {
    let (mut commands, mut clients, log) = commands(&[(StoredCommand::Command17, 0, CALLBACK)]);
    commands.reply_command17(CompletionStatus(3), &mut clients);
    assert_eq!(*log.borrow(), [Event::Entered(CALLBACK, ClientArguments::Status(CompletionStatus(3)))]);
}

#[test]
fn pending_flags_are_set_for_any_command() {
    let (mut commands, _, log) = commands(&[]);
    commands.set_pending(CommandKind::Command2d, true);
    assert_eq!(*log.borrow(), [Event::Pending(CommandKind::Command2d, true)]);
}

struct Lens {
    states: [ProbeState; 3],
    value: u32,
}

impl LensControl for Lens {
    fn probe(&mut self, probe: LensProbe) -> ProbeState {
        self.states[match probe {
            LensProbe::First => 0,
            LensProbe::Second => 1,
            LensProbe::Third => 2,
        }]
    }

    fn value(&mut self) -> u32 { self.value }

    fn step(&mut self) -> u32 { unreachable!("the autofocus reply reads no step") }

    fn prefers_relative(&mut self) -> bool { unreachable!("the autofocus reply reads no preference") }

    fn zone_limit(&mut self, _: u8) -> i16 { unreachable!("the autofocus reply reads no zones") }
}

struct Notices {
    words: [u32; 3],
    log: Log,
}

impl LensNotices for Notices {
    fn notice(&self, notice: Notice) -> u32 {
        self.words[match notice {
            Notice::First => 0,
            Notice::Second => 1,
            Notice::Third => 2,
        }]
    }

    fn clear_notices(&mut self) {
        self.words = [0; 3];
        self.log.borrow_mut().push(Event::NoticesCleared);
    }

    fn notifier_enabled(&self) -> bool { false }

    fn notifier(&self) -> Option<StockCallbackWord> { None }

    fn enter_notifier(&mut self, _: StockCallbackWord, _: u32, _: LensBounds) { unreachable!("no notifier is set") }
}

fn autofocus_reply(states: [ProbeState; 3], words: [u32; 3]) -> Vec<Event> {
    let (mut commands, mut clients, log) = commands(&[(StoredCommand::Autofocus, 0, CALLBACK), (StoredCommand::Autofocus, 39, 0xabcd)]);
    let mut notices = Notices { words, log: log.clone() };
    commands.reply_autofocus_lens(CompletionStatus(0), &mut Lens { states, value: 0x80 }, &mut notices, &mut clients);
    assert_eq!(notices.words, [0; 3]);
    let events = log.borrow().clone();
    events
}

#[test]
fn the_autofocus_lens_reply_consumes_the_notices_before_finishing() {
    let events = autofocus_reply([ProbeState::Clear; 3], [0, 1, 0]);
    assert_eq!(events, [
        Event::NoticesCleared,
        Event::Pending(CommandKind::Autofocus, false),
        Event::Entered(CALLBACK, ClientArguments::Reply { status: CompletionStatus(0), words: [0x80, 0xabcd, 1, 0] }),
    ]);
}

#[test]
fn only_a_notice_of_one_is_reported() {
    let events = autofocus_reply([ProbeState::Reported; 3], [2, 0, 0xffff_ffff]);
    assert_eq!(events.last(), Some(&Event::Entered(CALLBACK, ClientArguments::Reply { status: CompletionStatus(0), words: [0x80, 0xabcd, 0, 0] })));
}

#[test]
fn overridden_probes_set_their_reply_bits() {
    use ProbeState::{Clear, Overridden};
    for (states, bits) in [
        ([Overridden, Clear, Clear], 0b010),
        ([Clear, Overridden, Clear], 0b001),
        ([Clear, Clear, Overridden], 0b100),
        ([Overridden; 3], 0b111),
    ] {
        let events = autofocus_reply(states, [0; 3]);
        assert_eq!(events.last(), Some(&Event::Entered(CALLBACK, ClientArguments::Reply { status: CompletionStatus(0), words: [0x80, 0xabcd, 0, bits] })));
    }
}
