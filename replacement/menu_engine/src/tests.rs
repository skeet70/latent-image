use crate::block::{decode, mirror, DecodeError, RecordAddress, RecordTable, BLOCK_LEN, LAST_MOVE_LEN};
use crate::engine::{
    apply, Effect, EngineResult, EngineServices, MenuEvent, NotOwned, Notify, SnapshotSlot, StageEvent, StageId,
    StageKind, StageNode, Transition,
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

const fn row(token: u32, child: Option<NodeRef>) -> Row {
    Row { token: RowToken::new(token), child }
}

static TOP_ROWS: [Row; 1] = [row(0x10, Some(PAGE))];
static PAGE_ROWS: [Row; 3] = [row(0x20, None), row(0x21, Some(PICKER)), row(0x22, None)];
static PICKER_ROWS: [Row; 4] = [row(0, None), row(1, None), row(2, None), row(3, None)];

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
        labels: None,
    }
}

static NODES: [Node; 4] = [
    node(0, 5, Wrap::Wraps, None, &TOP_ROWS),
    node(1, 5, Wrap::Wraps, Some(TOP), &PAGE_ROWS),
    node(1, 0x104, Wrap::Wraps, Some(PAGE), &PICKER_ROWS),
    node(2, 0x104, Wrap::Stops, Some(PAGE), &PICKER_ROWS),
];

const fn registry() -> [Option<NodeRef>; REGISTRY_SLOTS] {
    let mut slots = [None; REGISTRY_SLOTS];
    slots[0] = Some(TOP);
    slots[1] = Some(PAGE);
    slots[0x7a] = Some(PAGE);
    slots
}

static TREE: Tree = Tree { nodes: &NODES, registry: registry(), identity: TreeIdentity([7; 32]) };

static RECORD_ADDRESSES: [RecordAddress; 4] =
    [RecordAddress::new(0x0078_0000), RecordAddress::new(0x0078_0040), RecordAddress::new(0x0078_0080), RecordAddress::new(0x0078_00c0)];

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

/// Answers from queues and records each question.
#[derive(Default)]
struct Answers {
    availability: Vec<RowState>,
    initial_row: Vec<Option<RowToken>>,
    availability_asked: Vec<(MenuClass, RowToken)>,
    initial_row_asked: Vec<(StageKind, StageNode, MenuClass)>,
}

impl EngineServices for Answers {
    fn availability(&mut self, _: &EngineState, class: MenuClass, token: RowToken) -> RowState {
        self.availability_asked.push((class, token));
        self.availability.remove(0)
    }

    fn initial_row(&mut self, _: &EngineState, kind: StageKind, node: StageNode, class: MenuClass) -> Option<RowToken> {
        self.initial_row_asked.push((kind, node, class));
        self.initial_row.remove(0)
    }
}

fn event(kind: u32, node: u32, id: u32, state: &EngineState) -> MenuEvent {
    MenuEvent::decode(StageEvent { kind, node, id, extra: 0 }, &TREE, state)
}

fn run(state: &EngineState, kind: u32, node: u32, id: u32, answers: &mut Answers) -> Result<Transition, NotOwned> {
    apply(&TREE, state, event(kind, node, id, state), answers)
}

fn effects(transition: &Transition) -> Vec<Effect> {
    transition.effects.iter().copied().collect()
}

fn stores(transition: &Transition) -> Vec<(FrameDepth, SnapshotSlot)> {
    transition
        .effects
        .iter()
        .filter_map(|effect| match *effect {
            Effect::StoreSnapshot { depth, slot, .. } => Some((depth, slot)),
            Effect::Notify(_) => None,
        })
        .collect()
}

