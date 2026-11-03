//! Owned generic value picker for the controller-75 keys of the menu-node-1
//! shooting-menu pickers: Up, Down, Left, Right, Ok, Back and the router tail.
//!
//! A controller-75 cell named by the build-time table runs its owned action
//! while the open menu node is a table node and the live cell word and action
//! target equal the table's; every other cell runs the stock action. An owned
//! action runs the stock generic path through `PickerServices`, or returns
//! `Outcome::Decline` before its first side-effecting service call, and the
//! dispatcher then runs the stock action.

#![cfg_attr(not(test), allow(dead_code))]

/// Menu node record address, `*0x009571d0` while a menu is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NodeAddress(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PropertyId(pub u16);

#[derive(Clone, Copy, Debug)]
pub struct PickerNode {
    pub node: NodeAddress,
    pub property: PropertyId,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickerAction {
    Up,
    Down,
    Left,
    Right,
    Ok,
    Back,
    RouterTail,
}

#[derive(Clone, Copy, Debug)]
pub struct PickerCell {
    pub controller: u16,
    pub state: u16,
    pub event: u16,
    /// Full stock cell word.
    pub word: u32,
    /// Stock action address.
    pub target: u32,
    pub action: PickerAction,
}

#[derive(Clone, Copy, Debug)]
pub struct PickerTable {
    pub nodes: &'static [PickerNode],
    pub cells: &'static [PickerCell],
}

/// The owned table, generated from owner-supplied image5 bytes and the
/// decoded menu tables by `//replacement/modal_runtime:picker-table`.
#[cfg(any(rb_menu_picker_owned, rb_menu_picker_vectors))]
pub static TABLE: PickerTable = include!(env!("RB_PICKER_TABLE"));

/// Live cell word and action target read before the action.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LiveCell {
    pub word: u32,
    pub target: u32,
}

#[derive(Clone, Copy, Debug)]
pub enum PickerPlan<'a> {
    Owned(&'a PickerCell, &'a PickerNode),
    Stock,
}

const PICKER_CONTROLLER: u16 = 75;

pub fn plan<'a>(
    table: &'a PickerTable,
    controller: u16,
    state: u16,
    event: u16,
    live: LiveCell,
    open_node: NodeAddress,
) -> PickerPlan<'a> {
    if controller != PICKER_CONTROLLER {
        return PickerPlan::Stock;
    }
    let Some(node) = table.nodes.iter().find(|node| node.node == open_node) else {
        return PickerPlan::Stock;
    };
    let Some(cell) =
        table.cells.iter().find(|cell| cell.controller == controller && cell.state == state && cell.event == event)
    else {
        return PickerPlan::Stock;
    };
    if cell.word != live.word || cell.target != live.target {
        return PickerPlan::Stock;
    }
    PickerPlan::Owned(cell, node)
}

/// Menu node id returned by `0x61d154`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NodeId(pub u32);

/// Node class returned by `0x61d1f4`; a picker node's class is its property.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClassId(pub u32);

/// Cursor token returned by `0x61d244`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CursorToken(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StageKind(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StageId(pub u32);

/// r0 of `0xc12aac`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RouterResult(pub u32);

impl RouterResult {
    /// Stock tests `r == -1 || r == 2` after the router.
    const fn accepted(self) -> bool {
        self.0 == u32::MAX || self.0 == 2
    }
}

/// Argument of `0xd38d98`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NotifyCode(pub u32);

/// Word globals the owned actions read or write.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WordCell {
    Mode9544f8,
    Mode950474,
    Mode950478,
    Mode954500,
    NotifyFlag,
}

impl WordCell {
    pub const fn address(self) -> u32 {
        match self {
            Self::Mode9544f8 => 0x0095_44f8,
            Self::Mode950474 => 0x0095_0474,
            Self::Mode950478 => 0x0095_0478,
            Self::Mode954500 => 0x0095_4500,
            Self::NotifyFlag => 0x0095_04a8,
        }
    }
}

