//! The engine wrapper `0xbf95c8`: the kind switch `0xbfa234`, then the
//! snapshot store at the open node's record id. Owned classes, each with an
//! oracle case:
//!
//! - kinds 4 and 5, a row step of any nonzero count other than a page;
//! - kinds `0x45`..`0x48`, the group moves of `0xbf9c1c`;
//! - kind 2 with an open node;
//! - kinds `0xe` and `0xf`, no seek id, into a node of type other than 5 or 7;
//! - kinds 6..`0xb`, whose inner pushes, pops and moves are those of kinds
//!   `0xe`, `0xf`, 4 and 5, the seek of `0xa` and `0xb` on a node of type
//!   other than 5 or 7;
//! - kind 1 on a registered node other than `0x38` of type other than 5 or 7,
//!   with an id the `0xbfad04` switch passes to its default arm;
//! - kind `0x13` on the open node.
//!
//! Every node a push, pop, open or re-open opens (`0xbfa800`) needs 1..=`ROW_STATES`
//! rows and a restored cursor below `ROW_STATES`, and `apply` decides this for
//! each node an event can reach before its first service call. When no row is
//! selectable after the availability answers, the engine takes `0xbfa800`'s
//! empty arm: it returns 0 at a node without a parent, and otherwise calls the
//! stock kind switch for the nested pop as stock does.

use core::ops::DerefMut;

use super::frame::{
    self, FrameCells, FrameDepth, FrameNode, MenuClass, MenuFrameSnapshot, PageNumber, RowIndex, RowToken, RowValues,
    RowsPerPage, ScrollTop, VisibleCount, VisibleOrdinal,
};
use super::frame::{word, NodeId, NodeType};
use super::state::{CursorFrame, EngineState, LastMove, MenuOpen, RowCount, RowState, StackDepth, ROW_STATES, STACK_FRAMES};
use super::tree::{Availability, Node, NodeIndex, NodeRef, NodeWord10, Remember, Row, RowChild, Tree, Wrap};

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
            Some(open) if tree.node(open).is_some_and(|node| node.rows_per_page.get() == raw.id) && raw.node != 0 => Step::Page,
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
    NoValues,
    /// A row index past the rows array, a row-state index past `ROW_STATES`
    /// or a node with more rows than `ROW_STATES`.
    RowBounds,
    StackBounds,
    /// A stage node outside the registry or one with a node-specific branch.
    StageNode,
    /// `0xbfad04` pushes or pops again for a node of type 5 or 7.
    FramedNode,
    NoChild,
    /// A row whose child is a package node outside the tree.
    PackageChild,
    /// The block's total rows differ from the open node's rows in the tree.
    RowTotal,
    NoParent,
    /// A node without rows, which `0xbfa800` treats as empty.
    EmptyNode,
    /// A `NodeRef` past the tree's nodes.
    TreeBounds,
    /// An inner move of kinds 6..`0xb` that `0xbf9f44` takes as a page step
    /// or refuses for a zero id.
    InnerMove,
    /// A seek of kinds `0xa` and `0xb` that steps.
    SeekSteps,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Notify {
    pub kind: StageKind,
    pub node: StageNode,
    pub id: StageId,
    pub class: MenuClass,
}

word!(
    /// A settings property `FUN_0061c91c` writes.
    PropertyId
);

impl PropertyId {
    /// The property `0xbf9a5c(0x7a)` writes with the remembered row's index.
    pub const REMEMBERED_7A: Self = Self::new(0x285);
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
    /// The installed bank's `rb_bank_page_entry` work at each call of
    /// `0xbfa800` (`menu/recipe_bank_arm.S`), before `0xbfa800` runs.
    PageEntry,
    /// `FUN_0061c91c(property, value, 0)`.
    PropertyWrite { property: PropertyId, value: RowIndex },
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

/// What the engine asks of stock, in stock's order. Before each call the
/// adapter writes `state` to the block. `kind_switch` and `seek` tail-call
/// stock, decode the block afterwards back into `state` and return the
/// function's result, which stock limits to 0 and 1. Every call comes after
/// `apply` has committed to the event, so an answer or block that does not
/// decode is a run-failing guard event in the adapter.
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
    fn kind_switch(&mut self, state: &mut EngineState, call: KindSwitchCall) -> EngineResult;
    fn seek(&mut self, state: &mut EngineState, call: SeekCall) -> EngineResult;
    /// A check that holds before the first call failed after it.
    fn invariant_failure(&mut self) -> !;
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
    const DOWN_BOTTOM_ARM: bool = true;
    const OPEN_BOTTOM_ARM: bool = true;
    /// `0xbf9a5c` returns for a node whose `Remember` is off.
    const REMEMBER_FLAG: bool = true;
    /// `0xbf9a5c` skips the write when the parent has the node's id.
    const REMEMBER_PARENT_ID: bool = true;
    /// A pop runs `0xbf9a5c`'s write.
    const POP_REMEMBERS: bool = true;
    /// Kind 2 clears `0x009575fc`.
    const CLOSE_CLEARS_MENU_OPEN: bool = true;
    /// The inner steps of kinds 6..`0xb` notify with id 0.
    const FRAME_NOTIFY_ID_ZERO: bool = true;
    /// Kinds `0xa` and `0xb` run `0xbfad04`'s seek loop.
    const SEEK_STEPS: bool = true;
    /// `0xbfa800` asks no availability on a node with fixed availability.
    const FIXED_UNQUERIED: bool = true;
    /// `0xbf9a5c(0x7a)` writes property `0x285`.
    const PROPERTY_WRITE: bool = true;
    /// Kinds `0x45`..`0x48` move by `0xbf9c1c`.
    const GROUP_ARMS: bool = true;
    /// Kind `0x13` makes no notification.
    const REOPEN_SILENT: bool = true;
    /// Kinds 4 and 5 repeat the step `id` times.
    const STEP_COUNT: bool = true;
    /// The repeated step stops once the cursor passes its start.
    const PASS_STOP: bool = true;
}

