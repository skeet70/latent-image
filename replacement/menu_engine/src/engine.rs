//! The engine wrapper `0xbf95c8`: the kind switch `0xbfa234`, then the
//! snapshot store at the open node's record id. Owned classes, each with an
//! oracle case:
//!
//! - kinds 4 and 5, a row step on a node with `Wrap::Wraps` and
//!   `Availability::Queried`;
//! - kind `0xe`, no seek id, from a node whose remember write runs (parent with
//!   another id) into a node of type other than 5 or 7;
//! - kind `0xf`, no seek id, from a node whose parent has its id, into a node of
//!   type other than 5 or 7;
//! - kind 1 on a registered node other than `0x38` of type other than 5 or 7,
//!   with an id the `0xbfad04` switch passes to its default arm.
//!
//! The node a push, pop or open opens (`0xbfa800`) also needs `Wrap::Wraps`,
//! `Availability::Queried`, 1..=`ROW_STATES` rows and a restored cursor below
//! `ROW_STATES`. `apply` decides all of this before its first service call.
//! When no row is selectable after the availability answers, the engine takes
//! `0xbfa800`'s empty arm: it returns 0 at a node without a parent, and
//! otherwise calls the stock kind switch for the nested pop as stock does.

use super::frame::{
    self, FrameCells, FrameDepth, FrameNode, MenuClass, MenuFrameSnapshot, PageNumber, RowIndex, RowToken, RowValues,
    RowsPerPage, ScrollTop, VisibleCount, VisibleOrdinal,
};
use super::frame::{word, NodeId, NodeType};
use super::state::{CursorFrame, EngineState, LastMove, MenuOpen, RowCount, RowState, StackDepth, ROW_STATES, STACK_FRAMES};
use super::tree::{Availability, Node, NodeIndex, NodeRef, Remember, Row, Tree, Wrap};

word!(
    /// The node word of a stage event, and the node argument the engine passes
    /// to notifications, the initial row and snapshot stores.
    StageNode
);
word!(
    /// Word 2 of a stage event.
    StageId
);
word!(
    /// Word 3 of a stage event.
    StageExtra
);
word!(
    /// The index `FUN_0061ceac` stores a depth-one or depth-two record at.
    SnapshotSlot
);
word!(
    /// A row move's repeat count, the id word of kinds 4 and 5.
    StepCount
);

impl StageId {
    /// The id the engine passes with its own notifications.
    pub const VALUE: Self = Self::new(0x1d);
    /// The id `0xbfa800` passes to the nested pop of an empty node.
    pub const NESTED_POP: Self = Self::new(0x15);
    const SEEK: Self = Self::new(0x17);
    const SEEK_PUSH: Self = Self::new(0x18);
    const NO_SEEK: Self = Self::new(0);

    /// An id the `0xbfad04` switch sends to an arm other than its default.
    fn seeks(self) -> bool {
        matches!(self.0, 0xe..=0x12) || self == Self::SEEK || self == Self::SEEK_PUSH
    }
}

impl From<NodeId> for StageNode {
    fn from(id: NodeId) -> Self {
        Self::new(id.get())
    }
}

/// The arms of the kind switch `0xbfa234`.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StageKind {
    Open = 1,
    Close = 2,
    MoveDown = 4,
    MoveUp = 5,
    PopMoveDownPush = 6,
    PopMoveUpPush = 7,
    PushMoveDownPop = 8,
    PushMoveUpPop = 9,
    SeekPushMoveDownPop = 0xa,
    SeekPushMoveUpPop = 0xb,
    Push = 0xe,
    Pop = 0xf,
    PushTwice = 0x10,
    PopTwice = 0x11,
    PopThrice = 0x12,
    Reopen = 0x13,
    Move45 = 0x45,
    Move46 = 0x46,
    Move47 = 0x47,
    Move48 = 0x48,
}

impl StageKind {
    pub const fn parse(raw: u32) -> Option<Self> {
        Some(match raw {
            1 => Self::Open,
            2 => Self::Close,
            4 => Self::MoveDown,
            5 => Self::MoveUp,
            6 => Self::PopMoveDownPush,
            7 => Self::PopMoveUpPush,
            8 => Self::PushMoveDownPop,
            9 => Self::PushMoveUpPop,
            0xa => Self::SeekPushMoveDownPop,
            0xb => Self::SeekPushMoveUpPop,
            0xe => Self::Push,
            0xf => Self::Pop,
            0x10 => Self::PushTwice,
            0x11 => Self::PopTwice,
            0x12 => Self::PopThrice,
            0x13 => Self::Reopen,
            0x45 => Self::Move45,
            0x46 => Self::Move46,
            0x47 => Self::Move47,
            0x48 => Self::Move48,
            _ => return None,
        })
    }