/// Stock bindings used by the owned actions. `busy`, `node_id`, `class`,
/// `cursor_token` and `read_word` have no stock side effect.
pub trait PickerServices {
    /// `0x63a0f8(0x53) != 0`.
    fn busy(&mut self) -> bool;
    /// `0x61d154`.
    fn node_id(&mut self) -> NodeId;
    /// `0x61d1f4`.
    fn class(&mut self) -> ClassId;
    /// `0x61d244`.
    fn cursor_token(&mut self) -> CursorToken;
    /// `0xd3d858(kind, id)`.
    fn stage_open(&mut self, kind: StageKind, id: StageId);
    /// `0xc12aac`.
    fn router(&mut self) -> RouterResult;
    /// `0xd38d98(code)`.
    fn notify(&mut self, code: NotifyCode);
    fn read_word(&mut self, cell: WordCell) -> u32;
    fn write_word(&mut self, cell: WordCell, value: u32);
}

/// Stock returns the action's r0 unchanged as the dispatcher result.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ActionResult(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Result(ActionResult),
    Decline,
}

const PICKER_NODE_ID: NodeId = NodeId(1);

const STAGE_UP: (StageKind, StageId) = (StageKind(5), StageId(1));
const STAGE_DOWN: (StageKind, StageId) = (StageKind(4), StageId(1));
const STAGE_SELECT: (StageKind, StageId) = (StageKind(0xf), StageId(0x1d));
const STAGE_BACK: (StageKind, StageId) = (StageKind(0xf), StageId(0x1a));

const NOTIFY_SELECT: NotifyCode = NotifyCode(0x930);
const NOTIFY_BACK: NotifyCode = NotifyCode(0x933);

/// Classes `0xc6f14c` tests before its generic path: 5 (lines 19-45),
/// 0xee (34), 0x184 (40), 0xf5 (50), 0x167 (58), 0x19 (69), 0xed (86),
/// 0xe8 (101).
const LEFT_CLASSES: &[u32] = &[5, 0xee, 0x184, 0xf5, 0x167, 0x19, 0xed, 0xe8];

/// Classes `0xc72e10` tests before its generic path: 5 (22), 0x184 (338),
/// 0x141 (351), 0xf5 (364), 0x167 (380-418), 0x26 (428), 0x19 (440-453),
/// 0xed (467), 0xee (469), 0x47 (508), 0xe8 (535).
const OK_CLASSES: &[u32] = &[5, 0x184, 0x141, 0xf5, 0x167, 0x26, 0x19, 0xed, 0xee, 0x47, 0xe8];

/// Classes `0xc70528` tests before its generic return: 5 (21), 0x184 (23),
/// 0x141 (35), 0x167 (59-105), 0x26 (107), 0x19 (118-145), 0xed (146, 181),
/// 0xee (148), 0x47 (162).
const RIGHT_CLASSES: &[u32] = &[5, 0x184, 0x141, 0x167, 0x26, 0x19, 0xed, 0xee, 0x47];

/// Classes `0xc76650` tests before its generic path: 5 (20), 0x167 (74),
/// 0xf5 (85), 0xed (88, 102), 0xee (95), 0xfe (106).
const BACK_CLASSES: &[u32] = &[5, 0x167, 0xf5, 0xed, 0xee, 0xfe];

pub fn run(action: PickerAction, node: &PickerNode, services: &mut impl PickerServices) -> Outcome {
    match action {
        PickerAction::Up => step(node, STAGE_UP, services),
        PickerAction::Down => step(node, STAGE_DOWN, services),
        PickerAction::Left => select(node, LEFT_CLASSES, &[WordCell::Mode9544f8, WordCell::Mode950474, WordCell::Mode950478], services),
        PickerAction::Ok => select(
            node,
            OK_CLASSES,
            &[WordCell::Mode9544f8, WordCell::Mode950474, WordCell::Mode950478, WordCell::Mode954500],
            services,
        ),
        PickerAction::Right => right(node, services),
        PickerAction::Back => back(node, services),
        PickerAction::RouterTail => router_tail(services),
    }
}