pub(crate) struct Stock;

impl Rules for Stock {}

/// `0xbfad04` writes `0x009573e4` for this node index and class `0x228`.
const SEEK_CLASS_NODE: StageNode = StageNode::new(0x38);
/// `0xbf9a5c` writes property `0x285` for this node index.
const PROPERTY_NODE: NodeIndex = match NodeIndex::parse(0x7a) {
    Some(index) => index,
    None => panic!(),
};

/// Runs `event` on `state` in place. Returns `NotOwned` only before its
/// first service call, leaving `state` partly changed; the block is then
/// still what stock last left there.
pub fn apply<S: EngineServices, P: DerefMut<Target = EngineState>>(
    tree: &Tree,
    state: P,
    event: MenuEvent,
    services: &mut S,
) -> Result<EngineResult, NotOwned> {
    apply_with::<Stock, S, P>(tree, state, event, services)
}

pub(crate) fn apply_with<R: Rules, S: EngineServices, P: DerefMut<Target = EngineState>>(
    tree: &Tree,
    state: P,
    event: MenuEvent,
    services: &mut S,
) -> Result<EngineResult, NotOwned> {
    let mut machine = Machine::<R, S, P> {
        tree,
        state,
        services,
        called: false,
        delegated: false,
        rules: core::marker::PhantomData,
    };
    match machine.run(event) {
        Err(_) if machine.called => machine.services.invariant_failure(),
        result => result,
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
    /// Kinds `0x45`..`0x48`: the top stays.
    Keep,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RememberBranch {
    Off,
    NoParent,
    OtherParentId,
    SameParentId,
}

/// The step `0xbfab98` makes: a push into the cursor row's child or a pop to
/// the parent.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Switch {
    Push,
    Pop,
}

impl Switch {
    const fn kind(self) -> StageKind {
        match self {
            Self::Push => StageKind::Push,
            Self::Pop => StageKind::Pop,
        }
    }
}

/// `0xbfa800`'s kind argument and what it skips.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OpenMode {
    /// Kinds 1, `0xe` and `0xf`.
    Open(StageKind),
    /// Kind `0x13`: no scroll and no saved frame.
    Reopen,
}

impl OpenMode {
    const fn kind(self) -> StageKind {
        match self {
            Self::Open(kind) => kind,
            Self::Reopen => StageKind::Reopen,
        }
    }
}

/// `P` is the in-progress state: a borrow on the host, and in the guest a
/// zero-sized handle on the adapter's static state.
struct Machine<'a, R, S, P> {
    tree: &'a Tree,
    state: P,
    services: &'a mut S,
    called: bool,
    /// A stock call has replaced `state`.
    delegated: bool,
    rules: core::marker::PhantomData<R>,
}

fn framed(node: &Node) -> bool {
    FRAMED_TYPES.contains(&node.node_type)
}

/// Whether a node `0xbfad04` would push or pop again may be opened.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FramedTarget {
    /// `0xbfad04` follows the open.
    Refused,
    /// An inner step of kinds 6..`0xb`, which no `0xbfad04` follows.
    Allowed,
}

/// Kinds `0xa` and `0xb` whose seek makes steps.
const SEEK_STEPS_OWNED: bool = true;

