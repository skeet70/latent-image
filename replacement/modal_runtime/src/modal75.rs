//! A cell named by the build-time table runs its owned action while the live
//! cell word, action target and, for a positive high halfword, transition
//! entry still equal the table's; every other cell runs the stock action.
//!
//! The owned path reads the full cell word and the selected transition entry
//! before the action; stock reads neither then. The modal guard watches
//! writes and fetches only.
//!
//! Premise `transition-router-register-inputs` (E-MODAL75-TRANSITION-PREMISE-150):
//! `0xc12aac` and its callees read no incoming r0-r3 or r11 beyond their
//! declared arguments.

#![cfg_attr(not(test), allow(dead_code))]

pub const WIDTH: u16 = 46;
pub const EVENTS: u16 = 51;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event75(u8);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct State75(u8);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UnhandledInput(pub u16);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UnknownState(pub u16);

impl Event75 {
    pub const fn parse(raw: u16) -> Result<Self, UnhandledInput> {
        if raw < EVENTS { Ok(Self(raw as u8)) } else { Err(UnhandledInput(raw)) }
    }

    pub const fn raw(self) -> u16 {
        self.0 as u16
    }
}

impl State75 {
    pub const fn parse(raw: u16) -> Result<Self, UnknownState> {
        if raw < WIDTH { Ok(Self(raw as u8)) } else { Err(UnknownState(raw)) }
    }

    pub const fn raw(self) -> u16 {
        self.0 as u16
    }
}

/// Stock returns the action's r0 unchanged as the dispatcher result.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ActionResult(pub u32);

/// Next state selected by the transition router's result; results not listed
/// leave the state unchanged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Route {
    pairs: [(u32, State75); 4],
    len: u8,
}

impl Route {
    pub const fn new(pairs: [(u32, State75); 4], len: u8) -> Self {
        Self { pairs, len }
    }

