//! Owner oracle: build with `--cfg rb_menu_engine_cases` and
//! `RB_MENU_ENGINE_{TREE,RECORDS,CASES}` naming the outputs of
//! `//replacement/modal_runtime:menu-engine-tree` and `:menu-engine-cases`.

use std::panic::{catch_unwind, AssertUnwindSafe};

use super::block::{decode, mirror, snapshot_address, RecordTable, BLOCK_LEN, LAST_MOVE_LEN};
use super::engine::{
    apply_with, Effect, EngineResult, EngineServices, KindSwitchCall, MenuEvent, PropertyId, Rules, SeekCall, StageEvent,
    StageKind, StageNode, Stock,
};
use super::frame::{MenuClass, MenuFrameSnapshot, RowIndex, RowToken};
use super::state::{EngineState, LastMove, RowState};
use super::tree::{Node, Tree};

/// Names the generated tree and record table use.
#[allow(unused_imports)]
use super::{
    block::RecordAddress,
    frame::{NodeId, NodeType, RowsPerPage},
    tree::{Availability, Label, NodeRef, NodeWord10, Remember, Row, RowChild, StringId, TreeIdentity, Wrap},
};

include!(env!("RB_MENU_ENGINE_TREE"));
include!(env!("RB_MENU_ENGINE_RECORDS"));
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
    PropertyWrite { property: PropertyId, value: RowIndex, flags: u32 },
}

enum LastMoveWrite {
    Class(MenuClass),
    Token(RowToken),
}

