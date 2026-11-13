use crate::layers::{LensControl, LensProbe};
use crate::replies::{Clients, CommandBlocks, Composer, Replies, ReplyCommand, Values};
use std::cell::RefCell;
use std::{vec, vec::Vec};
use xm1_saclac_interface::{stock, ClientCallback, CommandKind as K, CompletionStatus};

#[derive(Clone, Debug, Eq, PartialEq)]
enum Seen {
    Ended(K),
    Entered(u32, i32, Vec<u32>),
    EnteredRecord(u32, i32, Vec<u8>),
}

#[derive(Default)]
struct Client {
    blocks: Vec<(K, Vec<u32>)>,
    overridden: Vec<LensProbe>,
}

impl Client {
    fn with(command: K, words: &[u32]) -> Self { Self { blocks: vec![(command, words.to_vec())], ..Self::default() } }

    fn reply(&mut self, command: ReplyCommand, status: i32, values: Values<'_>) -> Vec<Seen> {
        let log = RefCell::new(Vec::new());
        let mut blocks = Blocks { words: &self.blocks, log: &log };
        let mut lens = Lens(self.overridden.clone());
        let mut clients = Entered(&log);
        Composer { blocks: &mut blocks, lens: &mut lens, clients: &mut clients }.reply(command, CompletionStatus(status), values);
        log.into_inner()
    }
}

struct Blocks<'a> { words: &'a [(K, Vec<u32>)], log: &'a RefCell<Vec<Seen>> }

impl CommandBlocks for Blocks<'_> {
    fn word(&mut self, command: K, index: usize) -> u32 {
        self.words.iter().find(|(known, _)| *known == command).and_then(|(_, words)| words.get(index).copied()).unwrap_or(0)
    }
    fn end(&mut self, command: K) { self.log.borrow_mut().push(Seen::Ended(command)) }
}

struct Lens(Vec<LensProbe>);

impl LensControl for Lens {
    fn overridden(&mut self, probe: LensProbe) -> bool { self.0.contains(&probe) }
    fn value(&mut self) -> u32 { unreachable_lens() }
    fn position(&mut self) -> u32 { unreachable_lens() }
    fn mode_index(&mut self) -> u32 { unreachable_lens() }
    fn reference(&mut self) -> u32 { unreachable_lens() }
    fn step(&mut self) -> u32 { unreachable_lens() }
    fn channel(&mut self) -> (u32, u16) { unreachable_lens() }
}

fn unreachable_lens() -> ! { panic!("a reply reads only the lens probes") }

struct Entered<'a>(&'a RefCell<Vec<Seen>>);

impl Clients for Entered<'_> {
    fn enter(&mut self, callback: ClientCallback, status: CompletionStatus, values: Values<'_>) {
        let callback = stock::callback_word(callback);
        self.0.borrow_mut().push(match values {
            Values::Words(words) => Seen::Entered(callback, status.0, words.to_vec()),
            Values::Record(record) => Seen::EnteredRecord(callback, status.0, record.to_vec()),
        });
    }
}

#[test]
fn a_final_reply_ends_its_command_before_the_client_sees_it() {
    for status in [i32::MIN, -96, -1, 0, 20, 21] {
        let mut client = Client::with(K::Command1e, &[0xf0_1e00]);
        assert_eq!(
            client.reply(ReplyCommand::Command1e, status, Values::Words(&[])),
            vec![Seen::Ended(K::Command1e), Seen::Entered(0xf0_1e00, status, vec![])],
        );
    }
}

#[test]
fn a_progress_reply_leaves_its_command_pending() {
    for status in [1, 16, 17, 18, 19, 22, i32::MAX] {
        let mut client = Client::with(K::Command09, &[0xf0_0900]);
        assert_eq!(client.reply(ReplyCommand::Command09, status, Values::Words(&[0])), vec![Seen::Entered(0xf0_0900, status, vec![0])]);
    }
}

#[test]
fn a_command_without_a_callback_is_still_ended() {
    let mut client = Client::default();
    assert_eq!(client.reply(ReplyCommand::Command13, 20, Values::Words(&[])), vec![Seen::Ended(K::Command13)]);
}

#[test]
fn command0a_finishes_through_its_own_entry_and_the_release_through_command09() {
    let mut client = Client { blocks: vec![(K::Command0a, vec![0xa]), (K::Command09, vec![0x9])], ..Client::default() };
    assert_eq!(client.reply(ReplyCommand::Command0a, 21, Values::Words(&[])), vec![Seen::Ended(K::Command0a), Seen::Entered(0xa, 21, vec![])]);
    assert_eq!(client.reply(ReplyCommand::Command09, 0, Values::Words(&[0])), vec![Seen::Ended(K::Command09), Seen::Entered(0x9, 0, vec![0])]);
}

#[test]
fn profile_replies_carry_their_own_parameter_word_third() {
    let mut client = Client::with(K::Command02, &[0x200, 1, 2, 3, 4, 5, 0x66]);
    assert_eq!(
        client.reply(ReplyCommand::Command02, 0, Values::Words(&[0xd, 0x80, 0, 3])),
        vec![Seen::Ended(K::Command02), Seen::Entered(0x200, 0, vec![0xd, 0x80, 0x66, 0, 3])],
    );
    let mut client = Client::with(K::Command03, &[0x300, 1, 2, 3, 4, 0x55]);
    assert_eq!(
        client.reply(ReplyCommand::Command03, 0, Values::Words(&[0xd, 0x80, 7, 1])),
        vec![Seen::Ended(K::Command03), Seen::Entered(0x300, 0, vec![0xd, 0x80, 0x55, 7, 1])],
    );
}

#[test]
fn the_command11_reply_adds_the_lens_override_flags() {
    for (overridden, flags) in [(vec![], 0), (vec![LensProbe::Second], 1), (vec![LensProbe::First], 2), (vec![LensProbe::First, LensProbe::Second, LensProbe::Third], 3)] {
        let mut client = Client { overridden, ..Client::with(K::Command11, &[0x1100]) };
        assert_eq!(
            client.reply(ReplyCommand::Command11, 18, Values::Words(&[0xdd])),
            vec![Seen::Entered(0x1100, 18, vec![0xdd, flags])],
        );
    }
}

#[test]
fn the_preparation_notice_reaches_the_client_only_while_its_flag_is_set() {
    let mut client = Client::with(K::Prepare, &[0x1a00]);
    assert_eq!(client.reply(ReplyCommand::Prepare, 19, Values::Words(&[0b101])), vec![Seen::Entered(0x1a00, 19, vec![0b101])]);
    assert_eq!(client.reply(ReplyCommand::Prepare, 19, Values::Words(&[0b100])), vec![]);
    assert_eq!(
        client.reply(ReplyCommand::Prepare, 0, Values::Words(&[0b100])),
        vec![Seen::Ended(K::Prepare), Seen::Entered(0x1a00, 0, vec![0b100])],
    );
}

#[test]
fn a_record_reaches_the_client_as_a_record() {
    let mut client = Client::with(K::Command09, &[0x900]);
    assert_eq!(client.reply(ReplyCommand::Command09, 17, Values::Record(&[1, 2, 3])), vec![Seen::EnteredRecord(0x900, 17, vec![1, 2, 3])]);
}