    pub fn next_state(&self, result: u32) -> Option<State75> {
        self.pairs.iter().take(usize::from(self.len)).find(|(value, _)| *value == result).map(|(_, state)| *state)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenericAction {
    /// Calls transition router `0xc12aac` and returns its result.
    RouteToTransition(Route),
    ConstantResult(ActionResult),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellAction {
    Retained,
    Generic(u8),
}

/// `SetState` is the store of the negated non-positive high halfword.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AfterAction {
    KeepActionState,
    SetState(State75),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OwnedCell {
    pub event: Event75,
    pub state: State75,
    pub cell_word: u32,
    pub stock_target: u32,
    pub action: CellAction,
    pub after: AfterAction,
}

pub struct Table75 {
    pub actions: &'static [GenericAction],
    pub cells: &'static [OwnedCell],
}

/// The owned table, generated from owner-supplied image5 bytes by
/// `//replacement/modal_runtime:controller75-table`.
#[cfg(any(rb_modal75_owned, rb_modal75_vectors))]
pub static TABLE: Table75 = include!(env!("RB_MODAL75_TABLE"));

/// Live words read before the action. `transition` is the entry the positive
/// high halfword selects, or `None` when the high halfword is not positive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LiveCell {
    pub word: u32,
    pub target: u32,
    pub transition: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Plan {
    Stock,
    Owned { action: OwnedAction, after: AfterAction, cell_word: u32 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OwnedAction {
    Retained,
    Generic(&'static GenericAction),
}

pub fn plan(table: &Table75, event: u16, state: u16, live: LiveCell) -> Plan {
    let (Ok(event), Ok(state)) = (Event75::parse(event), State75::parse(state)) else {
        return Plan::Stock;
    };
    let Some(cell) = table.cells.iter().find(|cell| cell.event == event && cell.state == state) else {
        return Plan::Stock;
    };
    if cell.cell_word != live.word || cell.stock_target != live.target {
        return Plan::Stock;
    }
    if cell.after == AfterAction::KeepActionState && live.transition != Some(0) {
        return Plan::Stock;
    }
    let actions: &'static [GenericAction] = table.actions;
    let action = match cell.action {
        CellAction::Retained => OwnedAction::Retained,
        CellAction::Generic(index) => match actions.get(usize::from(index)) {
            Some(action) => OwnedAction::Generic(action),
            None => return Plan::Stock,
        },
    };
    Plan::Owned { action, after: cell.after, cell_word: cell.cell_word }
}

pub trait GenericServices {
    fn transition(&mut self) -> u32;
    fn write_state(&mut self, state: State75);
}

pub fn run_generic(action: &GenericAction, services: &mut impl GenericServices) -> ActionResult {
    match action {
        GenericAction::RouteToTransition(route) => {
            let result = services.transition();
            if let Some(next) = route.next_state(result) {
                services.write_state(next);
            }
            ActionResult(result)
        }
        GenericAction::ConstantResult(result) => *result,
    }
}

/// The state halfword stored after the action, from the high halfword read
/// after it; `None` when it is positive.
pub fn post_action_state(plan: Plan, high_after: i16) -> Option<u16> {
    if high_after > 0 {
        return None;
    }
    match plan {
        Plan::Owned { after: AfterAction::SetState(state), cell_word, .. } if (cell_word >> 16) as u16 as i16 == high_after => {
            Some(state.raw())
        }
        _ => Some(high_after.wrapping_neg() as u16),
    }
}

/// Stock behavior of one owned cell for one transition-router result, recorded
/// by the table generator. `result` is `None` for a retained action.
#[derive(Clone, Copy, Debug)]
pub struct StockVector {
    pub event: u16,
    pub state: u16,
    pub live: LiveCell,
    pub router: u32,
    pub result: Option<u32>,
    pub states: &'static [u16],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VectorMismatch {
    StockBodyFetch { event: u16, state: u16 },
    NotOwned { event: u16, state: u16 },
    Result { event: u16, state: u16, router: u32 },
    States { event: u16, state: u16, router: u32 },
}

struct VectorServices<'a> {
    router: u32,
    states: &'a mut [u16; 4],
    count: usize,
}

impl GenericServices for VectorServices<'_> {
    fn transition(&mut self) -> u32 {
        self.router
    }

    fn write_state(&mut self, state: State75) {
        self.record(state.raw());
    }
}

impl VectorServices<'_> {
    fn record(&mut self, value: u16) {
        if self.count < self.states.len() {
            self.states[self.count] = value;
        }
        self.count += 1;
    }
}

/// Runs `plan`, `run_generic` and `post_action_state` on every vector and
/// returns the first difference from stock.
pub fn check_vectors(table: &Table75, vectors: &[StockVector]) -> Result<(), VectorMismatch> {
    for vector in vectors {
        let (event, state, router) = (vector.event, vector.state, vector.router);
        let planned = plan(table, event, state, vector.live);
        let mut states = [0u16; 4];
        let mut services = VectorServices { router, states: &mut states, count: 0 };
        match (vector.result, planned) {
            (Some(expected), Plan::Owned { action: OwnedAction::Generic(action), .. }) => {
                if run_generic(action, &mut services).0 != expected {
                    return Err(VectorMismatch::Result { event, state, router });
                }
            }
            (Some(_), _) => return Err(VectorMismatch::StockBodyFetch { event, state }),
            (None, Plan::Owned { action: OwnedAction::Retained, .. }) => {}
            (None, _) => return Err(VectorMismatch::NotOwned { event, state }),
        }
        if let Some(value) = post_action_state(planned, (vector.live.word >> 16) as u16 as i16) {
            services.record(value);
        }
        let count = services.count;
        if count != vector.states.len() || states[..count.min(4)] != *vector.states {
            return Err(VectorMismatch::States { event, state, router });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn state(raw: u8) -> State75 {
        State75(raw)
    }

    const ROUTE: Route = Route::new([(64, state(0)), (49, state(22)), (0, state(0)), (0, state(0))], 2);

    static ACTIONS: [GenericAction; 2] =
        [GenericAction::RouteToTransition(ROUTE), GenericAction::ConstantResult(ActionResult(0xffff_ffff))];

    static CELLS: [OwnedCell; 3] = [
        OwnedCell { event: Event75(50), state: State75(5), cell_word: 0x0001_0110, stock_target: 0x00c7_a978, action: CellAction::Generic(0), after: AfterAction::KeepActionState },
        OwnedCell { event: Event75(41), state: State75(0), cell_word: 0xffe3_00f7, stock_target: 0x00c7_9a40, action: CellAction::Generic(1), after: AfterAction::SetState(State75(29)) },
        OwnedCell { event: Event75(4), state: State75(13), cell_word: 0x0000_0030, stock_target: 0x00c6_f994, action: CellAction::Retained, after: AfterAction::SetState(State75(0)) },
    ];

    static TABLE: Table75 = Table75 { actions: &ACTIONS, cells: &CELLS };

    const ROUTED: LiveCell = LiveCell { word: 0x0001_0110, target: 0x00c7_a978, transition: Some(0) };
    const CONSTANT: LiveCell = LiveCell { word: 0xffe3_00f7, target: 0x00c7_9a40, transition: None };
    const RETAINED: LiveCell = LiveCell { word: 0x0000_0030, target: 0x00c6_f994, transition: None };

    /// Stock outcomes of the three synthetic cells, written from their bodies.
    static VECTORS: [StockVector; 5] = [
        StockVector { event: 50, state: 5, live: ROUTED, router: 64, result: Some(64), states: &[0] },
        StockVector { event: 50, state: 5, live: ROUTED, router: 49, result: Some(49), states: &[22] },
        StockVector { event: 50, state: 5, live: ROUTED, router: 7, result: Some(7), states: &[] },
        StockVector { event: 41, state: 0, live: CONSTANT, router: 0, result: Some(0xffff_ffff), states: &[29] },
        StockVector { event: 4, state: 13, live: RETAINED, router: 0, result: None, states: &[0] },
    ];

    fn leak<T>(items: Vec<T>) -> &'static [T] {
        Vec::leak(items)
    }

    #[test]
    fn event_and_state_domains() {
        assert_eq!(Event75::parse(50).map(Event75::raw), Ok(50));
        assert_eq!(Event75::parse(51), Err(UnhandledInput(51)));
        assert_eq!(State75::parse(45).map(State75::raw), Ok(45));
        assert_eq!(State75::parse(46), Err(UnknownState(46)));
        assert_eq!(plan(&TABLE, 51, 5, ROUTED), Plan::Stock);
        assert_eq!(plan(&TABLE, 50, 46, ROUTED), Plan::Stock);
    }

    #[test]
    fn synthetic_vectors_match() {
        assert_eq!(check_vectors(&TABLE, &VECTORS), Ok(()));
    }

    #[test]
    fn changed_live_words_fall_back_to_stock() {
        assert_eq!(plan(&TABLE, 50, 5, LiveCell { word: 0x0001_0111, ..ROUTED }), Plan::Stock);
        assert_eq!(plan(&TABLE, 50, 5, LiveCell { target: 0x00a8_6144, ..ROUTED }), Plan::Stock);
        assert_eq!(plan(&TABLE, 50, 5, LiveCell { transition: Some(0x00c0_0000), ..ROUTED }), Plan::Stock);
        assert_eq!(plan(&TABLE, 49, 5, ROUTED), Plan::Stock);
    }

    #[test]
    fn post_action_state_follows_the_live_high_halfword() {
        let planned = plan(&TABLE, 41, 0, CONSTANT);
        assert_eq!(post_action_state(planned, -29), Some(29));
        assert_eq!(post_action_state(planned, -3), Some(3));
        assert_eq!(post_action_state(planned, 1), None);
        assert_eq!(post_action_state(Plan::Stock, 0), Some(0));
    }

    #[test]
    fn control_wrong_next_state_refuses() {
        let mut actions = ACTIONS.to_vec();
        actions[0] = GenericAction::RouteToTransition(Route::new([(64, state(0)), (49, state(23)), (0, state(0)), (0, state(0))], 2));
        let wrong = Table75 { actions: leak(actions), cells: &CELLS };
        assert_eq!(check_vectors(&wrong, &VECTORS), Err(VectorMismatch::States { event: 50, state: 5, router: 49 }));
    }

    #[test]
    fn control_wrong_direct_state_refuses() {
        let mut cells = CELLS.to_vec();
        cells[2].after = AfterAction::SetState(state(1));
        let wrong = Table75 { actions: &ACTIONS, cells: leak(cells) };
        assert_eq!(check_vectors(&wrong, &VECTORS), Err(VectorMismatch::States { event: 4, state: 13, router: 0 }));
    }

    #[test]
    fn control_wrong_action_result_refuses() {
        let mut actions = ACTIONS.to_vec();
        actions[1] = GenericAction::ConstantResult(ActionResult(0));
        let wrong = Table75 { actions: leak(actions), cells: &CELLS };
        assert_eq!(check_vectors(&wrong, &VECTORS), Err(VectorMismatch::Result { event: 41, state: 0, router: 0 }));
    }

    #[test]
    fn control_wrong_route_event_refuses() {
        let mut cells = CELLS.to_vec();
        cells[0].event = Event75(49);
        let wrong = Table75 { actions: &ACTIONS, cells: leak(cells) };
        assert_eq!(check_vectors(&wrong, &VECTORS), Err(VectorMismatch::StockBodyFetch { event: 50, state: 5 }));
    }

    #[test]
    fn control_retained_body_fetch_guard_refuses_stale_table() {
        let mut cells = CELLS.to_vec();
        cells[1].cell_word ^= 1;
        let wrong = Table75 { actions: &ACTIONS, cells: leak(cells) };
        assert_eq!(check_vectors(&wrong, &VECTORS), Err(VectorMismatch::StockBodyFetch { event: 41, state: 0 }));
    }

    /// Owner vectors: build with `--cfg rb_modal75_vectors` and
    /// `RB_MODAL75_TABLE`/`RB_MODAL75_VECTORS` naming the generator outputs.
    #[cfg(rb_modal75_vectors)]
    mod owner {
        use super::super::*;
        use super::leak;

        static OWNER_VECTORS: &[StockVector] = include!(env!("RB_MODAL75_VECTORS"));

        fn first_route(actions: &[GenericAction]) -> usize {
            actions.iter().position(|a| matches!(a, GenericAction::RouteToTransition(r) if r.len > 0)).expect("routed action")
        }

        #[test]
        fn owner_table_matches_stock_vectors() {
            assert_eq!(check_vectors(&TABLE, OWNER_VECTORS), Ok(()));
        }

        #[test]
        fn owner_controls_refuse() {
            let mut actions = TABLE.actions.to_vec();
            let index = first_route(&actions);
            if let GenericAction::RouteToTransition(mut route) = actions[index] {
                route.pairs[0].1 = State75((route.pairs[0].1 .0 + 1) % 46);
                actions[index] = GenericAction::RouteToTransition(route);
            }
            let wrong_state = Table75 { actions: leak(actions), cells: TABLE.cells };
            assert!(matches!(check_vectors(&wrong_state, OWNER_VECTORS), Err(VectorMismatch::States { .. })));

            let mut actions = TABLE.actions.to_vec();
            let index = actions.iter().position(|a| matches!(a, GenericAction::ConstantResult(_))).expect("constant action");
            if let GenericAction::ConstantResult(ActionResult(value)) = actions[index] {
                actions[index] = GenericAction::ConstantResult(ActionResult(value.wrapping_add(1)));
            }
            let wrong_result = Table75 { actions: leak(actions), cells: TABLE.cells };
            assert!(matches!(check_vectors(&wrong_result, OWNER_VECTORS), Err(VectorMismatch::Result { .. })));

            let mut cells = TABLE.cells.to_vec();
            let index = cells.iter().position(|c| matches!(c.action, CellAction::Generic(_)) && c.event.raw() + 1 < EVENTS).expect("generic cell");
            cells[index].event = Event75(cells[index].event.0 + 1);
            let wrong_event = Table75 { actions: TABLE.actions, cells: leak(cells) };
            assert!(check_vectors(&wrong_event, OWNER_VECTORS).is_err());

            let mut cells = TABLE.cells.to_vec();
            let index = cells.iter().position(|c| matches!(c.after, AfterAction::SetState(_))).expect("direct-state cell");
            if let AfterAction::SetState(next) = cells[index].after {
                cells[index].after = AfterAction::SetState(State75((next.0 + 1) % 46));
            }
            let wrong_after = Table75 { actions: TABLE.actions, cells: leak(cells) };
            assert!(matches!(check_vectors(&wrong_after, OWNER_VECTORS), Err(VectorMismatch::States { .. })));

            let mut cells = TABLE.cells.to_vec();
            let index = cells.iter().position(|c| matches!(c.action, CellAction::Generic(_))).expect("generic cell");
            cells[index].cell_word ^= 1;
            let stale = Table75 { actions: TABLE.actions, cells: leak(cells) };
            assert!(matches!(check_vectors(&stale, OWNER_VECTORS), Err(VectorMismatch::StockBodyFetch { .. })));
        }
    }
}