    pub const fn raw(self) -> u32 {
        self as u32
    }
}

/// The four words `0xd4dd2c` passes on: kind, node, id, extra.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StageEvent {
    pub kind: u32,
    pub node: u32,
    pub id: u32,
    pub extra: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// Step `0x15`.
    Down,
    /// Step `0x16`.
    Up,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Rows(StepCount),
    /// The id equals the open node's rows per page and the node word is nonzero.
    Page,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuEvent {
    Open { node: StageNode, id: StageId, extra: StageExtra },
    Close { node: StageNode, id: StageId },
    Move { direction: Direction, step: Step, node: StageNode },
    /// Kinds 6..`0xb`.
    FrameMove { kind: StageKind, node: StageNode, id: StageId, extra: StageExtra },
    /// Kinds `0x45`..`0x48`.
    GroupMove { kind: StageKind, node: StageNode, id: StageId },
    Push { node: StageNode, id: StageId, extra: StageExtra },
    Pop { node: StageNode, id: StageId, extra: StageExtra },
    /// Kinds `0x10`..`0x12`.
    MultiLevel { kind: StageKind, node: StageNode, id: StageId },
    Reopen { node: StageNode },
    /// A kind the switch sends to its default arm.
    Unknown { kind: u32 },
}

impl MenuEvent {
    /// The page test reads the open node's rows per page; without an open node
    /// a move decodes as a row step.
    pub fn decode(raw: StageEvent, tree: &Tree, state: &EngineState) -> Self {
        let node = StageNode::new(raw.node);
        let id = StageId::new(raw.id);
        let extra = StageExtra::new(raw.extra);
        let Some(kind) = StageKind::parse(raw.kind) else {
            return Self::Unknown { kind: raw.kind };
        };
        let step = || match state.node {
            Some(open) if tree.node(open).rows_per_page.get() == raw.id && raw.node != 0 => Step::Page,
            _ => Step::Rows(StepCount::new(raw.id)),
        };
        match kind {
            StageKind::Open => Self::Open { node, id, extra },
            StageKind::Close => Self::Close { node, id },
            StageKind::MoveDown => Self::Move { direction: Direction::Down, step: step(), node },
            StageKind::MoveUp => Self::Move { direction: Direction::Up, step: step(), node },
            StageKind::PopMoveDownPush
            | StageKind::PopMoveUpPush
            | StageKind::PushMoveDownPop
            | StageKind::PushMoveUpPop
            | StageKind::SeekPushMoveDownPop
            | StageKind::SeekPushMoveUpPop => Self::FrameMove { kind, node, id, extra },
            StageKind::Push => Self::Push { node, id, extra },
            StageKind::Pop => Self::Pop { node, id, extra },
            StageKind::PushTwice | StageKind::PopTwice | StageKind::PopThrice => Self::MultiLevel { kind, node, id },
            StageKind::Reopen => Self::Reopen { node },
            StageKind::Move45 | StageKind::Move46 | StageKind::Move47 | StageKind::Move48 => {
                Self::GroupMove { kind, node, id }
            }
        }
    }
}

/// The value `0xd4dd2c`, the rollback bracket, `0xd42a64` and the value post
/// branch on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EngineResult {
    Accepted,
    Rejected,
}

/// Why `apply` left an event to the stock engine.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotOwned {
    EventClass,
    NoOpenNode,
    NodeFlags,
    NoValues,
    /// A row index past the rows array, a row-state index past `ROW_STATES`
    /// or a node with more rows than `ROW_STATES`.
    RowBounds,
    StackBounds,
    /// A stage node outside the registry or one with a node-specific branch.
    StageNode,
    RememberBranch,
    /// `0xbfad04` pushes or pops again for a node of type 5 or 7.
    FramedNode,
    NoChild,
    NoParent,
    /// A node without rows, which `0xbfa800` treats as empty.
    EmptyNode,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Notify {
    pub kind: StageKind,
    pub node: StageNode,
    pub id: StageId,
    pub class: MenuClass,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Effect {
    /// `FUN_00d5b350(kind, node, id, class)`.
    Notify(Notify),
    /// `FUN_0061ceac(slot)` with the record `FUN_0061cd14` writes.
    StoreSnapshot { depth: FrameDepth, slot: SnapshotSlot, record: MenuFrameSnapshot },
    /// `FUN_0061ceac(slot)` run by the owned frame service over the block:
    /// after a stock call replaced the state, or where the record reads past
    /// the tree's rows.
    StoreFromBlock { slot: SnapshotSlot },
}

/// `0xbfa234(kind, node, id, extra)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KindSwitchCall {
    pub kind: StageKind,
    pub node: StageNode,
    pub id: StageId,
    pub extra: StageExtra,
}