impl<'a, R: Rules, S: EngineServices, P: DerefMut<Target = EngineState>> Machine<'a, R, S, P> {
    fn run(&mut self, event: MenuEvent) -> Result<EngineResult, NotOwned> {
        if let Some(open) = self.state.node {
            if self.node(open)?.rows.map_or(0, <[Row]>::len) != self.state.total.get() as usize {
                return Err(NotOwned::RowTotal);
            }
        }
        let result = match event {
            MenuEvent::Move { direction, step: Step::Rows(count), node } => self.move_rows(direction, node, count)?,
            MenuEvent::GroupMove { kind, node, .. } => self.group_move(kind, node)?,
            MenuEvent::Push { node, id, .. } => self.push_pop(Switch::Push, node, id)?,
            MenuEvent::Pop { node, id, .. } => self.push_pop(Switch::Pop, node, id)?,
            MenuEvent::Open { node, id, extra } => self.open(node, id, extra)?,
            MenuEvent::Close { node, id } => self.close(node, id)?,
            MenuEvent::FrameMove { kind, node, id, extra } => self.frame_move(kind, node, id, extra)?,
            MenuEvent::Reopen { node } => self.reopen(node)?,
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
        self.node(self.current_ref()?)
    }

    fn node(&self, node: NodeRef) -> Result<&'a Node, NotOwned> {
        self.tree.node(node).ok_or(NotOwned::TreeBounds)
    }

