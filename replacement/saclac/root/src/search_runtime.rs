#![forbid(unsafe_code)]

//! A persistent search lane borrows services only while advancing its strategy or posting a
//! request. Acquisition can use those same services between search events.

use core::cell::RefCell;
use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, SearchControl};
use xm1_saclac_controllers::search::{FocusSearch, SearchDecision, SearchInput, SearchTerminal};
use xm1_saclac_focus_search::contrast::{BridgeDrive, Context, ContrastSearch};
use xm1_saclac_focus_search::live::{self, LiveStrategy, Measurement};
use xm1_saclac_focus_search::requests::Request;
use xm1_saclac_focus_search::s1::S1Search;
use xm1_saclac_interface::search::{BridgeReport, Evaluation, SearchRecord};
use xm1_saclac_interface::{Calc, ImgDc};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_search_bindings::{RecordBinding, Session};
use xm1_saclac_shared::stock::Machine;

use crate::services::Services;

pub struct SearchSession<'s, M, R, E, P> {
    services: &'s RefCell<Services<M, R, E, P>>,
    record: &'s RecordBinding,
}

impl<M, R, E, P> Copy for SearchSession<'_, M, R, E, P> {}
impl<M, R, E, P> Clone for SearchSession<'_, M, R, E, P> {
    fn clone(&self) -> Self { *self }
}

impl<'s, M: Machine + Clone, R: Memory, E: StockEntries, P> SearchSession<'s, M, R, E, P> {
    pub const fn new(services: &'s RefCell<Services<M, R, E, P>>, record: &'s RecordBinding) -> Self {
        Self { services, record }
    }

    pub fn set_record(&self, record: xm1_saclac_interface::RecordHandle) { self.record.set(record); }

    pub fn record(&self) -> xm1_saclac_interface::RecordHandle { self.record.get() }

    pub fn set_history(&self, history: &xm1_focus_measurement_round::history::MeasurementHistory, kept: bool) {
        self.services.borrow_mut().search_bindings().set_history(history, kept);
    }

    pub fn live_completion(&self, completed: xm1_saclac_interface::Completed<'_>) -> Option<live::LaneEvent<Evaluation>> {
        self.with_session(|session| session.live_completion(completed))
    }

    fn with_session<T>(&self, operation: impl FnOnce(Session<'_, '_, M, R, E, P>) -> T) -> T {
        let mut services = self.services.borrow_mut();
        let bindings = RefCell::new(services.search_bindings());
        operation(Session::new(&bindings, self.record))
    }

    fn with_strategy<T>(&self, operation: impl FnOnce(&mut ContrastSearch<Session<'_, '_, M, R, E, P>>) -> T) -> T {
        self.with_session(|session| operation(&mut ContrastSearch::new(session)))
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> FocusSearch for SearchSession<'_, M, R, E, P> {
    type Context = Context;
    type Sample = Measurement<Evaluation>;
    type Drive = BridgeDrive;

    fn advance(&mut self, context: &mut Context, input: SearchInput<Self::Sample>) -> SearchDecision<BridgeDrive> {
        self.with_strategy(|strategy| strategy.advance(context, input))
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> LiveStrategy for SearchSession<'_, M, R, E, P> {
    type Evaluation = Evaluation;
    type Record = SearchRecord;

    fn ready(&mut self, context: &Context, bridge_busy: bool) -> bool {
        self.with_strategy(|strategy| strategy.ready(context, bridge_busy))
    }
    fn take_in(&mut self, context: &mut Context) -> SearchRecord {
        self.with_strategy(|strategy| strategy.take_in(context))
    }
    fn lens_reported(&mut self, context: &mut Context, report: BridgeReport) {
        self.with_strategy(|strategy| strategy.lens_reported(context, report));
    }
    fn release(&mut self, context: &Context) -> BridgeDrive {
        self.with_strategy(|strategy| strategy.release(context))
    }
    fn accepts_begin(&self, context: &Context) -> bool {
        self.with_strategy(|strategy| strategy.accepts_begin(context))
    }
    fn finished(&mut self, context: &mut Context, outcome: SearchTerminal) {
        self.with_strategy(|strategy| strategy.finished(context, outcome));
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: Calc + ImgDc> live::Ports<SearchRecord, BridgeDrive>
    for SearchSession<'_, M, R, E, P>
{
    fn fault(&mut self) {
        self.with_session(|mut session| live::Ports::fault(&mut session));
    }
    fn request(&mut self, request: Request<SearchRecord, BridgeDrive>) {
        self.with_session(|mut session| live::Ports::request(&mut session, request));
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: Calc + ImgDc> SearchSession<'_, M, R, E, P> {
    pub fn handle_live(&self, lane: &mut live::LiveSearch<Self>, event: live::LaneEvent<Evaluation>) -> live::LaneStatus {
        let mut ports = *self;
        lane.handle(event, &mut ports)
    }

    pub fn handle_s1(&self, lane: &mut S1Search, event: ChildEvent<'_, SearchControl>) -> ChildOutcome {
        self.with_session(|mut session| lane.handle(event, &mut session))
    }
}