/// `0xbfad04(kind, node, node_type, id, seek, target)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SeekCall {
    pub kind: StageKind,
    pub node: StageNode,
    pub node_type: NodeType,
    pub id: StageId,
    pub seek: StageId,
    pub target: StageExtra,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Transition {
    pub result: EngineResult,
    pub state: EngineState,
}

/// What the engine asks of stock, in stock's order. Before each call the
/// adapter writes `state` to the block. The two stock calls return the block
/// decoded afterwards and the function's result, which stock limits to 0 and 1.
pub trait EngineServices {
    fn perform(&mut self, state: &EngineState, effect: Effect);
    /// `FUN_00d52684(class, token)`.
    fn availability(&mut self, state: &EngineState, class: MenuClass, token: RowToken) -> RowState;
    /// `FUN_00d588b8(kind, node, class)`; `None` for `0x7ffffffe`.
    fn initial_row(&mut self, state: &EngineState, kind: StageKind, node: StageNode, class: MenuClass)
        -> Option<RowToken>;
    /// The token word of `values[row]` of the open node for a row past its
    /// terminator, which stock reads from whatever follows the array.
    fn token_past_rows(&mut self, state: &EngineState, row: RowIndex) -> RowToken;
    fn kind_switch(&mut self, state: &EngineState, call: KindSwitchCall) -> (EngineState, EngineResult);
    fn seek(&mut self, state: &EngineState, call: SeekCall) -> (EngineState, EngineResult);
}

/// Stock decisions the negative controls replace.
pub(crate) trait Rules {
    fn wraps(node: &Node) -> bool {
        node.wrap == Wrap::Wraps
    }

    fn selectable(state: RowState) -> bool {
        state.raw() < RowState::Disabled.raw()
    }

    const INITIAL_ROW: bool = true;
    const LAST_MOVE: bool = true;
    const SCROLL: bool = true;
    const PUSH_CLASSIFIER: bool = true;
}

pub(crate) struct Stock;

impl Rules for Stock {}

/// `0xbf9a5c` also writes property `0x285` for this node index.
const PROPERTY_NODE: StageNode = StageNode::new(0x7a);
/// `0xbfad04` writes `0x009573e4` for this node index and class `0x228`.
const SEEK_CLASS_NODE: StageNode = StageNode::new(0x38);

/// Returns `NotOwned` only before its first service call.
pub fn apply<S: EngineServices>(
    tree: &Tree,
    state: &EngineState,
    event: MenuEvent,
    services: &mut S,
) -> Result<Transition, NotOwned> {
    apply_with::<Stock, S>(tree, state, event, services)
}

