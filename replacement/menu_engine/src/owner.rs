//! Owner oracle: build with `--cfg rb_menu_engine_cases` and
//! `RB_MENU_ENGINE_{TREE,RECORDS,CASES}` naming the outputs of
//! `//replacement/modal_runtime:menu-engine-tree` and `:menu-engine-cases`.

use std::panic::{catch_unwind, AssertUnwindSafe};

use super::block::{decode, mirror, snapshot_address, RecordTable, BLOCK_LEN, LAST_MOVE_LEN};
use super::engine::{
    apply_with, Effect, EngineResult, EngineServices, KindSwitchCall, MenuEvent, Rules, SeekCall, StageEvent, StageKind,
    StageNode, Stock,
};
use super::frame::{MenuClass, MenuFrameSnapshot, RowIndex, RowToken};
use super::state::{EngineState, LastMove, RowState};
use super::tree::{Node, Tree};

/// Names the generated tree and record table use.
#[allow(unused_imports)]
use super::{
    block::RecordAddress,
    frame::{NodeId, NodeType, RowsPerPage},
    tree::{Availability, Label, NodeRef, NodeWord10, Remember, Row, StringId, TreeIdentity, Wrap},
};

static TREE: Tree = include!(env!("RB_MENU_ENGINE_TREE"));
static RECORDS: RecordTable = include!(env!("RB_MENU_ENGINE_RECORDS"));
static CASES: &[Case] = include!(env!("RB_MENU_ENGINE_CASES"));

struct LastMoveBefore {
    class: Option<MenuClass>,
    token: Option<RowToken>,
}

/// A stock call the receipt shows, in instruction order.
#[derive(Debug)]
enum Expected {
    Notify { kind: u32, node: u32, id: u32, class: u32 },
    Availability { class: MenuClass, token: RowToken, state: RowState },
    InitialRow { kind: u32, node: u32, class: MenuClass, answer: Option<RowToken> },
    Store { slot: u32 },
}

enum LastMoveWrite {
    Class(MenuClass),
    Token(RowToken),
}

#[allow(dead_code)]
struct Case {
    name: &'static str,
    receipt_sha256: &'static str,
    config_sha256: &'static str,
    event: StageEvent,
    before: [u8; BLOCK_LEN],
    last_move_before: LastMoveBefore,
    /// Each call with the block as it stood at the call.
    calls: &'static [(Expected, [u8; BLOCK_LEN])],
    after: [u8; BLOCK_LEN],
    last_move_writes: &'static [LastMoveWrite],
    snapshot_writes: &'static [(u32, u32)],
    result: EngineResult,
}

/// Words a receipt leaves unobserved; the engine must not change them.
const UNOBSERVED_CLASS: MenuClass = MenuClass::new(0xdead_0001);
const UNOBSERVED_TOKEN: RowToken = RowToken::new(0xdead_0002);

/// Answers from the receipt's calls in order and records the first call that
/// differs from the receipt's in kind, arguments or block.
struct Answers<'c> {
    case: &'c Case,
    next: usize,
    snapshot_writes: Vec<(u32, u32)>,
    mismatch: Option<String>,
}

impl<'c> Answers<'c> {
    fn take(&mut self, state: &EngineState, asked: &str) -> Option<&'c Expected> {
        let index = self.next;
        self.next += 1;
        let Some((expected, block)) = self.case.calls.get(index) else {
            self.mismatch.get_or_insert(format!("call {index} {asked}: the receipt has {} calls", self.case.calls.len()));
            return None;
        };
        let (owned, _) = mirror(state, &RECORDS);
        if let Some(offset) = (0..BLOCK_LEN).find(|&offset| owned[offset] != block[offset]) {
            self.mismatch.get_or_insert(format!(
                "call {index} {asked}: block differs at +{offset:#x} ({:#x}, stock {:#x})",
                owned[offset], block[offset]
            ));
        }
        Some(expected)
    }

    fn differs(&mut self, index: usize, asked: String, expected: &Expected) {
        self.mismatch.get_or_insert(format!("call {index} {asked}, stock {expected:x?}"));
    }
}

