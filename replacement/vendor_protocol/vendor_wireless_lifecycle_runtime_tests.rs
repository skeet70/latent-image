use super::{RadioAction, RadioPhase, RadioPublication, RadioRequest, RadioRuntime};
use crate::vendor_wireless_adapter::{Adapter, AddressConfiguration, CountryCode, HardwareConfiguration,
    HardwareRevision, PhysicalWrite, RadioCapabilities, regulatory::RegulatoryInput};
use crate::vendor_wireless_lifecycle::{DriverAction, SynchronizationReply};
use crate::vendor_wireless_network::MacAddress;
use crate::vendor_wireless_power::{RadioTick, SdioReply};
use crate::vendor_wireless_radio_program::{RadioChannel, RadioMode, RadioParameterProgram, RadioRate, RadioSelection};

fn runtime<'a>() -> RadioRuntime<'a, 4, 8> {
    let hardware = HardwareConfiguration {
        first_revision: HardwareRevision { major: 1, minor: 0 },
        second_revision: HardwareRevision { major: 1, minor: 0 }, use_calibration: false,
        calibration: [0; 40], feature_requested: false, feature_supported: false, feature_disabled: false,
        country: CountryCode(0), radio_parameters: [0; 144], regulatory: RegulatoryInput::country([0; 2]), antenna: None,
    };
    let address = AddressConfiguration { address: Some(MacAddress([0, 1, 2, 3, 4, 5])),
        preserve_invalid: false, timestamp: 0, capabilities: RadioCapabilities(0) };
    RadioRuntime::new(Adapter::new(), 0, hardware, address)
}

fn program() -> RadioParameterProgram {
    RadioParameterProgram::new(RadioMode::Direct, RadioSelection::Receive,
        RadioChannel::from_abi(1).unwrap(), RadioRate(0), false)
}

#[test]
fn radio_test_publication_waits_for_address_transaction_completion() {
    let mut radio = runtime();
    radio.submit(RadioRequest::Configure(program()), RadioTick(0)).unwrap();
    radio.synchronize(SynchronizationReply::Acquired);
    radio.synchronize(SynchronizationReply::Acquired);
    for (now, reply) in [(0, SdioReply::Written), (500, SdioReply::Read(0x0020_0000u32.to_le_bytes())),
        (500, SdioReply::Read([0; 4])), (500, SdioReply::Written)] {
        let Some(RadioAction::Driver(DriverAction::Register(transfer))) = radio.action(RadioTick(now)) else { panic!() };
        radio.register_reply(transfer.token, reply, RadioTick(now));
    }
    assert!(matches!(radio.action(RadioTick(500)), Some(RadioAction::Driver(DriverAction::Synchronize(_)))));
    radio.synchronize(SynchronizationReply::Released);
    radio.synchronize(SynchronizationReply::Released);
    let Some(RadioAction::Adapter(command)) = radio.action(RadioTick(500)) else { panic!() };
    let identity = command.identity();
    assert_eq!(command.bytes()[4], 0xc1);
    assert_eq!(radio.phase(), RadioPhase::ReinitializingAdapter);
    assert_eq!(radio.pop_publication(), None);
    assert_eq!(radio.outcome(), None);
    radio.adapter_reply(identity, PhysicalWrite::Written);
    radio.action(RadioTick(500));
    assert_eq!(radio.pop_publication(), Some(RadioPublication::RadioTestEntered));
    assert_eq!(radio.phase(), RadioPhase::ConfiguringParameters);
}

#[test]
fn denied_global_lock_does_not_publish_radio_mode() {
    let mut radio = runtime();
    radio.submit(RadioRequest::Configure(program()), RadioTick(0)).unwrap();
    radio.synchronize(SynchronizationReply::TimedOut);
    radio.action(RadioTick(0));
    assert_eq!(radio.pop_publication(), None);
    assert_eq!(radio.phase(), RadioPhase::Finished);
}
