use crate::ports::{DriveDetail, DriveFourth, DrivePolicy, DriveReply, Probe, Probes, Reply, Status};
use crate::{bring_up, live, s1, stock, Outcome};
use xm1_saclac_interface::lens::Setting;
use xm1_saclac_interface::{stock as interface, ClientNotification, CommandKind, Completion as C, ParameterValue as V, ReplyTarget};

const CLEAR: Probes = Probes { bit0: Probe::Clear, bit1: Probe::Clear, bit2: Probe::Clear };

#[test]
fn the_live_row_codes_route_to_their_events() {
    use live::{DriveOrigin, Event, Profile};
    let parameters = interface::parameters(&[]);
    for (code, expected) in [
        (0x02, Event::Profile(Profile::Primary)),
        (0x03, Event::Profile(Profile::Secondary)),
        (0xa3, Event::Setting(Setting::Request04)),
        (0xa4, Event::Setting(Setting::Request06)),
        (0xa6, Event::Setting(Setting::Request05)),
        (0xa5, Event::PrepareDrive),
        (0x0e, Event::Drive(DriveOrigin::Command)),
        (0xa7, Event::Drive(DriveOrigin::Cancel)),
        (0x0f, Event::Cancel),
        (0x10, Event::Stop),
        (0x1b, Event::Step),
        (0x2c, Event::Select),
        (0x2d, Event::Release),
        (0x109, Event::Other),
        (0x33, Event::Other),
    ] {
        assert_eq!(stock::live_event(code, parameters), expected, "{code:#x}");
    }
    for (code, completion) in [(0x34, C::Lens34), (0x35, C::Lens35), (0x36, C::Lens36), (0x37, C::Lens37), (0x38, C::Lens38), (0x39, C::Lens39), (0x3d, C::Lens3d)] {
        assert!(matches!(stock::live_event(code, parameters), Event::Completed(done) if done.completion == completion), "{code:#x}");
    }
}

#[test]
fn the_s1_and_bring_up_row_codes_route_to_their_events() {
    let parameters = interface::parameters(&[]);
    assert_eq!(stock::s1_event(0xa7, parameters), s1::Event::CancelDrive);
    assert_eq!(stock::s1_event(0x0f, parameters), s1::Event::Cancel);
    assert_eq!(stock::s1_event(0x10, parameters), s1::Event::Stop);
    assert_eq!(stock::s1_event(0x0e, parameters), s1::Event::Other);
    assert!(matches!(stock::s1_event(0x33, parameters), s1::Event::Completed(done) if done.completion == C::Lens33));
    assert_eq!(stock::bring_up_event(0x04, parameters), bring_up::Event::BringUp);
    assert_eq!(stock::bring_up_event(0x05, parameters), bring_up::Event::Stop);
    assert_eq!(stock::bring_up_event(0x33, parameters), bring_up::Event::Other);
    for (code, completion) in [(0x2f, C::Lens2f), (0x30, C::Lens30), (0x31, C::Lens31), (0x32, C::Lens32), (0x3f, C::Image3f), (0x40, C::Image40)] {
        assert!(matches!(stock::bring_up_event(code, parameters), bring_up::Event::Completed(done) if done.completion == completion), "{code:#x}");
    }
}

#[test]
fn outcomes_and_statuses_keep_their_stock_words() {
    assert_eq!([Outcome::Fault, Outcome::Complete, Outcome::Pending, Outcome::Inactive].map(stock::outcome_word), [0, 1, 2, 3]);
    assert_eq!(
        [Status::Done, Status::Progress, Status::Accepted, Status::Unneeded, Status::Refused, Status::Lens(7)].map(|status| stock::status(status).0),
        [0, 18, 20, 21, -98, 7]
    );
}