pub(crate) fn apply_with<R: Rules, S: EngineServices>(
    tree: &Tree,
    state: &EngineState,
    event: MenuEvent,
    services: &mut S,
) -> Result<Transition, NotOwned> {
    let mut machine = Machine::<R, S> {
        tree,
        state: *state,
        services,
        called: false,
        delegated: false,
        rules: core::marker::PhantomData,
    };
    match machine.run(event) {
        Err(reason) if machine.called => unreachable!("{reason:?} after a service call"),
        Err(reason) => Err(reason),
        Ok(result) => Ok(Transition { result, state: machine.state }),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CountMode {
    /// `0x36`.
    All,
    /// `0x37`.
    Visible,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OrdinalMode {
    /// Mode 0: the row index.
    Index,
    /// Mode 1: hidden rows before it are not counted.
    Visible,
}

/// The arms of `0xbf988c`, chosen by its fourth argument.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScrollRule {
    /// Kind 4.
    Down,
    /// Kind 5.
    Up,
    /// Kind 1, used by every node open.
    Open,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RememberBranch {
    Off,
    NoParent,
    OtherParentId,
    SameParentId,
}

struct Machine<'a, R, S> {
    tree: &'a Tree,
    state: EngineState,
    services: &'a mut S,
    called: bool,
    /// A stock call has replaced `state`.
    delegated: bool,
    rules: core::marker::PhantomData<R>,
}

fn framed(node: &Node) -> bool {
    FRAMED_TYPES.contains(&node.node_type)
}

impl<'a, R: Rules, S: EngineServices> Machine<'a, R, S> {
    fn run(&mut self, event: MenuEvent) -> Result<EngineResult, NotOwned> {
        let result = match event {
            MenuEvent::Move { direction, step: Step::Rows(count), node } => self.move_rows(direction, node, count)?,
            MenuEvent::Push { node, id, .. } => self.push_pop(StageKind::Push, node, id)?,
            MenuEvent::Pop { node, id, .. } => self.push_pop(StageKind::Pop, node, id)?,
            MenuEvent::Open { node, id, extra } => self.open(node, id, extra)?,
            _ => return Err(NotOwned::EventClass),
        };
        let id = self.current()?.id;
        self.store(SnapshotSlot::new(id.get()));
        Ok(result)
    }

    fn current_ref(&self) -> Result<NodeRef, NotOwned> {
        self.state.node.ok_or(NotOwned::NoOpenNode)
    }

    fn current(&self) -> Result<&'a Node, NotOwned> {
        Ok(self.tree.node(self.current_ref()?))
    }

    fn rows(node: &Node) -> Result<&'static [Row], NotOwned> {
        node.rows.ok_or(NotOwned::NoValues)
    }

    /// `values[row]`; the row after the last reads the terminator pair.
    fn row(node: &Node, row: RowIndex) -> Result<Option<&'static Row>, NotOwned> {
        let rows = Self::rows(node)?;
        match (row.get() as usize).cmp(&rows.len()) {
            core::cmp::Ordering::Less => Ok(Some(&rows[row.get() as usize])),
            core::cmp::Ordering::Equal => Ok(None),
            core::cmp::Ordering::Greater => Err(NotOwned::RowBounds),
        }
    }

    fn token_at(node: &Node, row: RowIndex) -> Result<RowToken, NotOwned> {
        Ok(Self::row(node, row)?.map_or(RowToken::TERMINATOR, |row| row.token))
    }

    fn row_state(&self, row: RowIndex) -> Result<RowState, NotOwned> {
        self.state.row_states.get(row.get() as usize).copied().ok_or(NotOwned::RowBounds)
    }

    /// Row states as `0xbf985c` and `0xbf9a2c` pass them: none for a node with
    /// fixed availability.
    fn states_for(&self, node: &Node) -> Result<Option<&[RowState]>, NotOwned> {
        if node.availability != Availability::Queried {
            return Ok(None);
        }
        let rows = node.rows.map_or(0, <[Row]>::len);
        if rows > ROW_STATES {
            return Err(NotOwned::RowBounds);
        }
        Ok(Some(&self.state.row_states[..rows]))
    }

    /// `0xbf985c` / `0xbf9794`.
    fn count(&self, mode: CountMode) -> Result<u32, NotOwned> {
        let node = self.current()?;
        let rows = node.rows.map_or(0, <[Row]>::len) as u32;
        let hidden = match (mode, self.states_for(node)?) {
            (CountMode::Visible, Some(states)) => states.iter().filter(|&&state| state == RowState::Hidden).count() as u32,
            _ => 0,
        };
        Ok(rows.wrapping_sub(hidden))
    }

    /// `0xbf9a2c` / `0xbf9990`: 0 for a token in no row.
    fn ordinal(&self, token: RowToken, mode: OrdinalMode) -> Result<u32, NotOwned> {
        let node = self.current()?;
        let Some(rows) = node.rows else {
            return Ok(0);
        };
        let states = self.states_for(node)?;
        let mut hidden = 0u32;
        for (index, row) in rows.iter().enumerate() {
            if row.token == token {
                return Ok((index as u32).wrapping_sub(hidden));
            }
            if let (Some(states), OrdinalMode::Visible) = (states, mode) {
                if states[index] == RowState::Hidden {
                    hidden += 1;
                }
            }
        }
        Ok(0)
    }

    fn visible_ordinal_at(&self, row: RowIndex) -> Result<VisibleOrdinal, NotOwned> {
        let token = Self::token_at(self.current()?, row)?;
        Ok(VisibleOrdinal::new(self.ordinal(token, OrdinalMode::Visible)?))
    }

    /// `0xbf9b1c`.
    fn step(&mut self, direction: Direction) -> Result<bool, NotOwned> {
        let node = self.current()?;
        let last = self.state.total.get().wrapping_sub(1);
        if last == 0 {
            return Ok(false);
        }
        let mut tries = 0u32;
        let mut row = self.state.live.cursor.get();
        loop {
            match direction {
                Direction::Up if row == 0 => {
                    row = last;
                    if !R::wraps(node) {
                        return Ok(false);
                    }
                }
                Direction::Up => row -= 1,
                Direction::Down if row < last => row += 1,
                Direction::Down => {
                    if !R::wraps(node) {
                        return Ok(false);
                    }
                    row = 0;
                }
            }
            if node.availability == Availability::Fixed || R::selectable(self.row_state(RowIndex::new(row))?) {
                break;
            }
            tries += 1;
            if last <= tries {
                return Ok(false);
            }
        }
        let cursor = RowIndex::new(row);
        self.state.live.cursor = cursor;
        self.state.live.ordinal = self.visible_ordinal_at(cursor)?;
        self.state.live.count = VisibleCount::new(self.count(CountMode::Visible)?);
        Ok(true)
    }

    /// `0xbf988c`.
    fn scroll(&self, top: ScrollTop, ordinal: VisibleOrdinal, rule: ScrollRule) -> Result<ScrollTop, NotOwned> {
        let page = self.current()?.rows_per_page.get();
        let top = top.get();
        let new = ordinal.get();
        if self.state.total.get() < page || !R::SCROLL {
            return Ok(ScrollTop::new(0));
        }
        let on_page = new <= page.wrapping_sub(1).wrapping_add(top);
        let from_bottom = 1u32.wrapping_sub(page).wrapping_add(new);
        let scrolled = match rule {
            ScrollRule::Down if new < top && on_page => {
                if page <= new { from_bottom } else { 0 }
            }
            ScrollRule::Down if new >= top && on_page => top,
            ScrollRule::Down => from_bottom,
            ScrollRule::Up if new < top && on_page => new,
            ScrollRule::Up if new >= top && on_page => top,
            ScrollRule::Up => self.count(CountMode::Visible)?.wrapping_sub(page),
            ScrollRule::Open if page <= new => from_bottom,
            ScrollRule::Open => 0,
        };
        Ok(ScrollTop::new(scrolled))
    }

    fn view(&self) -> View<'_> {
        View { tree: self.tree, state: &self.state }
    }

    /// The cursor indexes at most the terminator, so `classify` reads only tree
    /// words.
    fn check_cursor(&self) -> Result<(), NotOwned> {
        Self::row(self.current()?, self.state.live.cursor).map(|_| ())
    }

    /// The saved cursor also indexes at most the terminator, so `fill` reads
    /// only tree words.
    fn check_view(&self) -> Result<(), NotOwned> {
        self.check_cursor()?;
        Self::row(self.current()?, self.state.saved.cursor).map(|_| ())
    }

    /// `FUN_0061cd14` of the in-progress state.
    fn snapshot(&self) -> Result<MenuFrameSnapshot, NotOwned> {
        self.check_view()?;
        let mut record = MenuFrameSnapshot::DETACHED;
        frame::fill(&self.view(), &mut record);
        Ok(record)
    }

    fn perform(&mut self, effect: Effect) {
        self.called = true;
        self.services.perform(&self.state, effect);
    }

    /// `FUN_0061ceac`.
    fn store(&mut self, slot: SnapshotSlot) {
        match self.snapshot() {
            Ok(record) if !self.delegated => self.perform(Effect::StoreSnapshot { depth: record.depth, slot, record }),
            _ => self.perform(Effect::StoreFromBlock { slot }),
        }
    }

    fn notify(&mut self, kind: StageKind, node: StageNode, id: StageId, class: MenuClass) {
        self.perform(Effect::Notify(Notify { kind, node, id, class }));
    }

    fn availability(&mut self, class: MenuClass, token: RowToken) -> RowState {
        self.called = true;
        self.services.availability(&self.state, class, token)
    }

    fn initial_row(&mut self, kind: StageKind, node: StageNode, class: MenuClass) -> Option<RowToken> {
        self.called = true;
        self.services.initial_row(&self.state, kind, node, class)
    }

    /// The visible ordinal of the cursor's token; a restored cursor may lie
    /// past the terminator.
    fn open_cursor_ordinal(&mut self) -> Result<VisibleOrdinal, NotOwned> {
        let node = self.current()?;
        let cursor = self.state.live.cursor;
        let token = if cursor.get() as usize > Self::rows(node)?.len() {
            self.called = true;
            self.services.token_past_rows(&self.state, cursor)
        } else {
            Self::token_at(node, cursor)?
        };
        Ok(VisibleOrdinal::new(self.ordinal(token, OrdinalMode::Visible)?))
    }

    fn kind_switch(&mut self, call: KindSwitchCall) -> EngineResult {
        self.called = true;
        self.delegated = true;
        let (state, result) = self.services.kind_switch(&self.state, call);
        self.state = state;
        result
    }

    /// `0xbfad04` after an open that succeeded. Its arms other than the
    /// default return 1 here unless the open node has type 5 or 7, which only
    /// a nested pop leaves possible; stock then runs it.
    fn seek_framed(
        &mut self,
        kind: StageKind,
        node: StageNode,
        id: StageId,
        seek: StageId,
        target: StageExtra,
    ) -> Result<EngineResult, NotOwned> {
        let current = self.current()?;
        if !framed(current) {
            return Ok(EngineResult::Accepted);
        }
        self.called = true;
        self.delegated = true;
        let call = SeekCall { kind, node, node_type: current.node_type, id, seek, target };
        let (state, result) = self.services.seek(&self.state, call);
        self.state = state;
        Ok(result)
    }

    /// `0xbf976c`.
    fn set_node(&mut self, node: NodeRef) {
        self.state.node = Some(node);
        self.state.word10 = self.tree.node(node).word10;
    }

    fn remember_branch(&self, node: &Node) -> RememberBranch {
        match (node.remember, node.parent) {
            (Remember::Off, _) => RememberBranch::Off,
            (Remember::On, None) => RememberBranch::NoParent,
            (Remember::On, Some(parent)) if self.tree.node(parent).id != node.id => RememberBranch::OtherParentId,
            (Remember::On, Some(_)) => RememberBranch::SameParentId,
        }
    }

    /// `0xbf9a5c` for a node index other than `0x7a`.
    fn remember(&mut self, index: NodeIndex) -> Result<(), NotOwned> {
        let node = self.current()?;
        match self.remember_branch(node) {
            RememberBranch::Off | RememberBranch::SameParentId => {}
            RememberBranch::NoParent | RememberBranch::OtherParentId => {
                if node.rows.is_some() {
                    self.state.remembered[index.index()] = Self::token_at(node, self.state.live.cursor)?;
                }
            }
        }
        Ok(())
    }

    /// What `open_rows` needs of the node it opens and of the cursor it keeps
    /// when the initial row gives no selection.
    fn check_open(target: &Node, restored: RowIndex) -> Result<(), NotOwned> {
        if framed(target) {
            return Err(NotOwned::FramedNode);
        }
        if target.wrap != Wrap::Wraps || target.availability != Availability::Queried {
            return Err(NotOwned::NodeFlags);
        }
        let rows = Self::rows(target)?.len();
        if rows == 0 {
            return Err(NotOwned::EmptyNode);
        }
        if rows > ROW_STATES || restored.get() as usize >= ROW_STATES {
            return Err(NotOwned::RowBounds);
        }
        Ok(())
    }

    /// `0xbfa800` for kinds other than `0x13`, on a node `check_open` passed.
    fn open_rows(&mut self, kind: StageKind, node_word: StageNode) -> Result<bool, NotOwned> {
        let node = self.current()?;
        let rows = Self::rows(node)?;
        let total = self.count(CountMode::All)?;
        self.state.total = RowCount::new(total);
        self.state.row_states = [RowState::Offered; ROW_STATES];
        for row in &rows[..total as usize] {
            let answer = self.availability(node.class, row.token);
            if let Some(first) = rows[..total as usize].iter().position(|other| other.token == row.token) {
                self.state.row_states[first] = answer;
            }
        }
        self.state.visible_at_open = VisibleCount::new(self.count(CountMode::Visible)?);
        if !self.state.row_states[..total as usize].iter().any(|&state| R::selectable(state)) {
            if node.parent.is_none() {
                return Ok(false);
            }
            let call =
                KindSwitchCall { kind: StageKind::Pop, node: node_word, id: StageId::NESTED_POP, extra: StageExtra::new(0) };
            return Ok(self.kind_switch(call) == EngineResult::Accepted);
        }

        let answer = self.initial_row(kind, node_word, node.class);
        if let (Some(token), true) = (answer, R::INITIAL_ROW) {
            self.state.live.cursor = RowIndex::new(self.ordinal(token, OrdinalMode::Index)?);
            self.state.live.ordinal = VisibleOrdinal::new(self.ordinal(token, OrdinalMode::Visible)?);
        }
        let result = if R::selectable(self.row_state(self.state.live.cursor)?) {
            true
        } else {
            if kind == StageKind::Open {
                self.state.live.cursor = RowIndex::new(self.state.total.get().wrapping_sub(1));
            }
            let stepped = self.step(Direction::Down)?;
            R::selectable(self.row_state(self.state.live.cursor)?) || stepped
        };
        let ordinal = self.open_cursor_ordinal()?;
        self.state.live.scroll = self.scroll(ScrollTop::new(0), ordinal, ScrollRule::Open)?;
        self.state.saved = self.state.live;
        self.state.live.ordinal = self.open_cursor_ordinal()?;
        self.state.live.count = VisibleCount::new(self.count(CountMode::Visible)?);
        Ok(result)
    }

    fn stage_index(node: StageNode) -> Result<NodeIndex, NotOwned> {
        if node == PROPERTY_NODE {
            return Err(NotOwned::StageNode);
        }
        NodeIndex::parse(node.get()).ok_or(NotOwned::StageNode)
    }

    /// Kinds 4 and 5 with a row step: `0xbf9f44`.
    fn move_rows(&mut self, direction: Direction, node_word: StageNode, count: StepCount) -> Result<EngineResult, NotOwned> {
        let node = self.current()?;
        if node.wrap != Wrap::Wraps || node.availability != Availability::Queried {
            return Err(NotOwned::NodeFlags);
        }
        if count.get() == 0 {
            return Err(NotOwned::EventClass);
        }
        let (kind, rule) = match direction {
            Direction::Down => (StageKind::MoveDown, ScrollRule::Down),
            Direction::Up => (StageKind::MoveUp, ScrollRule::Up),
        };
        let old_cursor = self.state.live.cursor;
        let old_scroll = self.state.live.scroll;
        let mut steps = 1u32;
        loop {
            if !self.step(direction)? {
                return Ok(EngineResult::Rejected);
            }
            let cursor = self.state.live.cursor;
            let passed_start = match direction {
                Direction::Down => cursor <= old_cursor,
                Direction::Up => old_cursor <= cursor,
            };
            if passed_start || count.get() <= steps {
                break;
            }
            steps += 1;
        }
        let ordinal = self.visible_ordinal_at(self.state.live.cursor)?;
        self.state.live.scroll = self.scroll(old_scroll, ordinal, rule)?;
        let record = self.snapshot()?;
        self.notify(kind, node_word, StageId::VALUE, node.class);
        if R::LAST_MOVE {
            self.state.last_move = LastMove { class: record.class, token: record.cursor_token };
        }
        self.store(SnapshotSlot::new(node_word.get()));
        Ok(EngineResult::Accepted)
    }

    /// Kinds `0xe` and `0xf` with an id other than `0x17` and `0x18`:
    /// `0xbfab98`, then `0xbfad04`.
    fn push_pop(&mut self, kind: StageKind, node_word: StageNode, id: StageId) -> Result<EngineResult, NotOwned> {
        if id == StageId::SEEK || id == StageId::SEEK_PUSH {
            return Err(NotOwned::EventClass);
        }
        let index = Self::stage_index(node_word)?;
        let source = self.current()?;
        let old_class = source.class;
        let depth = self.state.depth.get() as usize;
        match kind {
            StageKind::Push => {
                if self.remember_branch(source) != RememberBranch::OtherParentId {
                    return Err(NotOwned::RememberBranch);
                }
                let child = Self::row(source, self.state.live.cursor)?.and_then(|row| row.child).ok_or(NotOwned::NoChild)?;
                if depth + 1 >= STACK_FRAMES {
                    return Err(NotOwned::StackBounds);
                }
                Self::check_open(self.tree.node(child), self.state.stack[depth + 1].cursor)?;
                self.remember(index)?;
                self.check_cursor()?;
                if R::PUSH_CLASSIFIER {
                    self.state.push_classifier = frame::classify(&self.view());
                }
                self.set_node(child);
                self.state.stack[depth] = self.state.live;
                self.state.live = self.state.stack[depth + 1];
                self.state.depth = StackDepth::new(depth as u32 + 1);
            }
            _ => {
                if self.remember_branch(source) != RememberBranch::SameParentId {
                    return Err(NotOwned::RememberBranch);
                }
                let parent = source.parent.ok_or(NotOwned::NoParent)?;
                if depth == 0 || depth >= STACK_FRAMES {
                    return Err(NotOwned::StackBounds);
                }
                Self::check_open(self.tree.node(parent), self.state.stack[depth - 1].cursor)?;
                self.remember(index)?;
                self.set_node(parent);
                self.state.stack[depth] = self.state.live;
                self.state.live = self.state.stack[depth - 1];
                self.state.depth = StackDepth::new(depth as u32 - 1);
            }
        }
        let new_id = StageNode::from(self.current()?.id);
        self.notify(kind, new_id, id, old_class);
        let opened = self.open_rows(kind, new_id)?;
        self.store(SnapshotSlot::new(new_id.get()));
        if !opened {
            return Ok(EngineResult::Rejected);
        }
        let current = StageNode::from(self.current()?.id);
        self.seek_framed(kind, current, id, StageId::NO_SEEK, StageExtra::new(0))
    }

    /// Kind 1.
    fn open(&mut self, node_word: StageNode, id: StageId, extra: StageExtra) -> Result<EngineResult, NotOwned> {
        if id.seeks() {
            return Err(NotOwned::EventClass);
        }
        if node_word == SEEK_CLASS_NODE {
            return Err(NotOwned::StageNode);
        }
        let index = NodeIndex::parse(node_word.get()).ok_or(NotOwned::StageNode)?;
        let target = self.tree.registered(index).ok_or(NotOwned::StageNode)?;
        self.state.menu_open = MenuOpen::Open;
        self.set_node(target);
        self.state.depth = StackDepth::new(1);
        self.state.stack = [CursorFrame::ZERO; STACK_FRAMES];
        self.state.live = CursorFrame::ZERO;
        self.state.total = RowCount::new(self.count(CountMode::All)?);
        let remembered = self.state.remembered[index.index()];
        self.state.live.cursor = RowIndex::new(self.ordinal(remembered, OrdinalMode::Index)?);
        Self::check_open(self.tree.node(target), self.state.live.cursor)?;
        if self.open_rows(StageKind::Open, node_word)? {
            let class = self.current()?.class;
            self.notify(StageKind::Open, node_word, StageId::VALUE, class);
            if self.seek_framed(StageKind::Open, node_word, StageId::VALUE, id, extra)? == EngineResult::Accepted {
                return Ok(EngineResult::Accepted);
            }
        }
        self.state.menu_open = MenuOpen::Closed;
        Ok(EngineResult::Rejected)
    }
}