impl EngineServices for Answers<'_> {
    fn perform(&mut self, state: &EngineState, effect: Effect) {
        let index = self.next;
        let asked = format!("{effect:x?}");
        let Some(expected) = self.take(state, &asked) else { return };
        match (effect, expected) {
            (Effect::Notify(notify), Expected::Notify { kind, node, id, class })
                if (notify.kind.raw(), notify.node.get(), notify.id.get(), notify.class.get()) == (*kind, *node, *id, *class) => {}
            (Effect::StoreSnapshot { depth, slot, record }, Expected::Store { slot: stock }) if slot.get() == *stock => {
                self.snapshot_writes.extend(record_writes(snapshot_address(depth, slot), &record));
            }
            _ => self.differs(index, asked, expected),
        }
    }

    fn availability(&mut self, state: &EngineState, class: MenuClass, token: RowToken) -> RowState {
        let index = self.next;
        let asked = format!("availability({class:?}, {token:?})");
        match self.take(state, &asked) {
            Some(Expected::Availability { class: c, token: t, state: answer }) if *c == class && *t == token => *answer,
            Some(expected) => {
                self.differs(index, asked, expected);
                RowState::Offered
            }
            None => RowState::Offered,
        }
    }

    fn initial_row(&mut self, state: &EngineState, kind: StageKind, node: StageNode, class: MenuClass) -> Option<RowToken> {
        let index = self.next;
        let asked = format!("initial_row({kind:?}, {node:?}, {class:?})");
        match self.take(state, &asked) {
            Some(Expected::InitialRow { kind: k, node: n, class: c, answer })
                if *k == kind.raw() && *n == node.get() && *c == class =>
            {
                *answer
            }
            Some(expected) => {
                self.differs(index, asked, expected);
                None
            }
            None => None,
        }
    }

    fn kind_switch(&mut self, state: &EngineState, call: KindSwitchCall) -> (EngineState, EngineResult) {
        self.mismatch.get_or_insert(format!("call {} {call:x?}: no receipt shows a nested kind switch", self.next));
        (*state, EngineResult::Rejected)
    }

    fn seek(&mut self, state: &EngineState, call: SeekCall) -> (EngineState, EngineResult) {
        self.mismatch.get_or_insert(format!("call {} {call:x?}: no receipt shows a seek", self.next));
        (*state, EngineResult::Rejected)
    }

    fn token_past_rows(&mut self, _: &EngineState, row: RowIndex) -> RowToken {
        self.mismatch.get_or_insert(format!("call {} values[{row:?}]: no receipt shows a read past the rows", self.next));
        RowToken::TERMINATOR
    }
}

fn last_move_before(case: &Case) -> LastMove {
    LastMove {
        class: case.last_move_before.class.unwrap_or(UNOBSERVED_CLASS),
        token: case.last_move_before.token.unwrap_or(UNOBSERVED_TOKEN),
    }
}

fn last_move_bytes(pair: LastMove) -> [u8; LAST_MOVE_LEN] {
    let mut bytes = [0u8; LAST_MOVE_LEN];
    bytes[..4].copy_from_slice(&pair.class.get().to_le_bytes());
    bytes[4..].copy_from_slice(&pair.token.get().to_le_bytes());
    bytes
}

/// The stores the owned frame writer makes for one record: the detached
/// defaults, then the eleven words.
fn record_writes(address: u32, record: &MenuFrameSnapshot) -> impl Iterator<Item = (u32, u32)> {
    let words = MenuFrameSnapshot::DETACHED.words().into_iter().chain(record.words());
    words.enumerate().map(move |(index, word)| (address.wrapping_add(4 * (index as u32 % 11)), word))
}

fn run_case<R: Rules>(case: &Case) -> Result<(), String> {
    let before_pair = last_move_before(case);
    let state = decode(&case.before, &last_move_bytes(before_pair), &TREE, &RECORDS).map_err(|error| format!("decode {error:?}"))?;
    let event = MenuEvent::decode(case.event, &TREE, &state);
    let mut answers = Answers { case, next: 0, snapshot_writes: Vec::new(), mismatch: None };
    let applied = catch_unwind(AssertUnwindSafe(|| apply_with::<R, _>(&TREE, &state, event, &mut answers)))
        .map_err(|_| "engine panicked".to_string())?;
    let transition = applied.map_err(|reason| format!("not owned: {reason:?}"))?;
    if let Some(mismatch) = answers.mismatch {
        return Err(mismatch);
    }
    if answers.next != case.calls.len() {
        return Err(format!("engine made {} calls; the receipt has {}", answers.next, case.calls.len()));
    }
    let (block, _) = mirror(&transition.state, &RECORDS);
    let differing: Vec<usize> = (0..BLOCK_LEN).filter(|&offset| block[offset] != case.after[offset]).collect();
    if !differing.is_empty() {
        let words: Vec<String> = differing
            .iter()
            .map(|offset| offset & !3)
            .fold(Vec::new(), |mut words, offset| {
                if words.last() != Some(&offset) {
                    words.push(offset);
                }
                words
            })
            .into_iter()
            .take(8)
            .map(|offset| {
                let word = |bytes: &[u8; BLOCK_LEN]| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
                format!("+{offset:#x}: {:#x} expected {:#x}", word(&block), word(&case.after))
            })
            .collect();
        return Err(format!("after block differs at {} bytes: {}", differing.len(), words.join(", ")));
    }
    let mut expected_pair = before_pair;
    for write in case.last_move_writes {
        match *write {
            LastMoveWrite::Class(class) => expected_pair.class = class,
            LastMoveWrite::Token(token) => expected_pair.token = token,
        }
    }
    if transition.state.last_move != expected_pair {
        return Err(format!("last move {:?}, expected {:?}", transition.state.last_move, expected_pair));
    }
    if answers.snapshot_writes != case.snapshot_writes {
        let owned = &answers.snapshot_writes;
        let first = owned.iter().zip(case.snapshot_writes).position(|(owned, stock)| owned != stock);
        return Err(format!(
            "snapshot writes differ: {} owned, {} stock, first difference at {first:?}: {:x?} vs {:x?}",
            owned.len(),
            case.snapshot_writes.len(),
            first.map(|index| owned[index]),
            first.map(|index| case.snapshot_writes[index]),
        ));
    }
    if transition.result != case.result {
        return Err(format!("result {:?}, expected {:?}", transition.result, case.result));
    }
    Ok(())
}