#[test]
fn each_reply_reaches_its_entry_with_the_stock_argument_order() {
    let probes = Probes { bit1: Probe::Forced, bit2: Probe::Forced, ..CLEAR };
    let reply = DriveReply { status: Status::Refused, value: V(5), fourth: DriveFourth::Carried(V(14)), detail: DriveDetail::Lens(probes) };
    let drive = Reply::Drive(reply);
    let call = stock::reply_call(&drive);
    assert_eq!((call.entry, call.arguments, call.count), (0xefb104, [0xffff_ff9e, 5, 1, 6], 4));
    assert_eq!(stock::reply_target(&drive), ReplyTarget::Command(CommandKind::LensDrive));
    let corrected = Reply::Drive(DriveReply { fourth: DriveFourth::One, ..reply });
    assert_eq!(stock::reply_call(&corrected).arguments[2], 0);
    let select = Reply::Select { status: Status::Done, probes, value: V(9) };
    let call = stock::reply_call(&select);
    assert_eq!((call.entry, call.arguments, call.count), (0xefb654, [0, 6, 9, 0], 3));
    let step = Reply::Step { status: Status::Done, probes: Probes { bit0: Probe::Forced, ..CLEAR } };
    assert_eq!(stock::reply_target(&step), ReplyTarget::Notification(ClientNotification::Notification1b));
    assert_eq!((stock::reply_call(&step).entry, stock::reply_call(&step).arguments[1]), (0xefb604, 1));
    let bring_up = Reply::BringUp { status: Status::Lens(3), values: [V(1), V(2), V(4)] };
    assert_eq!(stock::reply_call(&bring_up).arguments, [3, 1, 2, 4]);
    for (reply, entry, target) in [
        (Reply::Cancel(Status::Accepted), 0xefb194, CommandKind::Cancel),
        (Reply::Stop(Status::Accepted), 0xefb1dc, CommandKind::Command10),
        (Reply::Release(Status::Accepted), 0xefb6ac, CommandKind::Command2d),
        (Reply::LensStop { status: Status::Lens(0), value: V(1) }, 0xefa3a8, CommandKind::LensStop),
    ] {
        assert_eq!(stock::reply_call(&reply).entry, entry);
        assert_eq!(stock::reply_target(&reply), ReplyTarget::Command(target));
    }
}

#[test]
fn the_reply_flag_word_ors_the_forced_probes() {
    for (bit0, bit1, bit2, word) in [
        (Probe::Clear, Probe::Clear, Probe::Clear, 0),
        (Probe::Forced, Probe::Clear, Probe::Clear, 1),
        (Probe::Clear, Probe::Forced, Probe::Clear, 2),
        (Probe::Clear, Probe::Clear, Probe::Forced, 4),
        (Probe::Forced, Probe::Forced, Probe::Clear, 3),
        (Probe::Forced, Probe::Clear, Probe::Forced, 5),
        (Probe::Clear, Probe::Forced, Probe::Forced, 6),
        (Probe::Forced, Probe::Forced, Probe::Forced, 7),
        (Probe::Set, Probe::Set, Probe::Set, 0),
    ] {
        assert_eq!(stock::reply_flags(Probes { bit0, bit1, bit2 }), word, "{bit0:?} {bit1:?} {bit2:?}");
    }
}

#[test]
fn the_step_reply_flag_word_assigns_bit_zero_over_bit_one() {
    let both = Probes { bit0: Probe::Forced, bit1: Probe::Forced, ..CLEAR };
    assert_eq!(stock::step_reply_flags(both), 1);
    assert_eq!(stock::reply_flags(both), 3);
    assert_eq!(stock::step_reply_flags(Probes { bit2: Probe::Forced, ..CLEAR }), 4);
    assert_eq!(stock::step_reply_flags(CLEAR), 0);
}

#[test]
fn the_drive_policy_reads_configuration_byte_0x88ef() {
    for (byte, policy) in [
        (0, DrivePolicy { always: false, resource_one: false, resource_zero: false }),
        (8, DrivePolicy { always: true, resource_one: false, resource_zero: false }),
        (2, DrivePolicy { always: false, resource_one: true, resource_zero: false }),
        (4, DrivePolicy { always: false, resource_one: false, resource_zero: true }),
        (0xe, DrivePolicy { always: true, resource_one: true, resource_zero: true }),
    ] {
        assert_eq!(stock::drive_policy(byte), policy, "{byte:#x}");
    }
}
