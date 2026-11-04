//! Owner oracle: build with `--cfg rb_menu_engine_cases` and
//! `RB_MENU_ENGINE_{TREE,RECORDS,CASES}` naming the outputs of
//! `//replacement/modal_runtime:menu-engine-tree` and `:menu-engine-cases`.

use super::block::{decode, mirror, snapshot_address, RecordTable, BLOCK_LEN, LAST_MOVE_LEN};
use super::engine::{apply_with, Effect, EngineResult, EngineServices, MenuEvent, Rules, StageEvent, StageKind, StageNode, Stock};
use super::frame::{MenuClass, MenuFrameSnapshot, RowToken};
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

struct AvailabilityAnswer {
    class: MenuClass,
    token: RowToken,
    state: RowState,
}

struct InitialRow {
    kind: u32,
    node: u32,
    class: MenuClass,
    answer: Option<RowToken>,
}

enum LastMoveWrite {
    Class(MenuClass),
    Token(RowToken),
}

struct NotifyCall {
    kind: u32,
    node: u32,
    id: u32,
    class: u32,
}

#[allow(dead_code)]
struct Case {
    name: &'static str,
    receipt_sha256: &'static str,
    config_sha256: &'static str,
    event: StageEvent,
    before: [u8; BLOCK_LEN],
    last_move_before: LastMoveBefore,
    availability: &'static [AvailabilityAnswer],
    initial_rows: &'static [InitialRow],
    after: [u8; BLOCK_LEN],
    last_move_writes: &'static [LastMoveWrite],
    snapshot_writes: &'static [(u32, u32)],
    notifies: &'static [NotifyCall],
    result: EngineResult,
}

/// Words a receipt leaves unobserved; the engine must not change them.
const UNOBSERVED_CLASS: MenuClass = MenuClass::new(0xdead_0001);
const UNOBSERVED_TOKEN: RowToken = RowToken::new(0xdead_0002);

/// Answers in the receipt's order and records the first question that
/// differs from the receipt's.
struct Answers<'c> {
    case: &'c Case,
    availability: usize,
    initial_rows: usize,
    mismatch: Option<String>,
}

impl EngineServices for Answers<'_> {
    fn availability(&mut self, _: &EngineState, class: MenuClass, token: RowToken) -> RowState {
        let answer = self.case.availability.get(self.availability);
        self.availability += 1;
        match answer {
            Some(answer) if answer.class == class && answer.token == token => answer.state,
            _ => {
                self.mismatch.get_or_insert(format!("availability {} asked ({class:?}, {token:?})", self.availability - 1));
                RowState::Offered
            }
        }
    }

    fn initial_row(&mut self, _: &EngineState, kind: StageKind, node: StageNode, class: MenuClass) -> Option<RowToken> {
        let answer = self.case.initial_rows.get(self.initial_rows);
        self.initial_rows += 1;
        match answer {
            Some(answer) if answer.kind == kind.raw() && answer.node == node.get() && answer.class == class => answer.answer,
            _ => {
                self.mismatch.get_or_insert(format!("initial row asked ({kind:?}, {node:?}, {class:?})"));
                None
            }
        }
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
    let mut answers = Answers { case, availability: 0, initial_rows: 0, mismatch: None };
    let transition = apply_with::<R, _>(&TREE, &state, event, &mut answers).map_err(|reason| format!("not owned: {reason:?}"))?;
    if let Some(mismatch) = answers.mismatch {
        return Err(mismatch);
    }
    if answers.availability != case.availability.len() || answers.initial_rows != case.initial_rows.len() {
        return Err(format!(
            "services asked {} availability and {} initial rows; receipt has {} and {}",
            answers.availability,
            answers.initial_rows,
            case.availability.len(),
            case.initial_rows.len()
        ));
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
    let snapshot_writes: Vec<(u32, u32)> = transition
        .effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::StoreSnapshot { depth, slot, record } => Some(record_writes(snapshot_address(*depth, *slot), record)),
            Effect::Notify(_) => None,
        })
        .flatten()
        .collect();
    if snapshot_writes != case.snapshot_writes {
        let first = snapshot_writes.iter().zip(case.snapshot_writes).position(|(owned, stock)| owned != stock);
        return Err(format!(
            "snapshot writes differ: {} owned, {} stock, first difference at {first:?}: {:x?} vs {:x?}",
            snapshot_writes.len(),
            case.snapshot_writes.len(),
            first.map(|index| snapshot_writes[index]),
            first.map(|index| case.snapshot_writes[index]),
        ));
    }
    let notifies: Vec<(u32, u32, u32, u32)> = transition
        .effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Notify(notify) => Some((notify.kind.raw(), notify.node.get(), notify.id.get(), notify.class.get())),
            Effect::StoreSnapshot { .. } => None,
        })
        .collect();
    let expected_notifies: Vec<(u32, u32, u32, u32)> =
        case.notifies.iter().map(|call| (call.kind, call.node, call.id, call.class)).collect();
    if notifies != expected_notifies {
        return Err(format!("notifies {notifies:x?}, expected {expected_notifies:x?}"));
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
        for (label, block) in [("before", &case.before), ("after", &case.after)] {
            let pair = last_move_bytes(last_move_before(case));
            let state = decode(block, &pair, &TREE, &RECORDS).unwrap_or_else(|error| panic!("{} {label}: {error:?}", case.name));
            assert_eq!(mirror(&state, &RECORDS), (*block, pair), "{} {label}", case.name);
        }
    }
}

#[test]
fn owner_cases_match_stock() {
    assert_eq!(CASES.len(), 5);
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
    ];
    for (label, failed) in counts {
        assert!(failed > 0, "control {label} matches every case");
    }
}