/// `node_id()` is the picker id and `class()` is the node property outside
/// `special`.
fn picker_node(node: &PickerNode, special: &[u32], services: &mut impl PickerServices) -> bool {
    if services.node_id() != PICKER_NODE_ID {
        return false;
    }
    let ClassId(class) = services.class();
    class == u32::from(node.property.0) && !special.contains(&class)
}

fn any_mode_set(modes: &[WordCell], services: &mut impl PickerServices) -> bool {
    modes.iter().any(|cell| services.read_word(*cell) == 1)
}

const fn result(value: u32) -> Outcome {
    Outcome::Result(ActionResult(value))
}

/// UP `0xc6e378` (stage 5,1) and DOWN `0xc6eb44` (stage 4,1). Declined: node
/// id other than the picker id, which covers the root-node branch (0xc6e378
/// lines 14-24, 0xc6eb44 lines 15-29).
fn step(node: &PickerNode, (kind, id): (StageKind, StageId), services: &mut impl PickerServices) -> Outcome {
    if services.busy() {
        return result(0);
    }
    if !picker_node(node, &[], services) {
        return Outcome::Decline;
    }
    services.stage_open(kind, id);
    result(services.router().0)
}

/// LEFT `0xc6f14c` lines 95-106 and OK `0xc72e10` lines 527-540. Declined:
/// node id other than the picker id (0xc6f14c line 14; 0xc72e10 lines 15,
/// 298), class other than the property or in the action's class list, and a
/// mode cell equal to 1 (0xc6f14c lines 20, 46, 51, 54; 0xc72e10 lines 494,
/// 501, 506).
fn select(node: &PickerNode, special: &[u32], modes: &[WordCell], services: &mut impl PickerServices) -> Outcome {
    if services.busy() {
        return result(0);
    }
    if !picker_node(node, special, services) || any_mode_set(modes, services) {
        return Outcome::Decline;
    }
    services.stage_open(STAGE_SELECT.0, STAGE_SELECT.1);
    let routed = services.router();
    if !routed.accepted() {
        return result(routed.0);
    }
    let ClassId(class) = services.class();
    if class == 5 && services.cursor_token() == CursorToken(0x35) {
        return result(routed.0);
    }
    if class == 0xe8 {
        return result(routed.0);
    }
    services.notify(NOTIFY_SELECT);
    services.write_word(WordCell::NotifyFlag, 1);
    result(routed.0)
}

/// RIGHT `0xc70528` line 179 returns 0. Declined: node id other than the
/// picker id (line 14), class other than the property or in `RIGHT_CLASSES`,
/// and `0x954500 == 1` (line 160).
fn right(node: &PickerNode, services: &mut impl PickerServices) -> Outcome {
    if services.busy() {
        return result(0);
    }
    if !picker_node(node, RIGHT_CLASSES, services) || any_mode_set(&[WordCell::Mode954500], services) {
        return Outcome::Decline;
    }
    result(0)
}

/// BACK `0xc76650` lines 110-127. Declined: node id other than the picker id
/// (lines 13, 20), class other than the property or in `BACK_CLASSES`, and a
/// mode cell `0x9544f8`, `0x950474` or `0x950478` equal to 1 (lines 25, 49).
fn back(node: &PickerNode, services: &mut impl PickerServices) -> Outcome {
    if services.busy() {
        return result(0);
    }
    if !picker_node(node, BACK_CLASSES, services)
        || any_mode_set(&[WordCell::Mode9544f8, WordCell::Mode950474, WordCell::Mode950478], services)
    {
        return Outcome::Decline;
    }
    services.stage_open(STAGE_BACK.0, STAGE_BACK.1);
    let routed = services.router();
    if !routed.accepted() {
        return result(routed.0);
    }
    if matches!(services.node_id(), NodeId(0x60 | 0x37 | 0x61)) {
        return result(routed.0);
    }
    services.notify(NOTIFY_BACK);
    services.write_word(WordCell::NotifyFlag, 1);
    result(routed.0)
}

