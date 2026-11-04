use crate::block::{decode, mirror, DecodeError, RecordAddress, RecordTable, BLOCK_LEN, LAST_MOVE_LEN};
use crate::engine::{
    apply, Effect, EngineResult, EngineServices, KindSwitchCall, MenuEvent, NotOwned, Notify, SeekCall, SnapshotSlot,
    StageEvent, StageExtra, StageId, StageKind, StageNode, Transition,
};
use crate::frame::{
    FrameDepth, MenuClass, MenuFrameSnapshot, NodeId, NodeType, PageNumber, RowIndex, RowToken, RowsPerPage,
    ScrollTop, VisibleCount, VisibleOrdinal,
};
use crate::state::{CursorFrame, EngineState, LastMove, MenuOpen, RowCount, RowState, StackDepth, ROW_STATES, STACK_FRAMES};
use crate::tree::{Availability, Node, NodeRef, NodeWord10, Remember, Row, Tree, TreeIdentity, Wrap, REGISTRY_SLOTS};

const TOP: NodeRef = NodeRef::new(0);
const PAGE: NodeRef = NodeRef::new(1);
const PICKER: NodeRef = NodeRef::new(2);
const STOPPING_PICKER: NodeRef = NodeRef::new(3);
const LONG: NodeRef = NodeRef::new(4);
const FRAMED: NodeRef = NodeRef::new(5);

const fn row(token: u32, child: Option<NodeRef>) -> Row {
    Row { token: RowToken::new(token), child }
}

static TOP_ROWS: [Row; 1] = [row(0x10, Some(PAGE))];
static PAGE_ROWS: [Row; 3] = [row(0x20, None), row(0x21, Some(PICKER)), row(0x22, None)];
static PICKER_ROWS: [Row; 4] = [row(0, None), row(1, None), row(2, None), row(3, None)];
static LONG_ROWS: [Row; 10] = [
    row(0, None),
    row(1, None),
    row(2, None),
    row(3, None),
    row(4, None),
    row(5, None),
    row(6, None),
    row(7, None),
    row(8, None),
    row(9, None),
];

const fn node(id: u32, class: u32, wrap: Wrap, parent: Option<NodeRef>, rows: &'static [Row]) -> Node {
    Node {
        id: NodeId::new(id),
        node_type: NodeType::new(0),
        class: MenuClass::new(class),
        rows_per_page: RowsPerPage::new(7),
        wrap,
        availability: Availability::Queried,
        remember: Remember::On,
        parent,
        word10: NodeWord10::new(0x2a),
        rows: Some(rows),
    }
}

static NODES: [Node; 6] = [
    node(0, 5, Wrap::Wraps, None, &TOP_ROWS),
    node(1, 5, Wrap::Wraps, Some(TOP), &PAGE_ROWS),
    node(1, 0x104, Wrap::Wraps, Some(PAGE), &PICKER_ROWS),
    node(2, 0x104, Wrap::Stops, Some(PAGE), &PICKER_ROWS),
    node(3, 0x105, Wrap::Wraps, Some(PAGE), &LONG_ROWS),
    Node { node_type: NodeType::new(7), ..node(4, 0x106, Wrap::Wraps, Some(PAGE), &PICKER_ROWS) },
];

const fn registry() -> [Option<NodeRef>; REGISTRY_SLOTS] {
    let mut slots = [None; REGISTRY_SLOTS];
    slots[0] = Some(TOP);
    slots[1] = Some(PAGE);
    slots[0x7a] = Some(PAGE);
    slots
}

static TREE: Tree = Tree { nodes: &NODES, registry: registry(), identity: TreeIdentity([7; 32]) };

static RECORD_ADDRESSES: [RecordAddress; 6] = [
    RecordAddress::new(0x0078_0000),
    RecordAddress::new(0x0078_0040),
    RecordAddress::new(0x0078_0080),
    RecordAddress::new(0x0078_00c0),
    RecordAddress::new(0x0078_0100),
    RecordAddress::new(0x0078_0140),
];