#[allow(dead_code)]
struct Case<'a> {
    name: &'static str,
    receipt_sha256: &'static str,
    config_sha256: &'static str,
    event: StageEvent,
    before: [u8; BLOCK_LEN],
    last_move_before: LastMoveBefore,
    /// Each call with the block as it stood at the call.
    calls: &'a [(Expected, [u8; BLOCK_LEN])],
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
    case: &'c Case<'c>,
    next: usize,
    snapshot_writes: Vec<(u32, u32)>,
    mismatch: Option<String>,
    page_entries: usize,
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
        // No receipt traces rb_bank_page_entry.
        if effect == Effect::PageEntry {
            self.page_entries += 1;
            return;
        }
        let index = self.next;
        let asked = format!("{effect:x?}");
        let Some(expected) = self.take(state, &asked) else { return };
        match (effect, expected) {
            (Effect::Notify(notify), Expected::Notify { kind, node, id, class })
                if (notify.kind.raw(), notify.node.get(), notify.id.get(), notify.class.get()) == (*kind, *node, *id, *class) => {}
            (Effect::StoreSnapshot { depth, slot, record }, Expected::Store { slot: stock }) if slot.get() == *stock => {
                self.snapshot_writes.extend(record_writes(snapshot_address(depth, slot), &record));
            }
            (Effect::PropertyWrite { property, value }, Expected::PropertyWrite { property: p, value: v, flags: 0 })
                if (property, value) == (*p, *v) => {}
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

    fn kind_switch(&mut self, _: &mut EngineState, call: KindSwitchCall) -> EngineResult {
        self.mismatch.get_or_insert(format!("call {} {call:x?}: no receipt shows a nested kind switch", self.next));
        EngineResult::Rejected
    }

    fn seek(&mut self, _: &mut EngineState, call: SeekCall) -> EngineResult {
        self.mismatch.get_or_insert(format!("call {} {call:x?}: no receipt shows a seek", self.next));
        EngineResult::Rejected
    }

    fn token_past_rows(&mut self, _: &EngineState, row: RowIndex) -> RowToken {
        self.mismatch.get_or_insert(format!("call {} values[{row:?}]: no receipt shows a read past the rows", self.next));
        RowToken::TERMINATOR
    }

    fn invariant_failure(&mut self) -> ! {
        panic!("invariant failure after call {}", self.next)
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
    let mut state = decode(&case.before, &last_move_bytes(before_pair), &TREE, &RECORDS).map_err(|error| format!("decode {error:?}"))?;
    let event = MenuEvent::decode(case.event, &TREE, &state);
    let mut answers = Answers { case, next: 0, snapshot_writes: Vec::new(), mismatch: None, page_entries: 0 };
    let applied = catch_unwind(AssertUnwindSafe(|| apply_with::<R, _, _>(&TREE, &mut state, event, &mut answers)))
        .map_err(|_| "engine panicked".to_string())?;
    let result = applied.map_err(|reason| format!("not owned: {reason:?}"))?;
    if let Some(mismatch) = answers.mismatch {
        return Err(mismatch);
    }
    if answers.next != case.calls.len() {
        return Err(format!("engine made {} calls; the receipt has {}", answers.next, case.calls.len()));
    }
    let opens = case.calls.iter().filter(|(expected, _)| matches!(expected, Expected::InitialRow { .. })).count();
    if answers.page_entries != opens {
        return Err(format!("{} page entries for {opens} initial-row calls", answers.page_entries));
    }
    let (block, _) = mirror(&state, &RECORDS);
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
    if state.last_move != expected_pair {
        return Err(format!("last move {:?}, expected {:?}", state.last_move, expected_pair));
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
    if result != case.result {
        return Err(format!("result {:?}, expected {:?}", result, case.result));
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
    assert!(!CASES.is_empty());
    let results = outcomes::<Stock>();
    for (name, outcome) in &results {
        println!("case {name}: {}", outcome.as_ref().map_or_else(|reason| format!("FAIL {reason}"), |_| "pass".into()));
    }
    assert!(results.iter().all(|(_, outcome)| outcome.is_ok()));
}

struct RefusalAccepted;
impl Rules for RefusalAccepted {
    const REFUSAL_REJECTS: bool = false;
}

const fn refusal_snapshot_writes(cursor: u32) -> [(u32, u32); 22] {
    let defaults = [1, 0x86, 0xc, 0, 0, 0, 0, 0, 0, 0, 0];
    let token = if cursor == 4 { 0x7fff_fffe } else { 8 };
    let words = [2, 0x10, 4, 0x1d8, token, 8, cursor, 0, 0, 1, 0];
    let mut writes = [(0, 0); 22];
    let mut index = 0;
    while index < 11 {
        let address = 0x941594 + index as u32 * 4;
        writes[index] = (address, defaults[index]);
        writes[index + 11] = (address, words[index]);
        index += 1;
    }
    writes
}

const REFUSAL_SNAPSHOT: [(u32, u32); 22] = refusal_snapshot_writes(0);
const REFUSAL_TERMINATOR_SNAPSHOT: [(u32, u32); 22] = refusal_snapshot_writes(4);

#[test]
fn owner_injected_refusals_match_stock() {
    for (name, kind, cursor) in [("no-child", 0xe, 0), ("no-child-terminator", 0xe, 4), ("no-parent", 0xf, 0)] {
        let mut before = [0; BLOCK_LEN];
        for (offset, value) in [(0, 0x77c0f0u32), (4, 0x15b), (8, cursor), (0x18, 4), (0x424, 1), (0x42c, 1)] {
            before[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        let case = Case {
            name,
            receipt_sha256: "",
            config_sha256: "",
            event: StageEvent { kind, node: 0x10, id: 0x1d, extra: 0 },
            before,
            last_move_before: LastMoveBefore { class: None, token: None },
            calls: &[(Expected::Store { slot: 0x10 }, before)],
            after: before,
            last_move_writes: &[],
            snapshot_writes: if cursor == 4 { &REFUSAL_TERMINATOR_SNAPSHOT } else { &REFUSAL_SNAPSHOT },
            result: EngineResult::Rejected,
        };
        assert_eq!(run_case::<Stock>(&case), Ok(()), "{name}");
        assert!(run_case::<RefusalAccepted>(&case).is_err(), "{name}: refusal-accepted control passed");
        println!("injected case {name}: pass; refusal-accepted control fails");
    }
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

/// The cases `R` fails.
fn control<R: Rules>(label: &str) -> Vec<&'static str> {
    let results = outcomes::<R>();
    for (name, outcome) in &results {
        println!("control {label} case {name}: {}", outcome.as_ref().map_or_else(|reason| format!("fails ({reason})"), |_| "passes".into()));
    }
    results.into_iter().filter(|(_, outcome)| outcome.is_err()).map(|(name, _)| name).collect()
}

struct PopNoRemember;
impl Rules for PopNoRemember {
    const POP_REMEMBERS: bool = false;
}

struct RememberIgnoresFlag;
impl Rules for RememberIgnoresFlag {
    const REMEMBER_FLAG: bool = false;
}

struct RememberIgnoresParentId;
impl Rules for RememberIgnoresParentId {
    const REMEMBER_PARENT_ID: bool = false;
}

struct CloseKeepsMenuOpen;
impl Rules for CloseKeepsMenuOpen {
    const CLOSE_CLEARS_MENU_OPEN: bool = false;
}

/// The inner steps of kinds 6..`0xb` notify with the event's id.
struct FrameNotifyId;
impl Rules for FrameNotifyId {
    const FRAME_NOTIFY_ID_ZERO: bool = false;
}

struct NoSeekStep;
impl Rules for NoSeekStep {
    const SEEK_STEPS: bool = false;
}

struct QueryFixed;
impl Rules for QueryFixed {
    const FIXED_UNQUERIED: bool = false;
}

struct NoPropertyWrite;
impl Rules for NoPropertyWrite {
    const PROPERTY_WRITE: bool = false;
}

struct GroupAsRowStep;
impl Rules for GroupAsRowStep {
    const GROUP_ARMS: bool = false;
}

struct AlwaysWrap;
impl Rules for AlwaysWrap {
    fn wraps(_: &Node) -> bool {
        true
    }
}

struct ReopenNotifies;
impl Rules for ReopenNotifies {
    const REOPEN_SILENT: bool = false;
}

struct SingleStep;
impl Rules for SingleStep {
    const STEP_COUNT: bool = false;
}

struct NoPassStop;
impl Rules for NoPassStop {
    const PASS_STOP: bool = false;
}

struct NoFramedFollow;
impl Rules for NoFramedFollow {
    const FRAMED_FOLLOW: bool = false;
}

struct FramedPopAsPush;
impl Rules for FramedPopAsPush {
    const FRAMED_POP: bool = false;
}

/// Kind 1's framed push notifies with the event's id, the framed step of
/// kinds `0xe` and `0xf` with `0x1d`.
struct FramedNotifyIdSwap;
impl Rules for FramedNotifyIdSwap {
    const FRAMED_NOTIFY_ID: bool = false;
}

struct PushMoveUpAsDown;
impl Rules for PushMoveUpAsDown {
    const PUSH_MOVE_UP: bool = false;
}

/// Each control and the cases it must fail; an empty list asks for any case.
#[test]
fn owner_controls_fail() {
    let controls: [(&str, Vec<&str>, &[&str]); 25] = [
        ("no-wrap", control::<NoWrap>("no-wrap"), &["fixed-wrap"]),
        ("inverted-skip", control::<InvertedSkip>("inverted-skip"), &[]),
        ("no-initial-row", control::<NoInitialRow>("no-initial-row"), &["fn"]),
        ("no-last-move", control::<NoLastMove>("no-last-move"), &[]),
        ("zero-scroll", control::<ZeroScroll>("zero-scroll"), &[]),
        ("no-push-classifier", control::<NoPushClassifier>("no-push-classifier"), &[]),
        ("down-scroll-keeps-top", control::<DownScrollKeepsTop>("down-scroll-keeps-top"), &[]),
        ("open-scroll-at-zero", control::<OpenScrollAtZero>("open-scroll-at-zero"), &[]),
        ("pop-no-remember", control::<PopNoRemember>("pop-no-remember"), &["pop-remember"]),
        ("remember-ignores-flag", control::<RememberIgnoresFlag>("remember-ignores-flag"), &["pop-remember-off", "push-remember-off"]),
        ("remember-ignores-parent-id", control::<RememberIgnoresParentId>("remember-ignores-parent-id"), &["push-same-parent"]),
        ("close-keeps-menu-open", control::<CloseKeepsMenuOpen>("close-keeps-menu-open"), &["close", "open-seek-1"]),
        (
            "frame-notify-id",
            control::<FrameNotifyId>("frame-notify-id"),
            &["frame-seek-down", "frame-seek-up", "frame-pop-down", "frame-pop-up", "frame-push-probe", "frame-push-up-probe"],
        ),
        ("no-seek-step", control::<NoSeekStep>("no-seek-step"), &["frame-seek-step"]),
        ("query-fixed", control::<QueryFixed>("query-fixed"), &["frame-fixed-open"]),
        ("no-property-write", control::<NoPropertyWrite>("no-property-write"), &["close-property"]),
        (
            "group-as-row-step",
            control::<GroupAsRowStep>("group-as-row-step"),
            &["group-up", "group-down", "group-right-wrap", "group-left-wrap", "group-down-wrap", "group-up-wrap"],
        ),
        ("always-wrap", control::<AlwaysWrap>("always-wrap"), &["nowrap-edge"]),
        ("reopen-notifies", control::<ReopenNotifies>("reopen-notifies"), &["reopen"]),
        ("single-step", control::<SingleStep>("single-step"), &["step-rows"]),
        ("no-pass-stop", control::<NoPassStop>("no-pass-stop"), &["step-rows-wrap"]),
        (
            "no-framed-follow",
            control::<NoFramedFollow>("no-framed-follow"),
            &["framed-open", "framed-open-restored", "framed-pop", "framed-pop-back", "framed-pop-group41", "framed-push"],
        ),
        ("framed-pop-as-push", control::<FramedPopAsPush>("framed-pop-as-push"), &["framed-pop", "framed-pop-back", "framed-pop-group41"]),
        // `framed-pop` and `framed-push` carry the event id 0x1d.
        (
            "framed-notify-id-swap",
            control::<FramedNotifyIdSwap>("framed-notify-id-swap"),
            &["framed-open", "framed-open-restored", "framed-pop-back", "framed-pop-group41"],
        ),
        ("push-move-up-as-down", control::<PushMoveUpAsDown>("push-move-up-as-down"), &["frame-push-up-probe"]),
    ];
    let mut missed = Vec::new();
    for (label, failed, required) in &controls {
        if failed.is_empty() {
            missed.push(format!("{label} matches every case"));
        }
        for case in required.iter().filter(|case| !failed.contains(case)) {
            missed.push(format!("{label} matches {case}"));
        }
    }
    assert!(missed.is_empty(), "{missed:?}");
}
