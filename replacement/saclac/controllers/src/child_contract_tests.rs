use crate::children::{AcquisitionControl, ChildEvent, ChildRequest, LensControl, LensEffect, MeasurementControl, ShotControl};
use crate::search::SearchInput;
use xm1_saclac_interface::{stock, Command, CommandKind, Message};

#[test]
fn search_and_lens_reachable_child_requests_are_emulator_only() {
    let parameters = stock::parameters(&[]);
    let requests = [
        ChildRequest::<()>::Measurement(ChildEvent::Control { control: MeasurementControl::Initialize, parameters }),
        ChildRequest::S1Search(SearchInput::Initialize),
        ChildRequest::S1Search(SearchInput::Begin),
        ChildRequest::S1Search(SearchInput::Cancel),
        ChildRequest::LensBringUp(Message::Command(Command { kind: CommandKind::LensBringUp, parameters })),
        ChildRequest::LensLive(ChildEvent::Control { control: LensControl::Initialize, parameters }),
        ChildRequest::LensS1(ChildEvent::Control { control: LensControl::S1Release, parameters }),
        ChildRequest::Shot(ChildEvent::Control { control: ShotControl::StartS1, parameters }),
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
    });
    assert_eq!(request.lens_effect(), LensEffect::MeasurementOnly);
}
