use super::state::{helper_ee8ba0, Channel, Position, TimingCallbackArguments, TimingValue};
use crate::record_address_space::{CallbackKey, RecordContext};

#[test]
fn callback_effect_is_visible_before_invocation_returns() {
    fn complete(context: &mut RecordContext<(), u32>) -> Result<(), core::convert::Infallible> {
        context.effects = 7;
        Ok(())
    }
    let mut context = RecordContext::new((), 0);
    assert_eq!(helper_ee8ba0(&mut context, complete), Ok(()));
    assert_eq!(context.effects, 7);
}

#[test]
fn publication_encodes_words_and_halfwords_without_changing_padding() {
    let arguments = TimingCallbackArguments {
        parameter: TimingValue(0x01020304), aperture: TimingValue(0x05060708), target: TimingValue(0x11121314),
        timing: TimingValue(0x15161718), flags: 0x21222324, status: 0x25262728,
        low: Position(-1), high: Position(2), step: Position(-3), saved: Position(4), geometry: Channel(0x31323334),
    };
    let mut payload = [0xa5; 48];
    arguments.store(CallbackKey::new(0x41424344).unwrap(), &mut payload);
    assert_eq!(payload, [
        0x44, 0x43, 0x42, 0x41, 4, 3, 2, 1, 8, 7, 0xa5, 0xa5, 0x14, 0x13, 0x12, 0x11,
        0x18, 0x17, 0xa5, 0xa5, 0x24, 0x23, 0x22, 0x21, 0x28, 0x27, 0x26, 0x25, 0, 0, 0, 0,
        1, 0, 0, 0, 0xff, 0xff, 2, 0, 0xfd, 0xff, 4, 0, 0x34, 0x33, 0x32, 0x31,
    ]);
}