fn outcomes<R: Rules>() -> Vec<(&'static str, Result<(), String>)> {
    CASES.iter().map(|case| (case.name, run_case::<R>(case))).collect()
}

#[test]
fn owner_receipt_blocks_round_trip() {
    for case in CASES {
        let pair = last_move_bytes(last_move_before(case));
        let blocks = [("before", &case.before), ("after", &case.after)].into_iter();
        for (label, block) in blocks.chain(case.calls.iter().map(|(_, block)| ("call", block))) {
            let state = decode(block, &pair, &TREE, &RECORDS).unwrap_or_else(|error| panic!("{} {label}: {error:?}", case.name));
            assert_eq!(mirror(&state, &RECORDS), (*block, pair), "{} {label}", case.name);
        }
    }
}

#[test]
fn owner_cases_match_stock() {
    assert_eq!(CASES.len(), 10);
    let results = outcomes::<Stock>();
    for (name, outcome) in &results {
        println!("case {name}: {}", outcome.as_ref().map_or_else(|reason| format!("FAIL {reason}"), |_| "pass".into()));
    }
    assert!(results.iter().all(|(_, outcome)| outcome.is_ok()));
}

struct NoWrap;
impl Rules for NoWrap {
    fn wraps(_: &Node) -> bool {
        false
    }
}

struct InvertedSkip;
impl Rules for InvertedSkip {
    fn selectable(state: RowState) -> bool {
        state.raw() >= RowState::Disabled.raw()
    }
}

/// Keeps the cursor restored from the stack.
struct NoInitialRow;
impl Rules for NoInitialRow {
    const INITIAL_ROW: bool = false;
}

struct NoLastMove;
impl Rules for NoLastMove {
    const LAST_MOVE: bool = false;
}

struct ZeroScroll;
impl Rules for ZeroScroll {
    const SCROLL: bool = false;
}

struct NoPushClassifier;
impl Rules for NoPushClassifier {
    const PUSH_CLASSIFIER: bool = false;
}

/// Kind 4's scroll keeps the top when the row leaves the page.
struct DownScrollKeepsTop;
impl Rules for DownScrollKeepsTop {
    const DOWN_BOTTOM_ARM: bool = false;
}

/// The open rule's scroll stays 0 when the row is below the first page.
struct OpenScrollAtZero;
impl Rules for OpenScrollAtZero {
    const OPEN_BOTTOM_ARM: bool = false;
}

fn control<R: Rules>(label: &str) -> usize {
    let results = outcomes::<R>();
    let failed: Vec<_> = results.iter().filter(|(_, outcome)| outcome.is_err()).collect();
    for (name, outcome) in &results {
        println!("control {label} case {name}: {}", outcome.as_ref().map_or_else(|reason| format!("fails ({reason})"), |_| "passes".into()));
    }
    failed.len()
}

#[test]
fn owner_controls_fail() {
    let counts = [
        ("no-wrap", control::<NoWrap>("no-wrap")),
        ("inverted-skip", control::<InvertedSkip>("inverted-skip")),
        ("no-initial-row", control::<NoInitialRow>("no-initial-row")),
        ("no-last-move", control::<NoLastMove>("no-last-move")),
        ("zero-scroll", control::<ZeroScroll>("zero-scroll")),
        ("no-push-classifier", control::<NoPushClassifier>("no-push-classifier")),
        ("down-scroll-keeps-top", control::<DownScrollKeepsTop>("down-scroll-keeps-top")),
        ("open-scroll-at-zero", control::<OpenScrollAtZero>("open-scroll-at-zero")),
    ];
    for (label, failed) in counts {
        assert!(failed > 0, "control {label} matches every case");
    }
}
