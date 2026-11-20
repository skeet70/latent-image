#![forbid(unsafe_code)]

use core::cell::{Cell, RefCell};
use xm1_focus_measurement_round::children::S1Acquisition;
use xm1_saclac_calc::scene_recognition::SceneRecognition;
use xm1_saclac_controllers::{children::{ChildOutcome, ChildRequest, Children, ShotEvent}, search::SearchTerminal};
use xm1_saclac_focus_search::s1::S1Search;
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{Calc, Calc1, ImageProcessor, ImgDc, LensDc};
use xm1_saclac_lens_drive::children::LensChildren;
use xm1_saclac_profile_geometry::controller::{ProfileGeometry, Tables};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_search_bindings::RecordBinding;
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{shot_ports::{ExternalPorts, ShotPorts}, state::StateMemory, Shot};
use crate::{geometry::GeometryBindings, lens_ports::LensPorts, measurement_runtime::MeasurementRuntime, search_runtime::SearchSession, services::Services, shot_services::BindRecord, stock::geometry::Memory as GeometryMemory};

type Geometry<M, P, I, T> = ProfileGeometry<GeometryMemory<M>, GeometryBindings<M, P, I>, T>;

/// All child engines survive queue dispatch. The ShotPorts owner is shared with profile views.
pub struct ChildRuntime<'s, M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, S, X> {
    services: &'s RefCell<Services<M, R, E, P>>,
    measurement: RefCell<MeasurementRuntime<'s, M, R, E, P, I, T, S, X>>,
    acquisition: RefCell<S1Acquisition>,
    search: RefCell<S1Search>,
    session: SearchSession<'s, M, R, E, P>,
    lens: RefCell<LensChildren>,
    shot: RefCell<Shot>,
    shot_ports: &'s RefCell<ShotPorts<S, X>>,
    terminal: Cell<Option<SearchTerminal>>,
}
impl<'s, M: Machine + Clone, R: Memory, E: StockEntries,
    P: ImageProcessor + Calc + Calc1 + ImgDc + LensDc + xm1_saclac_interface::Statistics, I: Isp, T: Tables, S, X>
    ChildRuntime<'s, M, R, E, P, I, T, S, X> {
    pub fn new(services: &'s RefCell<Services<M, R, E, P>>, geometry: &'s RefCell<Geometry<M, P, I, T>>, recognition: &'s RefCell<SceneRecognition>, live_record: &'s RecordBinding, s1_record: &'s RecordBinding, shot_ports: &'s RefCell<ShotPorts<S, X>>) -> Self {
        Self {
            services,
            measurement: RefCell::new(MeasurementRuntime::new(services, geometry, recognition, live_record, shot_ports)),
            acquisition: RefCell::new(S1Acquisition::new()),
            search: RefCell::new(S1Search::new()),
            session: SearchSession::new(services, s1_record),
            lens: RefCell::new(LensChildren::new()),
            shot: RefCell::new(Shot::new()),
            shot_ports,
            terminal: Cell::new(None),
        }
    }
    pub fn take_terminal(&self) -> Option<SearchTerminal> { self.terminal.take() }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries,
    P: ImageProcessor + Calc + Calc1 + ImgDc + LensDc + xm1_saclac_interface::Statistics, I: Isp, T: Tables,
    S: StateMemory, X: ExternalPorts + BindRecord> ChildRuntime<'_, M, R, E, P, I, T, S, X> {
    pub fn dispatch(&self, request: ChildRequest<'_>) -> ChildOutcome {
        match request {
            ChildRequest::Measurement(event) => {
                let dispatch = self.measurement.borrow_mut().handle(event);
                if let Some(terminal) = dispatch.terminal {
                    assert!(self.terminal.replace(Some(terminal)).is_none(), "search terminal must be consumed before another finishes");
                }
                dispatch.outcome
            }
            ChildRequest::S1Acquisition(event) => self.measurement.borrow().handle_s1_acquisition(&mut self.acquisition.borrow_mut(), event),
            ChildRequest::S1Search(event) => self.session.handle_s1(&mut self.search.borrow_mut(), event),
            ChildRequest::LensBringUp { message, record } => self.lens.borrow_mut().bring_up(message, record, &mut LensPorts(self.services)),
            ChildRequest::LensLive(event) => self.lens.borrow_mut().live(event, &mut LensPorts(self.services)),
            ChildRequest::LensS1(event) => self.lens.borrow_mut().s1(event, &mut LensPorts(self.services)),
            ChildRequest::Shot(event) => {
                let record = match event {
                    ShotEvent::Control { record, .. } | ShotEvent::Command { record, .. } | ShotEvent::Completed { record, .. } => record,
                };
                let mut ports = self.shot_ports.borrow_mut();
                ports.external().bind_record(record);
                match self.shot.borrow_mut().handle(event, &mut *ports) {
                    xm1_shot_sequence::Outcome::Complete => ChildOutcome::Complete,
                    xm1_shot_sequence::Outcome::Pending => ChildOutcome::Pending,
                    xm1_shot_sequence::Outcome::Unhandled => ChildOutcome::Inactive,
                    // Root's callers admit only native 1/2/3. They own the fault reaction for 0/21.
                    xm1_shot_sequence::Outcome::Fault | xm1_shot_sequence::Outcome::Aborted | xm1_shot_sequence::Outcome::Progress => ChildOutcome::Fault,
                }
            },
        }
    }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries,
    P: ImageProcessor + Calc + Calc1 + ImgDc + LensDc + xm1_saclac_interface::Statistics, I: Isp, T: Tables,
    S: StateMemory, X: ExternalPorts + BindRecord> Children for &ChildRuntime<'_, M, R, E, P, I, T, S, X> {
    fn handle(&mut self, request: ChildRequest<'_>) -> ChildOutcome { self.dispatch(request) }
}

#[cfg(saclac_qualification)]
#[allow(unsafe_code)]
pub mod qualification;