    fn rows(node: &Node) -> Result<&'static [Row], NotOwned> {
        node.rows.ok_or(NotOwned::NoValues)
    }

    /// `values[row]`; the row after the last reads the terminator pair.
    fn row(node: &Node, row: RowIndex) -> Result<Option<&'static Row>, NotOwned> {
        let rows = Self::rows(node)?;
        match rows.get(row.get() as usize) {
            Some(entry) => Ok(Some(entry)),
            None if row.get() as usize == rows.len() => Ok(None),
            None => Err(NotOwned::RowBounds),
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
        Ok(self.state.row_states.get(..rows))
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
                if states.get(index) == Some(&RowState::Hidden) {
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
            ScrollRule::Down if R::DOWN_BOTTOM_ARM => from_bottom,
            ScrollRule::Down => top,
            ScrollRule::Up if new < top && on_page => new,
            ScrollRule::Up if new >= top && on_page => top,
            ScrollRule::Up => self.count(CountMode::Visible)?.wrapping_sub(page),
            ScrollRule::Open if page <= new && R::OPEN_BOTTOM_ARM => from_bottom,
            ScrollRule::Open => 0,
            ScrollRule::Keep => top,
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
        self.services.kind_switch(&mut self.state, call)
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
        Ok(self.services.seek(&mut self.state, call))
    }

    /// `0xbf976c`.
    fn set_node(&mut self, node: NodeRef) -> Result<(), NotOwned> {
        let word10 = self.node(node)?.word10;
        self.state.node = Some(node);
        self.state.word10 = word10;
        Ok(())
    }

    fn remember_branch(&self, node: &Node) -> Result<RememberBranch, NotOwned> {
        Ok(match (node.remember, node.parent) {
            (Remember::Off, _) if R::REMEMBER_FLAG => RememberBranch::Off,
            (_, None) => RememberBranch::NoParent,
            (_, Some(parent)) if self.node(parent)?.id != node.id => RememberBranch::OtherParentId,
            (_, Some(_)) => RememberBranch::SameParentId,
        })
    }

    /// `0xbf9a5c(index)` on the open node, run by kind 2 and each push and pop.
    fn remember(&mut self, index: NodeIndex, switch: Option<Switch>) -> Result<(), NotOwned> {
        let node = self.current()?;
        let writes = match self.remember_branch(node)? {
            RememberBranch::Off => return Ok(()),
            RememberBranch::SameParentId => !R::REMEMBER_PARENT_ID,
            RememberBranch::NoParent | RememberBranch::OtherParentId => true,
        };
        if writes && node.rows.is_some() && (R::POP_REMEMBERS || switch != Some(Switch::Pop)) {
            let token = Self::token_at(node, self.state.live.cursor)?;
            *self.state.remembered.get_mut(index.index()).ok_or(NotOwned::StageNode)? = token;
        }
        if index == PROPERTY_NODE && R::PROPERTY_WRITE {
            let remembered = *self.state.remembered.get(index.index()).ok_or(NotOwned::StageNode)?;
            let value = RowIndex::new(self.ordinal(remembered, OrdinalMode::Index)?);
            self.perform(Effect::PropertyWrite { property: PropertyId::REMEMBERED_7A, value });
        }
        Ok(())
    }

    /// What `open_rows` needs of the node it opens and of the cursor it keeps
    /// when the initial row gives no selection.
    fn check_open(target: &Node, restored: RowIndex, framed_target: FramedTarget) -> Result<(), NotOwned> {
        if framed_target == FramedTarget::Refused && framed(target) {
            return Err(NotOwned::FramedNode);
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

    /// `0xbfa800(kind, node, direction)` on a node `check_open` passed.
    fn open_rows(&mut self, mode: OpenMode, node_word: StageNode, direction: Direction) -> Result<bool, NotOwned> {
        self.perform(Effect::PageEntry);
        let node = self.current()?;
        let rows = Self::rows(node)?;
        let total = self.count(CountMode::All)?;
        self.state.total = RowCount::new(total);
        let fixed = node.availability == Availability::Fixed;
        self.state.row_states = [RowState::Offered; ROW_STATES];
        if !fixed || !R::FIXED_UNQUERIED {
            for row in rows.iter().take(total as usize) {
                let answer = self.availability(node.class, row.token);
                let first = rows.iter().position(|other| other.token == row.token);
                if let Some(slot) = first.and_then(|first| self.state.row_states.get_mut(first)) {
                    *slot = answer;
                }
            }
            self.state.visible_at_open = VisibleCount::new(self.count(CountMode::Visible)?);
            if !self.state.row_states.iter().take(total as usize).any(|&state| R::selectable(state)) {
                if node.parent.is_none() {
                    return Ok(false);
                }
                let call =
                    KindSwitchCall { kind: StageKind::Pop, node: node_word, id: StageId::NESTED_POP, extra: StageExtra::new(0) };
                return Ok(self.kind_switch(call) == EngineResult::Accepted);
            }
        }

        let answer = self.initial_row(mode.kind(), node_word, node.class);
        if let (Some(token), true) = (answer, R::INITIAL_ROW) {
            self.state.live.cursor = RowIndex::new(self.ordinal(token, OrdinalMode::Index)?);
            self.state.live.ordinal = VisibleOrdinal::new(self.ordinal(token, OrdinalMode::Visible)?);
        }
        let result = if fixed || R::selectable(self.row_state(self.state.live.cursor)?) {
            true
        } else {
            if mode == OpenMode::Open(StageKind::Open) {
                self.state.live.cursor = RowIndex::new(self.state.total.get().wrapping_sub(1));
            }
            let stepped = self.step(direction)?;
            R::selectable(self.row_state(self.state.live.cursor)?) || stepped
        };
        if mode != OpenMode::Reopen {
            let ordinal = self.open_cursor_ordinal()?;
            self.state.live.scroll = self.scroll(ScrollTop::new(0), ordinal, ScrollRule::Open)?;
            self.state.saved = self.state.live;
        }
        self.state.live.ordinal = self.open_cursor_ordinal()?;
        self.state.live.count = VisibleCount::new(self.count(CountMode::Visible)?);
        Ok(result)
    }

    fn stage_index(node: StageNode) -> Result<NodeIndex, NotOwned> {
        NodeIndex::parse(node.get()).ok_or(NotOwned::StageNode)
    }

    /// `0xbf9f44`'s arm for an inner move of kinds 6..`0xb` is a row step.
    fn check_inner_move(node: &Node, node_word: StageNode, id: StageId) -> Result<(), NotOwned> {
        if id.get() == 0 || (node.rows_per_page.get() == id.get() && node_word.get() != 0) {
            return Err(NotOwned::InnerMove);
        }
        Ok(())
    }

    /// `0xbf9f44`'s tail: scroll, notify, the last-move pair and the snapshot
    /// store at the event's node.
    fn move_tail(&mut self, kind: StageKind, node_word: StageNode, old_scroll: ScrollTop, rule: ScrollRule) -> Result<EngineResult, NotOwned> {
        let node = self.current()?;
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

    /// Kinds 4 and 5 with a row step: `0xbf9f44`.
    fn move_rows(&mut self, direction: Direction, node_word: StageNode, count: StepCount) -> Result<EngineResult, NotOwned> {
        if count.get() == 0 {
            return Err(NotOwned::EventClass);
        }
        let (kind, rule) = match direction {
            Direction::Down => (StageKind::MoveDown, ScrollRule::Down),
            Direction::Up => (StageKind::MoveUp, ScrollRule::Up),
        };
        let count = if R::STEP_COUNT { count.get() } else { 1 };
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
            if (passed_start && R::PASS_STOP) || count <= steps {
                break;
            }
            steps += 1;
        }
        self.move_tail(kind, node_word, old_scroll, rule)
    }

    fn accept_row(&mut self, row: u32) -> Result<bool, NotOwned> {
        let cursor = RowIndex::new(row);
        self.state.live.cursor = cursor;
        self.state.live.ordinal = self.visible_ordinal_at(cursor)?;
        self.state.live.count = VisibleCount::new(self.count(CountMode::Visible)?);
        Ok(true)
    }

    fn hidden(&self, row: u32) -> Result<bool, NotOwned> {
        Ok(self.row_state(RowIndex::new(row))? == RowState::Hidden)
    }

    fn row_selectable(&self, row: u32) -> Result<bool, NotOwned> {
        Ok(R::selectable(self.row_state(RowIndex::new(row))?))
    }

    /// `0xbf9c1c`: kinds `0x47` and `0x48` step to the fourth row not hidden,
    /// with wrap; kinds `0x45` and `0x46` move within a group of four visible
    /// ordinals.
    fn group_step(&mut self, kind: StageKind) -> Result<bool, NotOwned> {
        let node = self.current()?;
        let total = self.state.total.get();
        let mut cursor = self.state.live.cursor.get();
        match kind {
            StageKind::Move47 | StageKind::Move48 => {
                let last = total.wrapping_sub(1);
                if last == 0 {
                    return Ok(false);
                }
                let mut counted = 0u32;
                let mut tries = 0u32;
                loop {
                    let next = match kind {
                        StageKind::Move47 if cursor != 0 => cursor - 1,
                        StageKind::Move47 => last,
                        _ if cursor < last => cursor + 1,
                        _ => 0,
                    };
                    if !self.hidden(next)? {
                        counted += 1;
                    }
                    if counted == 4 {
                        if self.row_selectable(next)? {
                            return self.accept_row(next);
                        }
                        counted = 0;
                    }
                    tries += 1;
                    cursor = next;
                    if last <= tries {
                        return Ok(false);
                    }
                }
            }
            StageKind::Move46 => {
                for _ in 0..4 {
                    let ordinal = self.ordinal(Self::token_at(node, RowIndex::new(cursor))?, OrdinalMode::Visible)?;
                    let mut next = cursor;
                    if ordinal.wrapping_add(1) & 3 == 0 {
                        for back in (1..=3u32).rev() {
                            let mut row = cursor;
                            for _ in 0..back {
                                row = row.wrapping_sub(1);
                                while self.hidden(row)? {
                                    row = row.wrapping_sub(1);
                                }
                            }
                            if self.row_selectable(row)? {
                                return self.accept_row(row);
                            }
                        }
                    } else if total.wrapping_sub(1) != 0 {
                        next = cursor.wrapping_add(1);
                        if next <= total {
                            let mut gave_up = false;
                            if self.hidden(next)? {
                                loop {
                                    if next == cursor.wrapping_add(total - 1) {
                                        gave_up = true;
                                        break;
                                    }
                                    next += 1;
                                    if total < next {
                                        gave_up = true;
                                        break;
                                    }
                                    if !self.hidden(next)? {
                                        break;
                                    }
                                }
                            }
                            if !gave_up && self.row_selectable(next)? {
                                return self.accept_row(next);
                            }
                        }
                    }
                    cursor = next;
                }
                Ok(false)
            }
            _ => {
                for _ in 0..4 {
                    let ordinal = self.ordinal(Self::token_at(node, RowIndex::new(cursor))?, OrdinalMode::Visible)?;
                    let mut next = cursor;
                    if ordinal & 3 == 0 {
                        for forward in (1..=3u32).rev() {
                            if cursor.wrapping_add(1) < total {
                                let mut seen = 0u32;
                                let mut row = cursor + 1;
                                while row < total {
                                    if !self.hidden(row)? {
                                        seen += 1;
                                        if seen == forward {
                                            if self.row_selectable(row)? {
                                                return self.accept_row(row);
                                            }
                                            break;
                                        }
                                    }
                                    row += 1;
                                }
                            }
                        }
                    } else if total != 1 {
                        next = cursor.wrapping_sub(1);
                        let mut gave_up = false;
                        if self.hidden(next)? {
                            loop {
                                if next == 1u32.wrapping_sub(total).wrapping_add(cursor) {
                                    gave_up = true;
                                    break;
                                }
                                next = next.wrapping_sub(1);
                                if !self.hidden(next)? {
                                    break;
                                }
                            }
                        }
                        if !gave_up && self.row_selectable(next)? {
                            return self.accept_row(next);
                        }
                    }
                    cursor = next;
                }
                Ok(false)
            }
        }
    }

    /// Kinds `0x45`..`0x48`: `0xbf9f44` with `0xbf9c1c`.
    fn group_move(&mut self, kind: StageKind, node_word: StageNode) -> Result<EngineResult, NotOwned> {
        let old_scroll = self.state.live.scroll;
        let moved = if R::GROUP_ARMS {
            self.group_step(kind)?
        } else {
            let direction = if matches!(kind, StageKind::Move45 | StageKind::Move47) { Direction::Up } else { Direction::Down };
            self.step(direction)?
        };
        if !moved {
            return Ok(EngineResult::Rejected);
        }
        self.move_tail(kind, node_word, old_scroll, ScrollRule::Keep)
    }

    /// `0xbfab98(kind, node, direction, notify_id)`: 0 with no call for a
    /// push from a row without a child or a pop from a node without a parent.
    fn switch_node(&mut self, switch: Switch, node_word: StageNode, direction: Direction, notify_id: StageId) -> Result<bool, NotOwned> {
        let index = Self::stage_index(node_word)?;
        let source = self.current()?;
        let old_class = source.class;
        let depth = self.state.depth.get() as usize;
        match switch {
            Switch::Push => {
                let child = match Self::row(source, self.state.live.cursor)?.map(|row| row.child) {
                    Some(RowChild::Node(child)) => child,
                    Some(RowChild::Package) => return Err(NotOwned::PackageChild),
                    Some(RowChild::Leaf) | None => return Ok(false),
                };
                let restored = *self.state.stack.get(depth + 1).ok_or(NotOwned::StackBounds)?;
                self.remember(index, Some(switch))?;
                self.check_cursor()?;
                if R::PUSH_CLASSIFIER {
                    self.state.push_classifier = frame::classify(&self.view());
                }
                self.set_node(child)?;
                *self.state.stack.get_mut(depth).ok_or(NotOwned::StackBounds)? = self.state.live;
                self.state.live = restored;
                self.state.depth = StackDepth::new(depth as u32 + 1);
            }
            Switch::Pop => {
                let Some(parent) = source.parent else {
                    return Ok(false);
                };
                let restored = *depth.checked_sub(1).and_then(|below| self.state.stack.get(below)).ok_or(NotOwned::StackBounds)?;
                if depth >= STACK_FRAMES {
                    return Err(NotOwned::StackBounds);
                }
                self.remember(index, Some(switch))?;
                self.set_node(parent)?;
                *self.state.stack.get_mut(depth).ok_or(NotOwned::StackBounds)? = self.state.live;
                self.state.live = restored;
                self.state.depth = StackDepth::new(depth as u32 - 1);
            }
        }
        let new_id = StageNode::from(self.current()?.id);
        self.notify(switch.kind(), new_id, notify_id, old_class);
        let opened = self.open_rows(OpenMode::Open(switch.kind()), new_id, direction)?;
        self.store(SnapshotSlot::new(new_id.get()));
        Ok(opened)
    }

    /// The checks `switch_node` needs before the first call: a push into the
    /// cursor row's child or a pop to the parent, with the frame each restores.
    fn check_switch(&self, switch: Switch, framed_target: FramedTarget) -> Result<(), NotOwned> {
        let source = self.current()?;
        let depth = self.state.depth.get() as usize;
        self.check_cursor()?;
        match switch {
            Switch::Push => {
                let child = match Self::row(source, self.state.live.cursor)?.map(|row| row.child) {
                    Some(RowChild::Node(child)) => child,
                    Some(RowChild::Package) => return Err(NotOwned::PackageChild),
                    Some(RowChild::Leaf) | None => return Err(NotOwned::NoChild),
                };
                let restored = *self.state.stack.get(depth + 1).ok_or(NotOwned::StackBounds)?;
                Self::check_open(self.node(child)?, restored.cursor, framed_target)
            }
            Switch::Pop => {
                let parent = source.parent.ok_or(NotOwned::NoParent)?;
                let restored = *depth.checked_sub(1).and_then(|below| self.state.stack.get(below)).ok_or(NotOwned::StackBounds)?;
                if depth >= STACK_FRAMES {
                    return Err(NotOwned::StackBounds);
                }
                Self::check_open(self.node(parent)?, restored.cursor, framed_target)
            }
        }
    }

    /// Kinds `0xe` and `0xf` with an id other than `0x17` and `0x18`:
    /// `0xbfab98`, then `0xbfad04`.
    fn push_pop(&mut self, switch: Switch, node_word: StageNode, id: StageId) -> Result<EngineResult, NotOwned> {
        if id == StageId::SEEK || id == StageId::SEEK_PUSH {
            return Err(NotOwned::EventClass);
        }
        Self::stage_index(node_word)?;
        self.check_switch(switch, FramedTarget::Refused)?;
        if !self.switch_node(switch, node_word, Direction::Down, id)? {
            return Ok(EngineResult::Rejected);
        }
        let current = StageNode::from(self.current()?.id);
        self.seek_framed(switch.kind(), current, id, StageId::NO_SEEK, StageExtra::new(0))
    }

    /// A push or pop target of an inner step of kinds 6..`0xb`: a node
    /// `check_open` passes with `restored`, or a row without a child.
    fn check_target(&self, child: RowChild, restored: RowIndex) -> Result<Option<&'a Node>, NotOwned> {
        match child {
            RowChild::Node(child) => {
                let node = self.node(child)?;
                Self::check_open(node, restored, FramedTarget::Allowed)?;
                Ok(Some(node))
            }
            RowChild::Package => Err(NotOwned::PackageChild),
            RowChild::Leaf => Ok(None),
        }
    }

    /// A push into `child` (the cursor row's child when `None`), a move there
    /// and the pop back.
    fn check_push_move_pop(&self, child: Option<RowChild>, node_word: StageNode, id: StageId) -> Result<(), NotOwned> {
        let source = self.current()?;
        let depth = self.state.depth.get() as usize;
        let child = match child {
            Some(child) => child,
            None => Self::row(source, self.state.live.cursor)?.map_or(RowChild::Leaf, |row| row.child),
        };
        if depth + 1 >= STACK_FRAMES {
            return Err(NotOwned::StackBounds);
        }
        let restored = *self.state.stack.get(depth + 1).ok_or(NotOwned::StackBounds)?;
        let Some(target) = self.check_target(child, restored.cursor)? else {
            return Ok(());
        };
        Self::check_inner_move(target, node_word, id)?;
        if let Some(parent) = target.parent {
            let last = RowIndex::new(Self::rows(source)?.len().saturating_sub(1) as u32);
            Self::check_open(self.node(parent)?, last.max(self.state.live.cursor), FramedTarget::Allowed)?;
        }
        Ok(())
    }

    /// Kinds 6..`0xb`: pops, pushes and moves through `0xbfab98` and
    /// `0xbf9f44` with notification id 0, after the seek for `0xa` and `0xb`.
    /// Every node the steps can open is checked before the first call.
    fn frame_move(&mut self, kind: StageKind, node_word: StageNode, id: StageId, extra: StageExtra) -> Result<EngineResult, NotOwned> {
        Self::stage_index(node_word)?;
        self.check_cursor()?;
        let source = self.current()?;
        let depth = self.state.depth.get() as usize;
        let entry = self.state.live;
        let notify_id = if R::FRAME_NOTIFY_ID_ZERO { StageId::new(0) } else { id };
        let count = StepCount::new(id.get());
        let up = matches!(kind, StageKind::PopMoveUpPush | StageKind::PushMoveUpPop | StageKind::SeekPushMoveUpPop);
        let direction = if up { Direction::Up } else { Direction::Down };
        match kind {
            StageKind::PopMoveDownPush | StageKind::PopMoveUpPush => {
                let parent = self.node(source.parent.ok_or(NotOwned::NoParent)?)?;
                if depth == 0 || depth >= STACK_FRAMES {
                    return Err(NotOwned::StackBounds);
                }
                let restored = *self.state.stack.get(depth - 1).ok_or(NotOwned::StackBounds)?;
                Self::check_open(parent, restored.cursor, FramedTarget::Allowed)?;
                Self::check_inner_move(parent, node_word, id)?;
                for row in Self::rows(parent)? {
                    self.check_target(row.child, entry.cursor)?;
                }
                if !self.switch_node(Switch::Pop, node_word, direction, notify_id)? {
                    return Ok(EngineResult::Rejected);
                }
                if self.move_rows(direction, node_word, count)? != EngineResult::Accepted {
                    return Ok(EngineResult::Rejected);
                }
                self.switched(Switch::Push, node_word, Direction::Down, notify_id)
            }
            StageKind::PushMoveDownPop | StageKind::PushMoveUpPop => {
                self.check_push_move_pop(None, node_word, id)?;
                if !self.switch_node(Switch::Push, node_word, Direction::Down, notify_id)? {
                    return Ok(EngineResult::Rejected);
                }
                if self.move_rows(direction, node_word, count)? != EngineResult::Accepted {
                    return Ok(EngineResult::Rejected);
                }
                self.switched(Switch::Pop, node_word, direction, notify_id)
            }
            _ => {
                if framed(source) {
                    return Err(NotOwned::FramedNode);
                }
                let rows = Self::rows(source)?;
                let total = self.state.total.get();
                let zero_step = total == 0 || Self::token_at(source, entry.cursor)?.get() == extra.get();
                if zero_step {
                    self.check_push_move_pop(None, node_word, id)?;
                } else {
                    if !SEEK_STEPS_OWNED {
                        return Err(NotOwned::SeekSteps);
                    }
                    Self::check_inner_move(source, node_word, StageId::new(1))?;
                    for row in rows {
                        self.check_push_move_pop(Some(row.child), node_word, id)?;
                    }
                }
                if R::SEEK_STEPS && total != 0 {
                    let mut result = EngineResult::Accepted;
                    for _ in 0..total {
                        if Self::token_at(self.current()?, self.state.live.cursor)?.get() == extra.get() {
                            break;
                        }
                        result = self.move_rows(Direction::Down, node_word, StepCount::new(1))?;
                    }
                    if result != EngineResult::Accepted {
                        return Ok(EngineResult::Rejected);
                    }
                }
                if !self.switch_node(Switch::Push, node_word, Direction::Down, notify_id)? {
                    return Ok(EngineResult::Rejected);
                }
                if self.move_rows(direction, node_word, count)? != EngineResult::Accepted {
                    return Ok(EngineResult::Rejected);
                }
                self.switched(Switch::Pop, node_word, Direction::Down, notify_id)
            }
        }
    }

    fn switched(&mut self, switch: Switch, node_word: StageNode, direction: Direction, notify_id: StageId) -> Result<EngineResult, NotOwned> {
        Ok(if self.switch_node(switch, node_word, direction, notify_id)? { EngineResult::Accepted } else { EngineResult::Rejected })
    }

    /// Kind 2: `0xd5b350`, `0xbf9a5c` and `0x009575fc` = 0 for ids
    /// `0x1a`..=`0x1d`, otherwise 0 with no call.
    fn close(&mut self, node_word: StageNode, id: StageId) -> Result<EngineResult, NotOwned> {
        let index = Self::stage_index(node_word)?;
        let node = self.current()?;
        if !(0x1a..=0x1d).contains(&id.get()) {
            return Ok(EngineResult::Rejected);
        }
        self.check_cursor()?;
        self.notify(StageKind::Close, node_word, id, node.class);
        self.remember(index, None)?;
        if R::CLOSE_CLEARS_MENU_OPEN {
            self.state.menu_open = MenuOpen::Closed;
        }
        Ok(EngineResult::Accepted)
    }

    /// Kind `0x13`: `0xbfa800(0x13, node, 0x15)` on the open node.
    fn reopen(&mut self, node_word: StageNode) -> Result<EngineResult, NotOwned> {
        let node = self.current()?;
        Self::check_open(node, self.state.live.cursor, FramedTarget::Allowed)?;
        let opened = self.open_rows(OpenMode::Reopen, node_word, Direction::Down)?;
        if opened && !R::REOPEN_SILENT {
            self.notify(StageKind::Open, node_word, StageId::VALUE, node.class);
        }
        Ok(if opened { EngineResult::Accepted } else { EngineResult::Rejected })
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
        self.set_node(target)?;
        self.state.depth = StackDepth::new(1);
        self.state.stack = [CursorFrame::ZERO; STACK_FRAMES];
        self.state.live = CursorFrame::ZERO;
        self.state.total = RowCount::new(self.count(CountMode::All)?);
        let remembered = *self.state.remembered.get(index.index()).ok_or(NotOwned::StageNode)?;
        self.state.live.cursor = RowIndex::new(self.ordinal(remembered, OrdinalMode::Index)?);
        Self::check_open(self.node(target)?, self.state.live.cursor, FramedTarget::Refused)?;
        if self.open_rows(OpenMode::Open(StageKind::Open), node_word, Direction::Down)? {
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

/// The node `View::reload` gives when no node is open, which `frame::fill`
/// does not reach.
static NO_NODE: Node = Node {
    id: NodeId::DETACHED,
    node_type: NodeType::DETACHED,
    class: MenuClass::new(0),
    rows_per_page: RowsPerPage::new(1),
    wrap: Wrap::Wraps,
    availability: Availability::Queried,
    remember: Remember::Off,
    parent: None,
    word10: NodeWord10::new(0),
    rows: None,
};

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
        self.0.and_then(|rows| rows.get(row.get() as usize)).is_some_and(|row| row.child != RowChild::Leaf)
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
        let node = self.tree.node(self.node.parent?)?;
        Some(NodeView { tree: self.tree, node })
    }

    fn values(self) -> RowsView {
        RowsView(self.node.rows)
    }
}

impl<'a> FrameCells for View<'a> {
    type Node = NodeView<'a>;

    fn current(&self) -> Option<NodeView<'a>> {
        let node = self.tree.node(self.state.node?)?;
        Some(NodeView { tree: self.tree, node })
    }

    fn reload(&self) -> NodeView<'a> {
        self.current().unwrap_or(NodeView { tree: self.tree, node: &NO_NODE })
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

    fn page(&self, rows: RowsPerPage, visible: VisibleOrdinal) -> PageNumber {
        PageNumber::new(divide(rows.get().wrapping_add(visible.get()), rows.get()))
    }
}

/// Unsigned division without a library call; a zero divisor gives 0 where
/// stock enters the loader's fatal divide.
fn divide(numerator: u32, divisor: u32) -> u32 {
    if divisor == 0 {
        return 0;
    }
    let mut quotient = 0u32;
    let mut remainder = 0u32;
    for bit in (0..32).rev() {
        remainder = (remainder << 1) | ((numerator >> bit) & 1);
        if remainder >= divisor {
            remainder -= divisor;
            quotient |= 1 << bit;
        }
    }
    quotient
}