fn notifies(transition: &Transition) -> Vec<Notify> {
    transition
        .effects
        .iter()
        .filter_map(|effect| match *effect {
            Effect::Notify(notify) => Some(notify),
            Effect::StoreSnapshot { .. } => None,
        })
        .collect()
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
    let transition = run(&state, 4, 1, 1, &mut Answers::default()).expect("owned");
    assert_eq!(transition.result, EngineResult::Accepted);
    assert_eq!(transition.state.live, frame(1, 1, 4, 0));
    assert_eq!(transition.state.last_move, LastMove { class: MenuClass::new(0x104), token: RowToken::new(1) });
    assert_eq!(
        notifies(&transition),
        [Notify { kind: StageKind::MoveDown, node: StageNode::new(1), id: StageId::VALUE, class: MenuClass::new(0x104) }]
    );
    assert_eq!(stores(&transition), [(FrameDepth::Second, SnapshotSlot::new(1)), (FrameDepth::Second, SnapshotSlot::new(1))]);
    let Effect::StoreSnapshot { record, .. } = effects(&transition)[1] else { panic!("store") };
    assert_eq!(
        record,
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
fn push_saves_the_parent_cursor_and_opens_the_child_at_its_initial_row() {
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
    let asked: Vec<_> = (0..4).map(|token| (MenuClass::new(0x104), RowToken::new(token))).collect();
    assert_eq!(answers.availability_asked, asked);
    assert_eq!(answers.initial_row_asked, [(StageKind::Push, StageNode::new(1), MenuClass::new(0x104))]);
    assert_eq!(
        notifies(&transition),
        [Notify { kind: StageKind::Push, node: StageNode::new(1), id: StageId::new(0x1d), class: MenuClass::new(5) }]
    );
    assert_eq!(stores(&transition), [(FrameDepth::Second, SnapshotSlot::new(1)), (FrameDepth::Second, SnapshotSlot::new(1))]);
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
    assert_eq!(answers.initial_row_asked, [(StageKind::Pop, StageNode::new(1), MenuClass::new(5))]);
    assert_eq!(
        notifies(&transition),
        [Notify { kind: StageKind::Pop, node: StageNode::new(1), id: StageId::new(0x1a), class: MenuClass::new(0x104) }]
    );
    assert_eq!(stores(&transition), [(FrameDepth::First, SnapshotSlot::new(1)), (FrameDepth::First, SnapshotSlot::new(1))]);
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
    assert_eq!(answers.initial_row_asked, [(StageKind::Open, StageNode::new(1), MenuClass::new(5))]);
    assert_eq!(
        notifies(&transition),
        [Notify { kind: StageKind::Open, node: StageNode::new(1), id: StageId::VALUE, class: MenuClass::new(5) }]
    );
    assert_eq!(stores(&transition), [(FrameDepth::First, SnapshotSlot::new(1))]);
}

/// Panics on any question.
struct Unasked;

impl EngineServices for Unasked {
    fn availability(&mut self, _: &EngineState, _: MenuClass, _: RowToken) -> RowState {
        panic!("availability asked")
    }

    fn initial_row(&mut self, _: &EngineState, _: StageKind, _: StageNode, _: MenuClass) -> Option<RowToken> {
        panic!("initial row asked")
    }
}

#[test]
fn every_other_class_is_refused() {
    let picker = at(PICKER, frame(1, 1, 4, 0), 4, 2);
    let stopping = at(STOPPING_PICKER, frame(3, 3, 4, 0), 4, 2);
    let page = at(PAGE, frame(1, 1, 3, 0), 3, 1);
    let refused = [
        (picker, StageEvent { kind: 4, node: 1, id: 7, extra: 0 }, NotOwned::EventClass),
        (picker, StageEvent { kind: 4, node: 1, id: 0, extra: 0 }, NotOwned::EventClass),
        (picker, StageEvent { kind: 5, node: 1, id: 1, extra: 0 }, NotOwned::EventClass),
        (picker, StageEvent { kind: 2, node: 1, id: 0x1a, extra: 0 }, NotOwned::EventClass),
        (picker, StageEvent { kind: 3, node: 1, id: 0, extra: 0 }, NotOwned::EventClass),
        (picker, StageEvent { kind: 0x13, node: 1, id: 0, extra: 0 }, NotOwned::EventClass),
        (stopping, StageEvent { kind: 4, node: 1, id: 1, extra: 0 }, NotOwned::NodeFlags),
        (page, StageEvent { kind: 0xe, node: 1, id: 0x17, extra: 0 }, NotOwned::EventClass),
        (page, StageEvent { kind: 0xe, node: 0x7a, id: 0x1d, extra: 0 }, NotOwned::StageNode),
        (picker, StageEvent { kind: 0xe, node: 1, id: 0x1d, extra: 0 }, NotOwned::RememberBranch),
        (page, StageEvent { kind: 0xf, node: 1, id: 0x1a, extra: 0 }, NotOwned::RememberBranch),
        (closed(), StageEvent { kind: 1, node: 0x38, id: 0, extra: 0 }, NotOwned::StageNode),
        (closed(), StageEvent { kind: 1, node: 2, id: 0, extra: 0 }, NotOwned::StageNode),
        (closed(), StageEvent { kind: 1, node: 1, id: 0xe, extra: 0 }, NotOwned::EventClass),
    ];
    for (state, raw, reason) in refused {
        let decoded = MenuEvent::decode(raw, &TREE, &state);
        assert_eq!(apply(&TREE, &state, decoded, &mut Unasked).map(|_| ()), Err(reason), "{raw:?}");
    }
}