static RECORDS: RecordTable = RecordTable { tree: TreeIdentity([7; 32]), records: &RECORD_ADDRESSES };

const fn frame(cursor: u32, ordinal: u32, count: u32, scroll: u32) -> CursorFrame {
    CursorFrame {
        cursor: RowIndex::new(cursor),
        ordinal: VisibleOrdinal::new(ordinal),
        count: VisibleCount::new(count),
        scroll: ScrollTop::new(scroll),
    }
}

fn closed() -> EngineState {
    EngineState {
        node: None,
        word10: NodeWord10::new(0),
        word10_high: [0, 0],
        live: CursorFrame::ZERO,
        total: RowCount::new(0),
        visible_at_open: VisibleCount::new(0),
        row_states: [RowState::Offered; ROW_STATES],
        saved: CursorFrame::ZERO,
        remembered: [RowToken::new(0); REGISTRY_SLOTS],
        stack: [CursorFrame::ZERO; STACK_FRAMES],
        depth: StackDepth::new(0),
        push_classifier: FrameDepth::Root,
        menu_open: MenuOpen::Closed,
        last_move: LastMove { class: MenuClass::new(0x99), token: RowToken::new(9) },
    }
}

fn at(node: NodeRef, live: CursorFrame, total: u32, depth: u32) -> EngineState {
    EngineState {
        node: Some(node),
        word10: NodeWord10::new(0x2a),
        live,
        total: RowCount::new(total),
        saved: live,
        depth: StackDepth::new(depth),
        menu_open: MenuOpen::Open,
        ..closed()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Call {
    Perform(Effect),
    Availability(MenuClass, RowToken),
    InitialRow(StageKind, StageNode, MenuClass),
    KindSwitch(KindSwitchCall),
    Seek(SeekCall),
    PastRows(RowIndex),
}

/// Answers from queues and records every call in order.
#[derive(Default)]
struct Answers {
    availability: Vec<RowState>,
    initial_row: Vec<Option<RowToken>>,
    kind_switch: Option<(EngineState, EngineResult)>,
    past_rows: Vec<RowToken>,
    seek: Option<(EngineState, EngineResult)>,
    calls: Vec<Call>,
}

impl EngineServices for Answers {
    fn perform(&mut self, _: &EngineState, effect: Effect) {
        self.calls.push(Call::Perform(effect));
    }

    fn availability(&mut self, _: &EngineState, class: MenuClass, token: RowToken) -> RowState {
        self.calls.push(Call::Availability(class, token));
        self.availability.remove(0)
    }

    fn initial_row(&mut self, _: &EngineState, kind: StageKind, node: StageNode, class: MenuClass) -> Option<RowToken> {
        self.calls.push(Call::InitialRow(kind, node, class));
        self.initial_row.remove(0)
    }

    fn kind_switch(&mut self, _: &EngineState, call: KindSwitchCall) -> (EngineState, EngineResult) {
        self.calls.push(Call::KindSwitch(call));
        self.kind_switch.take().expect("kind switch answer")
    }

    fn seek(&mut self, _: &EngineState, call: SeekCall) -> (EngineState, EngineResult) {
        self.calls.push(Call::Seek(call));
        self.seek.take().expect("seek answer")
    }

    fn token_past_rows(&mut self, _: &EngineState, row: RowIndex) -> RowToken {
        self.calls.push(Call::PastRows(row));
        self.past_rows.remove(0)
    }

    fn invariant_failure(&mut self) -> ! {
        panic!("invariant failure after {} calls", self.calls.len())
    }
}

impl Answers {
    fn stores(&self) -> Vec<(FrameDepth, SnapshotSlot)> {
        self.calls
            .iter()
            .filter_map(|call| match *call {
                Call::Perform(Effect::StoreSnapshot { depth, slot, .. }) => Some((depth, slot)),
                _ => None,
            })
            .collect()
    }

    fn notifies(&self) -> Vec<Notify> {
        self.calls
            .iter()
            .filter_map(|call| match *call {
                Call::Perform(Effect::Notify(notify)) => Some(notify),
                _ => None,
            })
            .collect()
    }

    fn records(&self) -> Vec<MenuFrameSnapshot> {
        self.calls
            .iter()
            .filter_map(|call| match *call {
                Call::Perform(Effect::StoreSnapshot { record, .. }) => Some(record),
                _ => None,
            })
            .collect()
    }
}

fn event(kind: u32, node: u32, id: u32, state: &EngineState) -> MenuEvent {
    MenuEvent::decode(StageEvent { kind, node, id, extra: 0 }, &TREE, state)
}

fn run(state: &EngineState, kind: u32, node: u32, id: u32, answers: &mut Answers) -> Result<Transition, NotOwned> {
    apply(&TREE, state, event(kind, node, id, state), answers)
}

#[test]
fn round_trip_restores_every_byte() {
    let mut state = at(PICKER, frame(3, 3, 4, 0), 4, 2);
    state.word10_high = [0xab, 0xcd];
    state.row_states[2] = RowState::Disabled;
    state.row_states[ROW_STATES - 1] = RowState::Hidden;
    state.remembered[0x85] = RowToken::new(0x7fff_fffe);
    state.stack[9] = frame(5, 4, 3, 2);
    state.push_classifier = FrameDepth::Second;
    let (block, last_move) = mirror(&state, &RECORDS);
    assert_eq!(&block[0..4], &0x0078_0080u32.to_le_bytes());
    let decoded = decode(&block, &last_move, &TREE, &RECORDS).expect("decodes");
    assert_eq!(decoded, state);
    assert_eq!(mirror(&decoded, &RECORDS), (block, last_move));
}

#[test]
fn decode_refuses_values_outside_the_block_domains() {
    let (block, last_move) = mirror(&at(PICKER, frame(0, 0, 4, 0), 4, 1), &RECORDS);
    let mut unknown_record = block;
    unknown_record[0..4].copy_from_slice(&0x0078_0004u32.to_le_bytes());
    assert_eq!(
        decode(&unknown_record, &last_move, &TREE, &RECORDS),
        Err(DecodeError::UnknownRecord(RecordAddress::new(0x0078_0004)))
    );
    let mut row_state = block;
    row_state[0x20 + 4 * 5] = 4;
    assert_eq!(decode(&row_state, &last_move, &TREE, &RECORDS), Err(DecodeError::RowState { row: 5, raw: 4 }));
    let other_tree = RecordTable { tree: TreeIdentity([8; 32]), records: &RECORD_ADDRESSES };
    assert_eq!(decode(&block, &last_move, &TREE, &other_tree), Err(DecodeError::TableMismatch));
    let zero_block = [0u8; BLOCK_LEN];
    assert_eq!(decode(&zero_block, &[0; LAST_MOVE_LEN], &TREE, &RECORDS).expect("closed").node, None);
}

#[test]
fn row_step_down_from_the_last_row_wraps_to_the_first_selectable() {
    let mut state = at(PICKER, frame(3, 3, 4, 0), 4, 2);
    state.row_states[0] = RowState::Disabled;
    let mut answers = Answers::default();
    let transition = run(&state, 4, 1, 1, &mut answers).expect("owned");
    assert_eq!(transition.result, EngineResult::Accepted);
    assert_eq!(transition.state.live, frame(1, 1, 4, 0));
    assert_eq!(transition.state.last_move, LastMove { class: MenuClass::new(0x104), token: RowToken::new(1) });
    let notify = Notify { kind: StageKind::MoveDown, node: StageNode::new(1), id: StageId::VALUE, class: MenuClass::new(0x104) };
    assert!(matches!(answers.calls[0], Call::Perform(Effect::Notify(n)) if n == notify));
    assert_eq!(answers.stores(), [(FrameDepth::Second, SnapshotSlot::new(1)), (FrameDepth::Second, SnapshotSlot::new(1))]);
    assert_eq!(
        answers.records()[1],
        MenuFrameSnapshot {
            depth: FrameDepth::Second,
            node: NodeId::new(1),
            node_type: NodeType::new(0),
            class: MenuClass::new(0x104),
            cursor_token: RowToken::new(1),
            current_token: RowToken::new(3),
            cursor: RowIndex::new(1),
            visible_ordinal: VisibleOrdinal::new(1),
            visible_count: VisibleCount::new(4),
            page: PageNumber::new(1),
            scroll_top: ScrollTop::new(0),
        }
    );
}

#[test]
fn row_step_down_inside_the_rows_moves_one_row() {
    let state = at(PICKER, frame(1, 1, 4, 0), 4, 2);
    let transition = run(&state, 4, 1, 1, &mut Answers::default()).expect("owned");
    assert_eq!(transition.state.live.cursor, RowIndex::new(2));
}

#[test]
fn row_step_up_past_the_first_row_wraps_and_scrolls_to_the_last_page() {
    let state = at(LONG, frame(0, 0, 10, 0), 10, 2);
    let mut answers = Answers::default();
    let transition = run(&state, 5, 3, 1, &mut answers).expect("owned");
    assert_eq!(transition.result, EngineResult::Accepted);
    assert_eq!(transition.state.live, frame(9, 9, 10, 3));
    assert_eq!(answers.notifies()[0].kind, StageKind::MoveUp);
    let back = run(&transition.state, 4, 3, 1, &mut Answers::default()).expect("owned");
    assert_eq!(back.state.live, frame(0, 0, 10, 0));
}

#[test]
fn row_step_up_skips_rows_that_are_not_selectable() {
    let mut state = at(PICKER, frame(2, 2, 4, 0), 4, 2);
    state.row_states[1] = RowState::Disabled;
    state.row_states[0] = RowState::Hidden;
    let transition = run(&state, 5, 1, 1, &mut Answers::default()).expect("owned");
    assert_eq!(transition.state.live, frame(3, 2, 3, 0));
}

#[test]
fn push_notifies_before_it_asks_availability_and_opens_the_child_at_its_initial_row() {
    let mut state = at(PAGE, frame(1, 1, 3, 0), 3, 1);
    state.stack[2] = frame(2, 2, 4, 0);
    let mut answers = Answers {
        availability: vec![RowState::Offered, RowState::Offered, RowState::Disabled, RowState::Disabled],
        initial_row: vec![Some(RowToken::new(0))],
        ..Answers::default()
    };
    let transition = run(&state, 0xe, 1, 0x1d, &mut answers).expect("owned");
    assert_eq!(transition.result, EngineResult::Accepted);
    let after = transition.state;
    assert_eq!(after.node, Some(PICKER));
    assert_eq!(after.depth, StackDepth::new(2));
    assert_eq!(after.stack[1], frame(1, 1, 3, 0));
    assert_eq!(after.remembered[1], RowToken::new(0x21));
    assert_eq!(after.push_classifier, FrameDepth::First);
    assert_eq!(after.live, frame(0, 0, 4, 0));
    assert_eq!(after.saved, frame(0, 0, 4, 0));
    assert_eq!(after.total, RowCount::new(4));
    assert_eq!(after.visible_at_open, VisibleCount::new(4));
    assert_eq!(&after.row_states[..5], &[RowState::Offered, RowState::Offered, RowState::Disabled, RowState::Disabled, RowState::Offered]);
    assert_eq!(after.last_move, state.last_move);
    let notify = Notify { kind: StageKind::Push, node: StageNode::new(1), id: StageId::new(0x1d), class: MenuClass::new(5) };
    let mut expected = vec![Call::Perform(Effect::Notify(notify)), Call::Perform(Effect::PageEntry)];
    expected.extend((0..4).map(|token| Call::Availability(MenuClass::new(0x104), RowToken::new(token))));
    expected.push(Call::InitialRow(StageKind::Push, StageNode::new(1), MenuClass::new(0x104)));
    assert_eq!(&answers.calls[..7], &expected[..]);
    assert_eq!(answers.stores(), [(FrameDepth::Second, SnapshotSlot::new(1)), (FrameDepth::Second, SnapshotSlot::new(1))]);
}

#[test]
fn push_without_an_initial_row_keeps_the_restored_cursor() {
    let mut state = at(PAGE, frame(1, 1, 3, 0), 3, 1);
    state.stack[2] = frame(2, 2, 4, 0);
    let mut answers =
        Answers { availability: vec![RowState::Offered; 4], initial_row: vec![None], ..Answers::default() };
    let transition = run(&state, 0xe, 1, 0x1d, &mut answers).expect("owned");
    assert_eq!(transition.state.live.cursor, RowIndex::new(2));
}

#[test]
fn push_keeping_a_restored_cursor_past_the_rows_reads_the_word_after_them() {
    let mut state = at(PAGE, frame(1, 1, 3, 0), 3, 1);
    state.stack[2] = frame(20, 19, 20, 13);
    let mut answers = Answers {
        availability: vec![RowState::Offered; 4],
        initial_row: vec![None],
        past_rows: vec![RowToken::new(2), RowToken::new(2)],
        ..Answers::default()
    };
    let transition = run(&state, 0xe, 1, 0x1d, &mut answers).expect("owned");
    assert_eq!(transition.result, EngineResult::Accepted);
    assert_eq!(transition.state.live, frame(20, 2, 4, 0));
    assert_eq!(&answers.calls[7..9], &[Call::PastRows(RowIndex::new(20)), Call::PastRows(RowIndex::new(20))]);
    assert_eq!(answers.calls[9], Call::Perform(Effect::StoreFromBlock { slot: SnapshotSlot::new(1) }));
}

#[test]
fn a_nested_pop_landing_on_a_framed_node_runs_the_stock_seek() {
    let mut state = at(PAGE, frame(1, 1, 3, 0), 3, 1);
    state.stack[2] = frame(2, 2, 4, 0);
    let popped = at(FRAMED, frame(0, 0, 4, 0), 4, 1);
    let sought = at(PAGE, frame(2, 2, 3, 0), 3, 1);
    let mut answers = Answers {
        availability: vec![RowState::Disabled; 4],
        kind_switch: Some((popped, EngineResult::Accepted)),
        seek: Some((sought, EngineResult::Rejected)),
        ..Answers::default()
    };
    let transition = run(&state, 0xe, 1, 0x1d, &mut answers).expect("owned");
    assert_eq!(transition.result, EngineResult::Rejected);
    assert_eq!(transition.state, sought);
    let seek = SeekCall {
        kind: StageKind::Push,
        node: StageNode::new(4),
        node_type: NodeType::new(7),
        id: StageId::new(0x1d),
        seek: StageId::new(0),
        target: StageExtra::new(0),
    };
    assert_eq!(
        &answers.calls[6..],
        &[
            Call::KindSwitch(KindSwitchCall { kind: StageKind::Pop, node: StageNode::new(1), id: StageId::NESTED_POP, extra: StageExtra::new(0) }),
            Call::Perform(Effect::StoreFromBlock { slot: SnapshotSlot::new(1) }),
            Call::Seek(seek),
            Call::Perform(Effect::StoreFromBlock { slot: SnapshotSlot::new(1) }),
        ]
    );
}

#[test]
fn push_into_a_node_without_selectable_rows_runs_the_stock_nested_pop() {
    let mut state = at(PAGE, frame(1, 1, 3, 0), 3, 1);
    state.stack[2] = frame(2, 2, 4, 0);
    let popped = at(PAGE, frame(1, 1, 3, 0), 3, 1);
    let mut answers = Answers {
        availability: vec![RowState::Disabled; 4],
        kind_switch: Some((popped, EngineResult::Accepted)),
        ..Answers::default()
    };
    let transition = run(&state, 0xe, 1, 0x1d, &mut answers).expect("owned");
    assert_eq!(transition.result, EngineResult::Accepted);
    assert_eq!(transition.state, popped);
    let call = KindSwitchCall { kind: StageKind::Pop, node: StageNode::new(1), id: StageId::NESTED_POP, extra: StageExtra::new(0) };
    assert_eq!(answers.calls[6], Call::KindSwitch(call));
    assert_eq!(
        &answers.calls[7..],
        &[
            Call::Perform(Effect::StoreFromBlock { slot: SnapshotSlot::new(1) }),
            Call::Perform(Effect::StoreFromBlock { slot: SnapshotSlot::new(1) })
        ]
    );
}

#[test]
fn open_of_a_node_without_selectable_rows_or_parent_is_rejected() {
    let mut answers = Answers { availability: vec![RowState::Hidden], ..Answers::default() };
    let transition = run(&closed(), 1, 0, 0, &mut answers).expect("owned");
    assert_eq!(transition.result, EngineResult::Rejected);
    assert_eq!(transition.state.menu_open, MenuOpen::Closed);
    assert_eq!(transition.state.visible_at_open, VisibleCount::new(0));
    assert!(answers.notifies().is_empty());
}

#[test]
fn pop_restores_the_parent_cursor() {
    let mut state = at(PICKER, frame(0, 0, 4, 0), 4, 2);
    state.stack[1] = frame(1, 1, 3, 0);
    state.remembered[1] = RowToken::new(0x55);
    let mut answers =
        Answers { availability: vec![RowState::Offered; 3], initial_row: vec![None], ..Answers::default() };
    let transition = run(&state, 0xf, 1, 0x1a, &mut answers).expect("owned");
    assert_eq!(transition.result, EngineResult::Accepted);
    let after = transition.state;
    assert_eq!(after.node, Some(PAGE));
    assert_eq!(after.depth, StackDepth::new(1));
    assert_eq!(after.stack[2], frame(0, 0, 4, 0));
    assert_eq!(after.live, frame(1, 1, 3, 0));
    assert_eq!(after.remembered[1], RowToken::new(0x55));
    assert_eq!(
        answers.notifies(),
        [Notify { kind: StageKind::Pop, node: StageNode::new(1), id: StageId::new(0x1a), class: MenuClass::new(0x104) }]
    );
    assert_eq!(answers.stores(), [(FrameDepth::First, SnapshotSlot::new(1)), (FrameDepth::First, SnapshotSlot::new(1))]);
}

#[test]
fn open_steps_past_a_hidden_initial_row() {
    let mut state = closed();
    state.remembered[1] = RowToken::new(0x22);
    state.stack[4] = frame(9, 9, 9, 9);
    let mut answers = Answers {
        availability: vec![RowState::Hidden, RowState::Offered, RowState::Offered],
        initial_row: vec![Some(RowToken::new(0x20))],
        ..Answers::default()
    };
    let transition = run(&state, 1, 1, 0, &mut answers).expect("owned");
    assert_eq!(transition.result, EngineResult::Accepted);
    let after = transition.state;
    assert_eq!(after.node, Some(PAGE));
    assert_eq!(after.menu_open, MenuOpen::Open);
    assert_eq!(after.depth, StackDepth::new(1));
    assert_eq!(after.stack, [CursorFrame::ZERO; STACK_FRAMES]);
    assert_eq!(after.live, frame(1, 0, 2, 0));
    assert_eq!(after.saved, frame(1, 0, 2, 0));
    assert_eq!(after.visible_at_open, VisibleCount::new(2));
    assert_eq!(
        answers.notifies(),
        [Notify { kind: StageKind::Open, node: StageNode::new(1), id: StageId::VALUE, class: MenuClass::new(5) }]
    );
    assert_eq!(answers.stores(), [(FrameDepth::First, SnapshotSlot::new(1))]);
}

/// Panics on any call.
struct Unasked;

impl EngineServices for Unasked {
    fn perform(&mut self, _: &EngineState, effect: Effect) {
        panic!("{effect:?} performed")
    }

    fn availability(&mut self, _: &EngineState, _: MenuClass, _: RowToken) -> RowState {
        panic!("availability asked")
    }

    fn initial_row(&mut self, _: &EngineState, _: StageKind, _: StageNode, _: MenuClass) -> Option<RowToken> {
        panic!("initial row asked")
    }

    fn kind_switch(&mut self, _: &EngineState, _: KindSwitchCall) -> (EngineState, EngineResult) {
        panic!("kind switch asked")
    }

    fn seek(&mut self, _: &EngineState, _: SeekCall) -> (EngineState, EngineResult) {
        panic!("seek asked")
    }

    fn token_past_rows(&mut self, _: &EngineState, _: RowIndex) -> RowToken {
        panic!("token past rows asked")
    }

    fn invariant_failure(&mut self) -> ! {
        panic!("invariant failure")
    }
}

#[test]
fn every_other_class_is_refused_before_any_service_call() {
    let picker = at(PICKER, frame(1, 1, 4, 0), 4, 2);
    let stopping = at(STOPPING_PICKER, frame(3, 3, 4, 0), 4, 2);
    let page = at(PAGE, frame(1, 1, 3, 0), 3, 1);
    let mut far_restored = page;
    far_restored.stack[2] = frame(ROW_STATES as u32, 0, 4, 0);
    let refused = [
        (picker, StageEvent { kind: 4, node: 1, id: 7, extra: 0 }),
        (picker, StageEvent { kind: 4, node: 1, id: 0, extra: 0 }),
        (picker, StageEvent { kind: 5, node: 1, id: 7, extra: 0 }),
        (picker, StageEvent { kind: 2, node: 1, id: 0x1a, extra: 0 }),
        (picker, StageEvent { kind: 3, node: 1, id: 0, extra: 0 }),
        (picker, StageEvent { kind: 0x13, node: 1, id: 0, extra: 0 }),
        (stopping, StageEvent { kind: 4, node: 1, id: 1, extra: 0 }),
        (stopping, StageEvent { kind: 5, node: 1, id: 1, extra: 0 }),
        (page, StageEvent { kind: 0xe, node: 1, id: 0x17, extra: 0 }),
        (page, StageEvent { kind: 0xe, node: 0x7a, id: 0x1d, extra: 0 }),
        (far_restored, StageEvent { kind: 0xe, node: 1, id: 0x1d, extra: 0 }),
        (picker, StageEvent { kind: 0xe, node: 1, id: 0x1d, extra: 0 }),
        (page, StageEvent { kind: 0xf, node: 1, id: 0x1a, extra: 0 }),
        (closed(), StageEvent { kind: 1, node: 0x38, id: 0, extra: 0 }),
        (closed(), StageEvent { kind: 1, node: 2, id: 0, extra: 0 }),
        (closed(), StageEvent { kind: 1, node: 1, id: 0xe, extra: 0 }),
    ];
    for (state, raw) in refused {
        let decoded = MenuEvent::decode(raw, &TREE, &state);
        assert!(apply(&TREE, &state, decoded, &mut Unasked).is_err(), "{raw:?}");
    }
}

#[test]
fn push_into_a_node_that_stops_at_its_ends_is_refused_before_any_service_call() {
    static STOP_ROWS: [Row; 1] = [row(0x30, Some(STOPPING_PICKER))];
    static STOP_NODES: [Node; 4] = [
        node(0, 5, Wrap::Wraps, None, &TOP_ROWS),
        node(1, 5, Wrap::Wraps, Some(TOP), &STOP_ROWS),
        node(1, 0x104, Wrap::Wraps, Some(PAGE), &PICKER_ROWS),
        node(2, 0x104, Wrap::Stops, Some(PAGE), &PICKER_ROWS),
    ];
    let tree = Tree { nodes: &STOP_NODES, registry: registry(), identity: TreeIdentity([7; 32]) };
    let state = at(PAGE, frame(0, 0, 1, 0), 1, 1);
    let decoded = MenuEvent::decode(StageEvent { kind: 0xe, node: 1, id: 0x1d, extra: 0 }, &tree, &state);
    assert!(apply(&tree, &state, decoded, &mut Unasked).is_err());
}