/// Router tail `0xc7a514`: `0xd4b2a4` (an empty function in image5), then
/// `0xc12aac`. No branch.
fn router_tail(services: &mut impl PickerServices) -> Outcome {
    result(services.router().0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROPERTY: u16 = 0xf6;

    static NODES: [PickerNode; 2] = [
        PickerNode { node: NodeAddress(0x0078_2000), property: PropertyId(PROPERTY) },
        PickerNode { node: NodeAddress(0x0078_2100), property: PropertyId(0x1a) },
    ];

    static CELLS: [PickerCell; 2] = [
        PickerCell { controller: 75, state: 0, event: 8, word: 0x0000_0108, target: 0x00c7_2e10, action: PickerAction::Ok },
        PickerCell { controller: 74, state: 0, event: 8, word: 0x0000_0108, target: 0x00c7_2e10, action: PickerAction::Ok },
    ];

    static TABLE: PickerTable = PickerTable { nodes: &NODES, cells: &CELLS };

    static NODE: &PickerNode = &NODES[0];

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Call {
        Stage(u32, u32),
        Router,
        Notify(u32),
        Word(WordCell, u32),
    }

    struct Fake {
        busy: bool,
        node_id: u32,
        node_id_after_router: u32,
        class: u32,
        class_after_router: u32,
        cursor: u32,
        router: u32,
        modes: [u32; 4],
        routed: bool,
        calls: Vec<Call>,
    }

    impl Fake {
        fn picker(node: &PickerNode) -> Self {
            let class = u32::from(node.property.0);
            Self {
                busy: false,
                node_id: 1,
                node_id_after_router: 1,
                class,
                class_after_router: class,
                cursor: 0,
                router: 0,
                modes: [0; 4],
                routed: false,
                calls: Vec::new(),
            }
        }
    }

    impl PickerServices for Fake {
        fn busy(&mut self) -> bool {
            self.busy
        }
        fn node_id(&mut self) -> NodeId {
            NodeId(if self.routed { self.node_id_after_router } else { self.node_id })
        }
        fn class(&mut self) -> ClassId {
            ClassId(if self.routed { self.class_after_router } else { self.class })
        }
        fn cursor_token(&mut self) -> CursorToken {
            CursorToken(self.cursor)
        }
        fn stage_open(&mut self, kind: StageKind, id: StageId) {
            self.calls.push(Call::Stage(kind.0, id.0));
        }
        fn router(&mut self) -> RouterResult {
            self.calls.push(Call::Router);
            self.routed = true;
            RouterResult(self.router)
        }
        fn notify(&mut self, code: NotifyCode) {
            self.calls.push(Call::Notify(code.0));
        }
        fn read_word(&mut self, cell: WordCell) -> u32 {
            match cell {
                WordCell::Mode9544f8 => self.modes[0],
                WordCell::Mode950474 => self.modes[1],
                WordCell::Mode950478 => self.modes[2],
                WordCell::Mode954500 => self.modes[3],
                WordCell::NotifyFlag => 0,
            }
        }
        fn write_word(&mut self, cell: WordCell, value: u32) {
            self.calls.push(Call::Word(cell, value));
        }
    }

    const K_ACTIONS: [PickerAction; 6] =
        [PickerAction::Up, PickerAction::Down, PickerAction::Left, PickerAction::Right, PickerAction::Ok, PickerAction::Back];

    const ACCEPTED: [u32; 2] = [u32::MAX, 2];

    #[test]
    fn plan_owns_a_table_cell_on_a_table_node() {
        let live = LiveCell { word: 0x0000_0108, target: 0x00c7_2e10 };
        let PickerPlan::Owned(cell, node) = plan(&TABLE, 75, 0, 8, live, NodeAddress(0x0078_2100)) else {
            panic!("expected owned");
        };
        assert_eq!(cell.action, PickerAction::Ok);
        assert_eq!(node.property, PropertyId(0x1a));
    }

    #[test]
    fn plan_refuses_unknown_node_cell_word_and_target() {
        let live = LiveCell { word: 0x0000_0108, target: 0x00c7_2e10 };
        let node = NodeAddress(0x0078_2000);
        let refusals = [
            plan(&TABLE, 75, 0, 8, live, NodeAddress(0x0078_2004)),
            plan(&TABLE, 75, 0, 6, live, node),
            plan(&TABLE, 75, 1, 8, live, node),
            plan(&TABLE, 75, 0, 8, LiveCell { word: 0x0000_0109, ..live }, node),
            plan(&TABLE, 75, 0, 8, LiveCell { target: 0x00c6_f14c, ..live }, node),
        ];
        for refused in refusals {
            assert!(matches!(refused, PickerPlan::Stock), "{refused:?}");
        }
    }

    #[test]
    fn plan_refuses_a_table_cell_of_another_controller() {
        let live = LiveCell { word: 0x0000_0108, target: 0x00c7_2e10 };
        let refused = plan(&TABLE, 74, 0, 8, live, NodeAddress(0x0078_2000));
        assert!(matches!(refused, PickerPlan::Stock), "{refused:?}");
    }

    #[test]
    fn busy_returns_zero_without_calls() {
        for action in K_ACTIONS {
            let mut fake = Fake::picker(NODE);
            fake.busy = true;
            assert_eq!(run(action, NODE, &mut fake), result(0), "{action:?}");
            assert!(fake.calls.is_empty(), "{action:?}");
        }
    }

    #[test]
    fn up_and_down_open_their_stage_and_return_the_router_result() {
        for (action, kind) in [(PickerAction::Up, 5), (PickerAction::Down, 4)] {
            for router in [0, 2, u32::MAX, 7] {
                let mut fake = Fake::picker(NODE);
                fake.router = router;
                assert_eq!(run(action, NODE, &mut fake), result(router));
                assert_eq!(fake.calls, [Call::Stage(kind, 1), Call::Router]);
            }
        }
    }

    #[test]
    fn left_and_ok_notify_on_an_accepted_router_result() {
        for action in [PickerAction::Left, PickerAction::Ok] {
            for router in ACCEPTED {
                let mut fake = Fake::picker(NODE);
                fake.router = router;
                assert_eq!(run(action, NODE, &mut fake), result(router));
                assert_eq!(
                    fake.calls,
                    [Call::Stage(0xf, 0x1d), Call::Router, Call::Notify(0x930), Call::Word(WordCell::NotifyFlag, 1)]
                );
            }
            let mut fake = Fake::picker(NODE);
            fake.router = 0;
            assert_eq!(run(action, NODE, &mut fake), result(0));
            assert_eq!(fake.calls, [Call::Stage(0xf, 0x1d), Call::Router]);
        }
    }

    #[test]
    fn left_and_ok_skip_the_notification_for_the_class_after_the_router() {
        for action in [PickerAction::Left, PickerAction::Ok] {
            for (class, cursor, notifies) in [(5, 0x35, false), (0xe8, 0, false), (5, 0x34, true)] {
                let mut fake = Fake::picker(NODE);
                fake.router = 2;
                fake.class_after_router = class;
                fake.cursor = cursor;
                assert_eq!(run(action, NODE, &mut fake), result(2));
                assert_eq!(fake.calls.contains(&Call::Notify(0x930)), notifies, "{action:?} {class:#x} {cursor:#x}");
            }
        }
    }

    #[test]
    fn right_returns_zero_without_calls() {
        let mut fake = Fake::picker(NODE);
        fake.router = 2;
        assert_eq!(run(PickerAction::Right, NODE, &mut fake), result(0));
        assert!(fake.calls.is_empty());
    }

    #[test]
    fn back_notifies_on_an_accepted_router_result() {
        for router in ACCEPTED {
            let mut fake = Fake::picker(NODE);
            fake.router = router;
            assert_eq!(run(PickerAction::Back, NODE, &mut fake), result(router));
            assert_eq!(
                fake.calls,
                [Call::Stage(0xf, 0x1a), Call::Router, Call::Notify(0x933), Call::Word(WordCell::NotifyFlag, 1)]
            );
        }
        let mut fake = Fake::picker(NODE);
        fake.router = 1;
        assert_eq!(run(PickerAction::Back, NODE, &mut fake), result(1));
        assert_eq!(fake.calls, [Call::Stage(0xf, 0x1a), Call::Router]);
    }

    #[test]
    fn back_skips_the_notification_for_the_node_after_the_router() {
        for node_id in [0x60, 0x37, 0x61] {
            let mut fake = Fake::picker(NODE);
            fake.router = u32::MAX;
            fake.node_id_after_router = node_id;
            assert_eq!(run(PickerAction::Back, NODE, &mut fake), result(u32::MAX));
            assert_eq!(fake.calls, [Call::Stage(0xf, 0x1a), Call::Router]);
        }
    }

    #[test]
    fn router_tail_returns_the_router_result() {
        let mut fake = Fake::picker(NODE);
        fake.router = 9;
        assert_eq!(run(PickerAction::RouterTail, NODE, &mut fake), result(9));
        assert_eq!(fake.calls, [Call::Router]);
    }

    #[test]
    fn k_actions_decline_off_the_picker_node() {
        for action in K_ACTIONS {
            for node_id in [0, 2, 0x5b] {
                let mut fake = Fake::picker(NODE);
                fake.node_id = node_id;
                assert_eq!(run(action, NODE, &mut fake), Outcome::Decline, "{action:?} {node_id:#x}");
                assert!(fake.calls.is_empty());
            }
            let mut fake = Fake::picker(NODE);
            fake.class = 0xf7;
            assert_eq!(run(action, NODE, &mut fake), Outcome::Decline, "{action:?}");
            assert!(fake.calls.is_empty());
        }
    }

    fn special_node(class: u32) -> PickerNode {
        PickerNode { property: PropertyId(class as u16), ..*NODE }
    }

    #[test]
    fn k_actions_decline_special_classes() {
        let cases: [(PickerAction, &[u32]); 4] = [
            (PickerAction::Left, &[5, 0xee, 0x184, 0xf5, 0x167, 0x19, 0xed, 0xe8]),
            (PickerAction::Ok, &[5, 0x184, 0x141, 0xf5, 0x167, 0x26, 0x19, 0xed, 0xee, 0x47, 0xe8]),
            (PickerAction::Right, &[5, 0x184, 0x141, 0x167, 0x26, 0x19, 0xed, 0xee, 0x47]),
            (PickerAction::Back, &[5, 0x167, 0xf5, 0xed, 0xee, 0xfe]),
        ];
        for (action, classes) in cases {
            for class in classes {
                let node = special_node(*class);
                let mut fake = Fake::picker(&node);
                assert_eq!(run(action, &node, &mut fake), Outcome::Decline, "{action:?} {class:#x}");
                assert!(fake.calls.is_empty());
            }
        }
    }

    #[test]
    fn k_actions_decline_set_mode_cells() {
        let cases: [(PickerAction, &[usize]); 4] = [
            (PickerAction::Left, &[0, 1, 2]),
            (PickerAction::Ok, &[0, 1, 2, 3]),
            (PickerAction::Right, &[3]),
            (PickerAction::Back, &[0, 1, 2]),
        ];
        for (action, modes) in cases {
            for mode in 0..4 {
                let mut fake = Fake::picker(NODE);
                fake.modes[mode] = 1;
                let declined = run(action, NODE, &mut fake) == Outcome::Decline;
                assert_eq!(declined, modes.contains(&mode), "{action:?} mode {mode}");
                if declined {
                    assert!(fake.calls.is_empty());
                }
            }
        }
    }
}
