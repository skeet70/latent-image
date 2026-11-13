use crate::children::{AcquisitionControl, ChildEvent, ChildRequest, LensControl, LensEffect, LensSetting, MeasurementControl, ShotControl, ShotEvent, ShotTarget};
use crate::search::SearchInput;
use xm1_saclac_interface::{stock, Command, CommandKind, Completed, Completion, Message};

#[test]
fn search_and_lens_reachable_child_requests_are_emulator_only() {
    let parameters = stock::parameters(&[]);
    let record = stock::record_handle_from_a2(0x1234);
    let requests = [
        ChildRequest::<()>::Measurement(ChildEvent::Control { control: MeasurementControl::Initialize, parameters, record }),
        ChildRequest::S1Search(SearchInput::Initialize),
        ChildRequest::S1Search(SearchInput::Begin),
        ChildRequest::S1Search(SearchInput::Cancel),
        ChildRequest::LensBringUp { message: Message::Command(Command { kind: CommandKind::LensBringUp, parameters }), record },
        ChildRequest::LensLive(ChildEvent::Control { control: LensControl::CancelDrive, parameters, record }),
        ChildRequest::LensS1(ChildEvent::Control { control: LensControl::Setting(LensSetting::Request04), parameters, record }),
        ChildRequest::Shot(ShotEvent::Control { control: ShotControl::StartS1, record }),
        ChildRequest::Shot(ShotEvent::Command { target: ShotTarget::Live, command: Command { kind: CommandKind::Command02, parameters }, record }),
        ChildRequest::Shot(ShotEvent::Completed { target: ShotTarget::Capture, completed: Completed { completion: Completion::Image5e, parameters }, record }),
    ];
    for request in requests {
        assert_eq!(request.lens_effect(), LensEffect::LensReaching);
    }
}

#[test]
fn acquisition_without_search_is_not_a_lens_request() {
    let request = ChildRequest::<()>::S1Acquisition(ChildEvent::Control {
        control: AcquisitionControl::Initialize,
        parameters: stock::parameters(&[]),
        record: stock::record_handle_from_a2(0x5678),
    });
    assert_eq!(request.lens_effect(), LensEffect::MeasurementOnly);
}
