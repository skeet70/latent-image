use crate::layers::{Block, Change, CommandParameters, DriveProgress, Field, LensControl, LensDriveReplies, LensProbe, ProfileRecord};
use crate::live::Profile;
use crate::live_replies::*;
use crate::replies::{Replies, ReplyCommand as R, Values};
use std::{vec, vec::Vec};
use xm1_saclac_interface::{stock, CommandKind as K, CompletionStatus, ParameterValue, RecordHandle};

#[derive(Debug, Eq, PartialEq)]
enum Seen {
    Reply(R, i32, Vec<u32>),
    Progress(DriveProgress),
}

#[derive(Default)]
struct Fake {
    overridden: Vec<LensProbe>,
    drive_fourth: u32,
    seen: Vec<Seen>,
}

const DESCRIPTOR: u32 = 0x0d;
const LENS_VALUE: u32 = 0x80;
const NOTICE: u32 = 0x31;

fn record() -> RecordHandle { stock::record_handle_from_a2(0x16a0_0490) }

impl ProfileRecord for Fake {
    fn read(&mut self, record: RecordHandle, field: Field) -> u32 {
        assert_eq!((record, field), (self::record(), Field::Descriptor));
        DESCRIPTOR
    }
    fn write(&mut self, _: RecordHandle, _: Field, _: u32) { panic!("the router's replies write no field") }
    fn changed(&mut self, _: RecordHandle, _: Change) -> bool { panic!("the router's replies read no change") }
    fn read_block(&mut self, _: RecordHandle, _: Block, _: &mut [u32]) { panic!("the router's replies read no block") }
    fn write_block(&mut self, _: RecordHandle, _: Block, _: &[u32]) { panic!("the router's replies write no block") }
    fn clear_selection(&mut self, _: RecordHandle) { panic!("the router's replies clear no selection") }
    fn select_descriptor(&mut self, _: RecordHandle, _: u32) { panic!("the router's replies select no descriptor") }
    fn set_descriptor_pair(&mut self, _: RecordHandle, _: u32, _: u32) { panic!("the router's replies set no pair") }
    fn notify_selection_owner(&mut self, _: RecordHandle) { panic!("the router's replies notify no owner") }
}

impl LensControl for Fake {
    fn overridden(&mut self, probe: LensProbe) -> bool { self.overridden.contains(&probe) }
    fn value(&mut self) -> u32 { LENS_VALUE }
    fn position(&mut self) -> u32 { panic!("the router's replies read no lens position") }
    fn mode_index(&mut self) -> u32 { panic!("the router's replies read no mode index") }
    fn reference(&mut self) -> u32 { panic!("the router's replies read no lens reference") }
    fn step(&mut self) -> u32 { panic!("the router's replies read no lens step") }
    fn channel(&mut self) -> (u32, u16) { panic!("the router's replies read no lens channel") }
    fn third_notice(&mut self) -> u32 { NOTICE }
}

impl CommandParameters for Fake {
    fn word(&mut self, command: K, index: usize) -> u32 {
        assert_eq!((command, index), (K::LensDrive, 4));
        self.drive_fourth
    }
}

impl Replies for Fake {
    fn reply(&mut self, command: R, status: CompletionStatus, values: Values<'_>) {
        let Values::Words(words) = values else { panic!("the profile replies carry words") };
        self.seen.push(Seen::Reply(command, status.0, words.to_vec()));
    }
}

impl LensDriveReplies for Fake {
    fn report_progress(&mut self, progress: DriveProgress) { self.seen.push(Seen::Progress(progress)) }
}

impl Layers for Fake {
    type Profile = Self;
    type Lens = Self;
    type Parameters = Self;
    type Replies = Self;
    type LensDrive = Self;
    fn profile(&mut self) -> &mut Self { self }
    fn lens(&mut self) -> &mut Self { self }
    fn parameters(&mut self) -> &mut Self { self }
    fn replies(&mut self) -> &mut Self { self }
    fn lens_drive(&mut self) -> &mut Self { self }
}

#[test]
fn each_profile_is_answered_through_its_command_with_the_lens_state() {
    for (profile, command) in [(Profile::Primary, R::Command02), (Profile::Secondary, R::Command03)] {
        let mut fake = Fake::default();
        reply_profile(profile, record(), &mut fake);
        assert_eq!(fake.seen, vec![Seen::Reply(command, 0, vec![DESCRIPTOR, LENS_VALUE, NOTICE, 0])]);
    }
}

#[test]
fn the_profile_reply_carries_a_bit_for_each_overridden_probe() {
    for (overridden, bits) in [
        (vec![LensProbe::First], 2),
        (vec![LensProbe::Second], 1),
        (vec![LensProbe::Third], 4),
        (vec![LensProbe::First, LensProbe::Second, LensProbe::Third], 7),
    ] {
        let mut fake = Fake { overridden, ..Fake::default() };
        reply_profile(Profile::Primary, record(), &mut fake);
        assert_eq!(fake.seen, vec![Seen::Reply(R::Command02, 0, vec![DESCRIPTOR, LENS_VALUE, NOTICE, bits])]);
    }
}

#[test]
fn drive_progress_reports_the_completion_words_and_whether_the_fourth_word_is_one() {
    let completion: Vec<u8> = [0x11u32, 0xfffe, 0x22, 0x5678].iter().flat_map(|word| word.to_le_bytes()).collect();
    for (fourth, carried) in [(1, false), (0, true), (7, true)] {
        let mut fake = Fake { drive_fourth: fourth, ..Fake::default() };
        report_drive_progress(stock::parameters(&completion), &mut fake);
        assert_eq!(
            fake.seen,
            vec![Seen::Progress(DriveProgress { channel: 0xfffe, fourth_carried: carried, reported: ParameterValue(0x5678) })],
        );
    }
}
