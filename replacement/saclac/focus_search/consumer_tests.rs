use xm1_saclac_controllers::search::{FocusSearch, LensPosition, SearchDecision, SearchInput, SearchStep, SearchTerminal};
use xm1_saclac_focus_search::live::{LaneEvent, LaneStatus, LiveSearch, LiveStrategy, Measurement, Ports};
use xm1_saclac_focus_search::requests::Request;
use xm1_saclac_interface::search::BridgeReport;

struct Candidate(SearchTerminal);
#[derive(Default)]
struct Context {
    outcome: Option<SearchTerminal>,
    notifications: usize,
    resumed: bool,
}
impl FocusSearch for Candidate {
    type Context = Context;
    type Sample = Measurement<()>;
    type Drive = ();
    fn advance(&mut self, context: &mut Context, input: SearchInput<Self::Sample>) -> SearchDecision<()> {
        match input {
            SearchInput::Initialize | SearchInput::Begin => SearchDecision::Terminal(self.0),
            SearchInput::Resume => {
                context.resumed = true;
                SearchDecision::Next(SearchStep::Measure)
            }
            SearchInput::Sample(_) | SearchInput::Suspend | SearchInput::ResumePending | SearchInput::Cancel => SearchDecision::Next(SearchStep::Measure),
        }
    }
}
impl LiveStrategy for Candidate {
    type Evaluation = ();
    type Record = ();
    fn ready(&mut self, _: &Context, bridge_busy: bool) -> bool { !bridge_busy }
    fn take_in(&mut self, _: &mut Context) {}
    fn lens_reported(&mut self, _: &mut Context, _: BridgeReport) {}
    fn release(&mut self, _: &Context) {}
    fn accepts_begin(&self, _: &Context) -> bool { false }
    fn finished(&mut self, context: &mut Context, outcome: SearchTerminal) {
        context.outcome = Some(outcome);
        context.notifications += 1;
    }
}
#[derive(Default)]
struct Peers(Vec<Request<(), ()>>);
impl Ports<(), ()> for Peers {
    fn fault(&mut self) { panic!("unexpected lane fault"); }
    fn request(&mut self, request: Request<(), ()>) { self.0.push(request); }
}

#[test]
fn external_candidate_receives_terminal_notification_and_the_caller_keeps_its_result() {
    for outcome in [SearchTerminal::Focused(LensPosition(-12)), SearchTerminal::Failed, SearchTerminal::Cancelled] {
        for start in [LaneEvent::Enable, LaneEvent::Initialize] {
            let mut lane = LiveSearch::new(Candidate(outcome), Context::default());
            let mut peers = Peers::default();
            assert_eq!(lane.handle(start, &mut peers), LaneStatus::Terminal(outcome));
            assert_eq!(lane.context().outcome, Some(outcome));
            assert_eq!(lane.handle(LaneEvent::Work, &mut peers), LaneStatus::Terminal(outcome));
            assert_eq!(lane.handle(LaneEvent::Suspend, &mut peers), LaneStatus::Complete);
            assert!(peers.0.is_empty());
            assert_eq!(lane.handle(LaneEvent::Resume, &mut peers), LaneStatus::Complete);
            assert!(lane.context().resumed);
            assert_eq!(lane.context().notifications, 1);
            assert_eq!(lane.handle(LaneEvent::Work, &mut peers), LaneStatus::Pending);
            assert_eq!(peers.0, [Request::Evaluate(())]);
        }
    }
}