/// Node types for which `0xbfad04` pushes or pops again.
const FRAMED_TYPES: [NodeType; 2] = [NodeType::new(5), NodeType::new(7)];

/// The frame cells of an in-progress state.
struct View<'a> {
    tree: &'a Tree,
    state: &'a EngineState,
}

#[derive(Clone, Copy)]
struct NodeView<'a> {
    tree: &'a Tree,
    node: &'a Node,
}

#[derive(Clone, Copy)]
struct RowsView(Option<&'static [Row]>);

impl RowValues for RowsView {
    fn is_present(self) -> bool {
        self.0.is_some()
    }

    fn token(self, row: RowIndex) -> RowToken {
        self.0.and_then(|rows| rows.get(row.get() as usize)).map_or(RowToken::TERMINATOR, |row| row.token)
    }

    fn has_child(self, row: RowIndex) -> bool {
        self.0.and_then(|rows| rows.get(row.get() as usize)).is_some_and(|row| row.child.is_some())
    }
}

impl<'a> FrameNode for NodeView<'a> {
    type Values = RowsView;

    fn id(self) -> NodeId {
        self.node.id
    }

    fn node_type(self) -> NodeType {
        self.node.node_type
    }

    fn class(self) -> MenuClass {
        self.node.class
    }

    fn rows_per_page(self) -> RowsPerPage {
        self.node.rows_per_page
    }

    fn parent(self) -> Option<Self> {
        self.node.parent.map(|parent| NodeView { tree: self.tree, node: self.tree.node(parent) })
    }

    fn values(self) -> RowsView {
        RowsView(self.node.rows)
    }
}

impl<'a> FrameCells for View<'a> {
    type Node = NodeView<'a>;

    fn current(&self) -> Option<NodeView<'a>> {
        self.state.node.map(|node| NodeView { tree: self.tree, node: self.tree.node(node) })
    }

    fn reload(&self) -> NodeView<'a> {
        self.current().expect("checked by check_view")
    }

    fn cursor(&self) -> RowIndex {
        self.state.live.cursor
    }

    fn visible_ordinal(&self) -> VisibleOrdinal {
        self.state.live.ordinal
    }

    fn visible_count(&self) -> VisibleCount {
        self.state.live.count
    }

    fn scroll_top(&self) -> ScrollTop {
        self.state.live.scroll
    }

    fn saved_cursor(&self) -> RowIndex {
        self.state.saved.cursor
    }

    /// A zero `RowsPerPage` enters the panic handler where stock enters the
    /// loader's fatal divide.
    fn page(&self, rows: RowsPerPage, visible: VisibleOrdinal) -> PageNumber {
        PageNumber::new(rows.get().wrapping_add(visible.get()) / rows.get())
    }
}
