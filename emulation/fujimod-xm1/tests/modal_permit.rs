//! Synthetic register snapshots exercise the pure preexecution contract.

use fujimod_xm1::{modal_permit::*, modal_task::{self, *}, provenance::Guest};
use std::{collections::BTreeMap, num::NonZeroU64};

const SEAM: Guest = Guest(0x0100_0000);
const OWNED: Guest = Guest(0x0100_1000);
const RETURN: Guest = Guest(0x0020_0000);
const SP: Guest = Guest(0x0300_0000);
const SECOND_SP: Guest = Guest(SP.0 - 0x8000);
const TASK_CPSR: Cpsr = Cpsr(0xa800_0153);

fn scoped_layout() -> Layout {
    Layout::new(
        CodeRange::new(SEAM, Guest(SEAM.0 + 56)).unwrap(),
        CodeRange::new(OWNED, Guest(OWNED.0 + 0x100)).unwrap(),
    ).unwrap()
}

fn monitor<const DEPTH: usize>(layout: Layout) -> Monitor<DEPTH> {
    Monitor::new(layout, ObservationLimit(NonZeroU64::new(1000).unwrap()))
}

struct TaskMemory {
    bytes: BTreeMap<Guest, u8>,
    stack_error: Option<MemoryError>,
    creation: Guest,
}

impl TaskMemory {
    fn new(kernel: KernelId, id: u16) -> Self {
        let layout = source_layouts().into_iter().find(|layout| layout.kernel == kernel).unwrap();
        let node = Guest(layout.nodes.start().0 + 56 * u32::from(id - 1) | 0x4000_0000);
        let creation = Guest(layout.creations.start().0 + 40 * u32::from(id - 1) | 0x4000_0000);
        let mut memory = Self { bytes: BTreeMap::new(), stack_error: None, creation };
        for layout in source_layouts() {
            memory.put(layout.current.start(), node.0);
            memory.put(layout.context.start(), 0);
        }
        for offset in (0..56).step_by(4) { memory.put(Guest(node.0 + offset), 0); }
        memory.put(Guest(node.0 + 8), creation.0);
        for offset in (0..40).step_by(4) { memory.put(Guest(creation.0 + offset), 0); }
        memory.put(Guest(creation.0 + 16), 0x4000);
        memory.put(Guest(creation.0 + 20), SP.0 - 0x2000);
        memory.put(Guest(creation.0 + 32), u32::from(id));
        memory.put(Guest(creation.0 + 36), 1);
        memory
    }

    fn stack_at(mut self, sp: Guest) -> Self {
        self.put(Guest(self.creation.0 + 20), sp.0 - 0x2000);
        self
    }

    fn put(&mut self, address: Guest, word: u32) {
        for (offset, byte) in word.to_le_bytes().into_iter().enumerate() {
            self.bytes.insert(Guest((address.0 & !0x4000_0000) + offset as u32), byte);
        }
    }

    fn task(&self, core: usize, sp: Guest) -> Result<TaskExecution, TaskError> {
        modal_task::sample(self, CoreId(core), sp, TASK_CPSR)
    }
}

impl ValidatedTaskMemory for TaskMemory {
    fn read(&self, address: Guest, output: &mut [u8]) -> Result<(), MemoryError> {
        for (offset, byte) in output.iter_mut().enumerate() {
            *byte = *self.bytes.get(&Guest((address.0 & !0x4000_0000) + offset as u32))
                .ok_or(MemoryError::Unmapped)?;
        }
        Ok(())
    }

    fn validate_stack(&self, _extent: StackExtent) -> Result<(), MemoryError> {
        self.stack_error.map_or(Ok(()), Err)
    }
}

fn instruction_at(pc: Guest) -> EntryInstruction {
    if pc == STOCK_ENTRY { EntryInstruction::ScopedBranch } else { EntryInstruction::NotEntry }
}

fn entry(core: usize, controller: u32, sp: Guest, lr: Guest) -> Snapshot {
    let mut registers = std::array::from_fn(|index| 0x1234_0000 + index as u32);
    registers[0] = controller;
    Snapshot {
        core: CoreId(core), pc: STOCK_ENTRY, registers, sp, lr,
        cpsr: TASK_CPSR, saved_stock_frame: None,
        task: TaskMemory::new(if core == 1 { KernelId::Secondary } else { KernelId::Primary }, 1).task(core, sp),
        entry_instruction: EntryInstruction::ScopedBranch,
    }
}

fn handoff(layout: Layout, entry: Snapshot) -> Snapshot {
    let route = match entry.entry_instruction {
        EntryInstruction::OriginalPush => Route::StockFallback,
        _ => layout.policy().route(ControllerId(entry.registers[0])),
    };
    match route {
        Route::Owned75 => Snapshot {
            pc: layout.owned().unwrap().start(), entry_instruction: EntryInstruction::NotEntry, ..entry
        },
        Route::StockFallback => {
            let mut words = [0; 9];
            words[..8].copy_from_slice(&entry.registers[4..12]);
            words[8] = entry.lr.0;
            Snapshot {
                pc: STOCK_RESUME, sp: Guest(entry.sp.0 - 36),
                saved_stock_frame: Some(StockFrame(words)),
                entry_instruction: EntryInstruction::NotEntry, ..entry
            }
        }
    }
}

fn returned(entry: Snapshot) -> Snapshot {
    Snapshot { pc: entry.lr, entry_instruction: EntryInstruction::NotEntry, ..entry }
}

fn enter_and_handoff<const DEPTH: usize>(monitor: &mut Monitor<DEPTH>, layout: Layout, entry: Snapshot, route: Route) {
    assert_eq!(monitor.observe(entry), Ok(Observation::Entry { core: entry.core, route, arm_return: entry.lr }));
    assert_eq!(monitor.observe(handoff(layout, entry)), Ok(Observation::Handoff { core: entry.core, route }));
}

#[test]
fn layouts_validate_linked_ranges_and_keep_stock_addresses_fixed() {
    assert_eq!(STOCK_ENTRY, Guest(0x00bf_b028));
    assert_eq!(STOCK_RESUME, Guest(0x00bf_b02c));
    assert_eq!(STOCK_END, Guest(0x00bf_b124));
    for (start, end) in [(0, 4), (4, 4), (8, 4), (5, 12), (4, 11), (0xffff_fffc, 0)] {
        assert_eq!(CodeRange::new(Guest(start), Guest(end)), Err(LayoutError::InvalidCodeRange));
    }
    let owned = CodeRange::new(OWNED, Guest(OWNED.0 + 4)).unwrap();
    assert_eq!(Layout::new(CodeRange::new(SEAM, Guest(SEAM.0 + 52)).unwrap(), owned), Err(LayoutError::WrongSeamSize));
    let seam = CodeRange::new(SEAM, Guest(SEAM.0 + 56)).unwrap();
    assert_eq!(Layout::new(seam, CodeRange::new(SEAM, Guest(SEAM.0 + 4)).unwrap()), Err(LayoutError::OverlappingCode));
    assert_eq!(Layout::new(seam, CodeRange::new(STOCK_RESUME, STOCK_END).unwrap()), Err(LayoutError::OverlappingCode));
    assert_eq!(Layout::new(CodeRange::new(STOCK_ENTRY, Guest(STOCK_ENTRY.0 + 56)).unwrap(), owned), Err(LayoutError::OverlappingCode));
    let layout = scoped_layout();
    assert_eq!(layout.policy(), Policy::ScopedOwned75);
    assert_eq!(layout.seam().unwrap().end(), Guest(SEAM.0 + 56));
}

#[test]
fn baseline_needs_no_owned_addresses_and_routes_every_controller_to_stock() {
    let layout = Layout::stock_baseline();
    assert_eq!(layout.policy(), Policy::StockBaseline);
    assert_eq!(layout.seam(), None);
    assert_eq!(layout.owned(), None);
    let mut monitor = monitor::<2>(layout);
    for core in 0..2 {
        for controller in [0, 74, 75, 76, 0x1_004b, u32::MAX] {
            let call = Snapshot {
                entry_instruction: EntryInstruction::OriginalPush, ..entry(core, controller, SP, RETURN)
            };
            enter_and_handoff(&mut monitor, layout, call, Route::StockFallback);
            assert_eq!(monitor.observe(returned(call)), Ok(Observation::Return { core: CoreId(core), route: Route::StockFallback }));
        }
    }
    let receipt = monitor.finish().unwrap();
    assert_eq!(receipt.entries, Count(12));
    assert_eq!(receipt.owned_handoffs, Count(0));
    assert_eq!(receipt.fallback_handoffs, Count(12));
    assert_eq!(receipt.returns, Count(12));
    assert_eq!(receipt.observations, Count(36));
}

#[test]
fn scoped_policy_compares_the_full_controller_word_on_both_cores() {
    let layout = scoped_layout();
    let mut monitor = monitor::<2>(layout);
    for core in 0..2 {
        for (controller, route) in [(0, Route::StockFallback), (75, Route::Owned75), (0x1_004b, Route::StockFallback), (0xffff_004b, Route::StockFallback), (u32::MAX, Route::StockFallback)] {
            let call = entry(core, controller, SP, RETURN);
            enter_and_handoff(&mut monitor, layout, call, route);
            monitor.observe(returned(call)).unwrap();
        }
    }
    let receipt = monitor.finish().unwrap();
    assert_eq!(receipt.entries, Count(10));
    assert_eq!(receipt.owned_handoffs, Count(2));
    assert_eq!(receipt.fallback_handoffs, Count(8));
    assert_eq!(receipt.returns, Count(10));
    assert_eq!(receipt.active_frames, Depth(0));
}

#[test]
fn opposite_routes_can_nest_in_either_direction_on_either_core() {
    let layout = scoped_layout();
    for core in 0..2 {
        for (outer_controller, outer_route, inner_controller, inner_route, continuation) in [
            (75, Route::Owned75, 12, Route::StockFallback, Guest(OWNED.0 + 8)),
            (12, Route::StockFallback, 75, Route::Owned75, Guest(STOCK_RESUME.0 + 8)),
        ] {
            let mut monitor = monitor::<2>(layout);
            let outer = entry(core, outer_controller, SP, RETURN);
            enter_and_handoff(&mut monitor, layout, outer, outer_route);
            let inner = entry(core, inner_controller, Guest(SP.0 - 0x100), continuation);
            enter_and_handoff(&mut monitor, layout, inner, inner_route);
            assert_eq!(monitor.finish(), Err(Failure::ActiveFrames(Depth(2))));
            assert_eq!(monitor.observe(returned(inner)), Ok(Observation::Return {
                core: CoreId(core), route: inner_route,
            }));
            let body = Snapshot { pc: Guest(continuation.0 + 4), ..returned(inner) };
            assert_eq!(monitor.observe(body), Ok(Observation::Continue));
            monitor.observe(returned(outer)).unwrap();
            let receipt = monitor.finish().unwrap();
            assert_eq!(receipt.peak_core_depth, Depth(2));
            assert_eq!(receipt.owned_handoffs, Count(1));
            assert_eq!(receipt.fallback_handoffs, Count(1));
        }
    }
}

#[test]
fn baseline_can_nest_controller75_inside_another_stock_dispatch() {
    let layout = Layout::stock_baseline();
    let mut monitor = monitor::<2>(layout);
    let outer = Snapshot { entry_instruction: EntryInstruction::OriginalPush, ..entry(0, 12, SP, RETURN) };
    enter_and_handoff(&mut monitor, layout, outer, Route::StockFallback);
    let inner = Snapshot {
        entry_instruction: EntryInstruction::OriginalPush,
        ..entry(0, 75, Guest(SP.0 - 0x100), Guest(STOCK_RESUME.0 + 8))
    };
    enter_and_handoff(&mut monitor, layout, inner, Route::StockFallback);
    monitor.observe(returned(inner)).unwrap();
    monitor.observe(returned(outer)).unwrap();
    assert_eq!(monitor.finish().unwrap().fallback_handoffs, Count(2));
}

#[test]
fn cores_keep_independent_pending_handoffs_and_return_stacks() {
    let layout = scoped_layout();
    let mut monitor = monitor::<1>(layout);
    let first = entry(0, 75, SP, RETURN);
    let second = Snapshot {
        task: TaskMemory::new(KernelId::Secondary, 2).stack_at(SECOND_SP).task(1, SECOND_SP),
        ..entry(1, 3, SECOND_SP, RETURN)
    };
    monitor.observe(first).unwrap();
    enter_and_handoff(&mut monitor, layout, second, Route::StockFallback);
    monitor.observe(returned(second)).unwrap();
    monitor.observe(handoff(layout, first)).unwrap();
    monitor.observe(returned(first)).unwrap();
    let receipt = monitor.finish().unwrap();
    assert_eq!(receipt.peak_core_depth, Depth(1));
    assert_eq!(receipt.returns, Count(2));
}

#[test]
fn a_permit_on_one_core_cannot_authorize_the_other_core() {
    let layout = scoped_layout();
    let mut monitor = monitor::<2>(layout);
    let first = entry(0, 75, SP, RETURN);
    monitor.observe(first).unwrap();
    let other = Snapshot { core: CoreId(1), ..handoff(layout, first) };
    assert_eq!(monitor.observe(other), Err(Failure::TaskAttribution {
        core: CoreId(1), error: TaskError::SampledCoreMismatch { sampled: CoreId(0), observed: CoreId(1) },
    }));
}

#[test]
fn direct_entries_into_any_protected_body_or_seam_require_a_permit() {
    for pc in [STOCK_RESUME, Guest(STOCK_RESUME.0 + 4), Guest(STOCK_END.0 - 4), OWNED, Guest(OWNED.0 + 4), SEAM, Guest(SEAM.0 + 4)] {
        let mut monitor = monitor::<2>(scoped_layout());
        assert_eq!(monitor.observe(Snapshot { pc, entry_instruction: instruction_at(pc), ..entry(0, 75, SP, RETURN) }), Err(Failure::MissingPermit { core: CoreId(0), pc }));
    }
    let mut baseline = monitor::<1>(Layout::stock_baseline());
    assert!(matches!(baseline.observe(Snapshot { pc: STOCK_RESUME, entry_instruction: EntryInstruction::NotEntry, ..entry(0, 75, SP, RETURN) }), Err(Failure::MissingPermit { .. })));
}

#[test]
fn a_pending_permit_cannot_skip_the_exact_body_handoff() {
    let layout = scoped_layout();
    for controller in [75, 12] {
        let mut monitor = monitor::<1>(layout);
        let call = entry(0, controller, SP, RETURN);
        monitor.observe(call).unwrap();
        let handoff = handoff(layout, call);
        let interior = Snapshot { pc: Guest(handoff.pc.0 + 4), ..handoff };
        assert_eq!(monitor.observe(interior), Err(Failure::MissingPermit { core: CoreId(0), pc: interior.pc }));
    }
}

#[test]
fn the_matching_seam_is_allowed_only_before_handoff() {
    let layout = scoped_layout();
    for controller in [75, 12] {
        let mut monitor = monitor::<1>(layout);
        let call = entry(0, controller, SP, RETURN);
        monitor.observe(call).unwrap();
        assert_eq!(monitor.observe(Snapshot { pc: SEAM, entry_instruction: EntryInstruction::NotEntry, sp: Guest(SP.0 - 8), ..call }), Ok(Observation::Continue));
        monitor.observe(handoff(layout, call)).unwrap();
        assert_eq!(monitor.observe(Snapshot { pc: SEAM, entry_instruction: EntryInstruction::NotEntry, ..call }), Err(Failure::MissingPermit { core: CoreId(0), pc: SEAM }));
    }
}

#[test]
fn wrong_route_is_rejected_before_and_after_handoff() {
    let layout = scoped_layout();
    for controller in [75, 12] {
        for after_handoff in [false, true] {
            let mut monitor = monitor::<1>(layout);
            let call = entry(0, controller, SP, RETURN);
            monitor.observe(call).unwrap();
            if after_handoff { monitor.observe(handoff(layout, call)).unwrap(); }
            let (expected, observed, pc) = if controller == 75 {
                (Route::Owned75, Route::StockFallback, STOCK_RESUME)
            } else {
                (Route::StockFallback, Route::Owned75, OWNED)
            };
            assert_eq!(monitor.observe(Snapshot { pc, entry_instruction: instruction_at(pc), ..call }), Err(Failure::WrongRoute { core: CoreId(0), expected, observed }));
        }
    }
}

#[test]
fn every_general_register_must_match_at_both_handoffs() {
    let layout = scoped_layout();
    for controller in [75, 12] {
        for index in 0..13 {
            let mut monitor = monitor::<1>(layout);
            let call = entry(0, controller, SP, RETURN);
            monitor.observe(call).unwrap();
            let mut changed = handoff(layout, call);
            changed.registers[index] ^= 0x8000_0000;
            assert!(matches!(monitor.observe(changed), Err(Failure::HandoffState { component: StateComponent::General(_), .. })));
            assert_eq!(monitor.receipt().owned_handoffs, Count(0));
            assert_eq!(monitor.receipt().fallback_handoffs, Count(0));
        }
    }
}

#[test]
fn link_flags_and_control_state_must_match_at_both_handoffs() {
    let layout = scoped_layout();
    for controller in [75, 12] {
        for bit in [0, 6, 7, 8, 16, 19, 27, 28, 29, 30, 31] {
            let mut monitor = monitor::<1>(layout);
            let call = entry(0, controller, SP, RETURN);
            monitor.observe(call).unwrap();
            let mut changed = handoff(layout, call);
            changed.cpsr.0 ^= 1 << bit;
            let expected = if controller == 75 && matches!(bit, 0 | 7) {
                Failure::TaskAttribution { core: CoreId(0), error: TaskError::InvalidExecutionCpsr(changed.cpsr) }
            } else {
                Failure::HandoffState { core: CoreId(0), component: StateComponent::Cpsr }
            };
            assert_eq!(monitor.observe(changed), Err(expected));
        }
        let mut monitor = monitor::<1>(layout);
        let call = entry(0, controller, SP, RETURN);
        monitor.observe(call).unwrap();
        assert_eq!(monitor.observe(Snapshot { lr: Guest(RETURN.0 + 4), ..handoff(layout, call) }), Err(Failure::HandoffState { core: CoreId(0), component: StateComponent::LinkRegister }));
    }
}

#[test]
fn handoff_stack_must_match_the_route_specific_exact_value() {
    let layout = scoped_layout();
    for controller in [75, 12] {
        let mut monitor = monitor::<1>(layout);
        let call = entry(0, controller, SP, RETURN);
        monitor.observe(call).unwrap();
        let handoff = handoff(layout, call);
        let observed = Guest(handoff.sp.0 - 4);
        assert_eq!(monitor.observe(Snapshot { sp: observed, ..handoff }), Err(Failure::HandoffStack { core: CoreId(0), expected: handoff.sp, observed }));
    }
}

#[test]
fn fallback_requires_all_nine_saved_stock_words_in_address_order() {
    let layout = scoped_layout();
    for index in 0..9 {
        let mut monitor = monitor::<1>(layout);
        let call = entry(0, 12, SP, RETURN);
        monitor.observe(call).unwrap();
        let mut changed = handoff(layout, call);
        changed.saved_stock_frame.as_mut().unwrap().0[index] ^= 1;
        assert_eq!(monitor.observe(changed), Err(Failure::HandoffState { core: CoreId(0), component: StateComponent::StockFrame }));
    }
    let mut monitor = monitor::<1>(layout);
    let call = entry(0, 12, SP, RETURN);
    monitor.observe(call).unwrap();
    assert_eq!(monitor.observe(Snapshot { saved_stock_frame: None, ..handoff(layout, call) }), Err(Failure::MissingStockFrame(CoreId(0))));
}

#[test]
fn return_requires_exact_entry_stack_and_never_pops_on_wrong_stack() {
    let layout = scoped_layout();
    for (controller, route) in [(75, Route::Owned75), (12, Route::StockFallback)] {
        let mut monitor = monitor::<1>(layout);
        let call = entry(0, controller, SP, RETURN);
        enter_and_handoff(&mut monitor, layout, call, route);
        let observed = Guest(SP.0 - 4);
        assert_eq!(monitor.observe(Snapshot { sp: observed, ..returned(call) }), Err(Failure::WrongReturnStack { core: CoreId(0), expected: SP, observed }));
        assert_eq!(monitor.receipt().active_frames, Depth(1));
        assert_eq!(monitor.receipt().returns, Count(0));
    }
}

#[test]
fn an_outer_return_cannot_pop_an_unresolved_inner_call() {
    let layout = scoped_layout();
    for same_return_pc in [false, true] {
        let mut monitor = monitor::<2>(layout);
        let outer = entry(0, 75, SP, RETURN);
        enter_and_handoff(&mut monitor, layout, outer, Route::Owned75);
        let inner = entry(0, 12, Guest(SP.0 - 0x100), if same_return_pc { RETURN } else { Guest(OWNED.0 + 8) });
        enter_and_handoff(&mut monitor, layout, inner, Route::StockFallback);
        assert_eq!(monitor.observe(returned(outer)), Err(Failure::WrongReturnOrder(CoreId(0))));
        assert_eq!(monitor.receipt().active_frames, Depth(2));
    }
}

#[test]
fn an_unknown_continuation_does_not_pop_the_frame() {
    let layout = scoped_layout();
    let mut monitor = monitor::<1>(layout);
    let call = entry(0, 75, SP, RETURN);
    enter_and_handoff(&mut monitor, layout, call, Route::Owned75);
    assert_eq!(monitor.observe(Snapshot { pc: Guest(RETURN.0 + 4), entry_instruction: EntryInstruction::NotEntry, ..call }), Ok(Observation::Continue));
    assert_eq!(monitor.finish(), Err(Failure::ActiveFrames(Depth(1))));
    assert_eq!(monitor.receipt().returns, Count(0));
}

#[test]
fn pending_contexts_cannot_return_reenter_or_escape_before_handoff() {
    let layout = scoped_layout();
    for pc in [RETURN, STOCK_ENTRY, Guest(RETURN.0 + 4)] {
        let mut monitor = monitor::<2>(layout);
        let call = entry(0, 75, SP, RETURN);
        monitor.observe(call).unwrap();
        let next = Snapshot { pc, entry_instruction: instruction_at(pc), sp: Guest(SP.0 - 0x100), ..call };
        let expected = if pc == RETURN {
            Failure::WrongReturnStack { core: CoreId(0), expected: SP, observed: next.sp }
        } else {
            Failure::UnresolvedHandoff(CoreId(0))
        };
        assert_eq!(monitor.observe(next), Err(expected));
    }
    let mut monitor = monitor::<1>(layout);
    let call = entry(0, 75, SP, RETURN);
    monitor.observe(call).unwrap();
    assert_eq!(monitor.observe(returned(call)), Err(Failure::UnresolvedHandoff(CoreId(0))));
}

#[test]
fn nested_entry_must_be_below_the_parent_stack_and_stock_save_area() {
    let layout = scoped_layout();
    for (controller, route, nested_sp) in [(75, Route::Owned75, SP), (75, Route::Owned75, Guest(SP.0 + 8)), (12, Route::StockFallback, Guest(SP.0 - 32))] {
        let mut monitor = monitor::<2>(layout);
        let outer = entry(0, controller, SP, RETURN);
        enter_and_handoff(&mut monitor, layout, outer, route);
        assert_eq!(monitor.observe(entry(0, 75, nested_sp, Guest(RETURN.0 + 4))), Err(Failure::NonNestedContext(CoreId(0))));
    }
}

#[test]
fn nested_continuation_cannot_point_into_the_opposite_parent_route() {
    let layout = scoped_layout();
    let mut monitor = monitor::<2>(layout);
    let outer = entry(0, 75, SP, RETURN);
    enter_and_handoff(&mut monitor, layout, outer, Route::Owned75);
    assert_eq!(monitor.observe(entry(0, 12, Guest(SP.0 - 0x100), STOCK_RESUME)), Err(Failure::WrongRoute { core: CoreId(0), expected: Route::Owned75, observed: Route::StockFallback }));
}

#[test]
fn repeated_handoff_cannot_reuse_a_consumed_entry_permit() {
    let layout = scoped_layout();
    for (controller, route) in [(75, Route::Owned75), (12, Route::StockFallback)] {
        let mut monitor = monitor::<1>(layout);
        let call = entry(0, controller, SP, RETURN);
        enter_and_handoff(&mut monitor, layout, call, route);
        assert_eq!(monitor.observe(handoff(layout, call)), Err(Failure::UnexpectedHandoff(CoreId(0))));
    }
}

#[test]
fn invalid_core_arm_state_pc_stack_and_return_site_are_rejected() {
    let layout = scoped_layout();
    let call = entry(0, 75, SP, RETURN);
    let cases = [
        (Snapshot { core: CoreId(2), ..call }, Failure::InvalidCore(CoreId(2))),
        (Snapshot { pc: Guest(STOCK_ENTRY.0 + 2), ..call }, Failure::MisalignedPc(Guest(STOCK_ENTRY.0 + 2))),
        (Snapshot { sp: Guest(SP.0 - 2), ..call }, Failure::MisalignedStack(Guest(SP.0 - 2))),
        (Snapshot { sp: Guest(SP.0 - 4), ..call }, Failure::MisalignedStack(Guest(SP.0 - 4))),
        (Snapshot { sp: Guest(0), ..call }, Failure::WrappingStack(Guest(0))),
        (Snapshot { registers: entry(0, 12, SP, RETURN).registers, sp: Guest(32), ..call }, Failure::WrappingStack(Guest(32))),
        (Snapshot { cpsr: Cpsr(call.cpsr.0 | 1 << 5), ..call }, Failure::InvalidArmState(Cpsr(call.cpsr.0 | 1 << 5))),
        (Snapshot { cpsr: Cpsr(call.cpsr.0 | 1 << 24), ..call }, Failure::InvalidArmState(Cpsr(call.cpsr.0 | 1 << 24))),
    ];
    for (snapshot, failure) in cases {
        assert_eq!(monitor::<1>(layout).observe(snapshot), Err(failure));
    }
    for lr in [Guest(0), Guest(RETURN.0 + 1), STOCK_ENTRY, STOCK_RESUME, OWNED, SEAM, Guest(SEAM.0 + 4)] {
        assert_eq!(monitor::<1>(layout).observe(Snapshot { lr, ..call }), Err(Failure::InvalidReturnSite(lr)));
    }
}

#[test]
fn depth_bounds_fail_without_overwriting_live_frames() {
    let layout = scoped_layout();
    assert_eq!(monitor::<0>(layout).observe(entry(0, 75, SP, RETURN)), Err(Failure::DepthOverflow(CoreId(0))));
    let mut monitor = monitor::<1>(layout);
    let outer = entry(0, 75, SP, RETURN);
    enter_and_handoff(&mut monitor, layout, outer, Route::Owned75);
    assert_eq!(monitor.observe(entry(0, 12, Guest(SP.0 - 0x100), Guest(OWNED.0 + 8))), Err(Failure::DepthOverflow(CoreId(0))));
    assert_eq!(monitor.receipt().entries, Count(1));
    assert_eq!(monitor.receipt().active_frames, Depth(1));
}

#[test]
fn counter_bound_failure_is_sticky_and_preserves_the_last_receipt() {
    let layout = scoped_layout();
    let mut monitor = Monitor::<1>::new(layout, ObservationLimit(NonZeroU64::new(2).unwrap()));
    let call = entry(0, 75, SP, RETURN);
    enter_and_handoff(&mut monitor, layout, call, Route::Owned75);
    let receipt = *monitor.receipt();
    assert_eq!(monitor.observe(returned(call)), Err(Failure::CounterOverflow));
    assert_eq!(monitor.failure(), Some(Failure::CounterOverflow));
    assert_eq!(*monitor.receipt(), receipt);
    assert_eq!(monitor.observe(returned(call)), Err(Failure::CounterOverflow));
    assert_eq!(monitor.finish(), Err(Failure::CounterOverflow));
}

#[test]
fn finish_rejects_pending_and_running_frames_and_accepts_exact_handback() {
    let layout = scoped_layout();
    let mut monitor = monitor::<1>(layout);
    assert!(monitor.finish().is_ok());
    let call = entry(0, 75, SP, RETURN);
    monitor.observe(call).unwrap();
    assert_eq!(monitor.finish(), Err(Failure::ActiveFrames(Depth(1))));
    monitor.observe(handoff(layout, call)).unwrap();
    assert_eq!(monitor.finish(), Err(Failure::ActiveFrames(Depth(1))));
    monitor.observe(returned(call)).unwrap();
    assert!(monitor.finish().is_ok());
    assert_eq!(monitor.failure(), None);
}

#[test]
fn task_source_layouts_match_pinned_xm1_tables() {
    let layouts = source_layouts();
    assert_eq!(layouts[0].kernel, KernelId::Primary);
    assert_eq!(layouts[0].current.start(), Guest(0x0005_4760));
    assert_eq!(layouts[0].context.start(), Guest(0x0005_45a4));
    assert_eq!(layouts[0].nodes.start(), Guest(0x0005_ff0c));
    assert_eq!(layouts[0].creations.start(), Guest(0x0005_beac));
    assert_eq!(layouts[1].kernel, KernelId::Secondary);
    assert_eq!(layouts[1].current.start(), Guest(0x000e_4720));
    assert_eq!(layouts[1].context.start(), Guest(0x000e_4564));
    assert_eq!(layouts[1].nodes.start(), Guest(0x000e_fecc));
    assert_eq!(layouts[1].creations.start(), Guest(0x000e_be6c));
    for layout in layouts {
        assert_eq!(layout.current.bytes(), 4);
        assert_eq!(layout.context.bytes(), 4);
        assert_eq!(layout.nodes.bytes(), 56 * 120);
        assert_eq!(layout.creations.bytes(), 40 * 120);
    }
}

#[test]
fn pointer_namespace_must_be_local_to_the_sampled_execution_core() {
    for kernel in [KernelId::Primary, KernelId::Secondary] {
        for id in [1, 102, 120] {
            let memory = TaskMemory::new(kernel, id);
            for core in [0, 1] {
                if (core == 0) == (kernel == KernelId::Primary) {
                    let task = memory.task(core, SP).unwrap();
                    assert_eq!(task.identity().kernel, kernel);
                    assert_eq!(task.identity().id.get(), id);
                    assert_eq!(task.execution_core(), CoreId(core));
                    assert!(task.stack().validate_sp(Guest(SP.0 | 0x4000_0000)).is_ok());
                } else {
                    assert_eq!(memory.task(core, SP),
                        Err(TaskError::WrongKernel { core: CoreId(core), observed: kernel }));
                }
            }
        }
    }
}

#[test]
fn null_outside_and_non_element_current_pointers_are_refused() {
    for (pointer, kind) in [
        (0, 0), (0x4005_ff0d, 1), (0x4005_ff10, 1),
        (0x4005_ff08, 2), (0x4005_ff0c + 56 * 120, 2),
        (0x0005_ff0c, 2), (u32::MAX, 1),
    ] {
        let mut memory = TaskMemory::new(KernelId::Primary, 1);
        memory.put(KernelId::Primary.current_cell(), pointer);
        let error = memory.task(0, SP).unwrap_err();
        assert!(match kind {
            0 => matches!(error, TaskError::NullCurrent),
            1 => matches!(error, TaskError::MisalignedNode(_)),
            _ => matches!(error, TaskError::OutsideNodeArrays(_)),
        });
        let call = Snapshot { task: Err(error), ..entry(0, 75, SP, RETURN) };
        assert_eq!(monitor::<1>(scoped_layout()).observe(call),
            Err(Failure::TaskAttribution { core: CoreId(0), error }));
    }
}

#[test]
fn inconsistent_backlink_creation_id_and_created_flag_are_refused() {
    for (offset, value) in [(8, 0), (32, 2), (36, 0), (36, 2)] {
        let mut memory = TaskMemory::new(KernelId::Primary, 1);
        let owner = memory.task(0, SP).unwrap().identity();
        let address = if offset == 8 { owner.node.address() } else { owner.creation.address() };
        memory.put(Guest(address.0 + offset), value);
        let error = memory.task(0, SP).unwrap_err();
        assert!(match offset {
            8 => matches!(error, TaskError::WrongBacklink { .. }),
            32 => matches!(error, TaskError::WrongCreationId { .. }),
            _ => matches!(error, TaskError::NotCreated(_)),
        });
        assert!(matches!(monitor::<1>(scoped_layout()).observe(Snapshot {
            task: Err(error), ..entry(0, 75, SP, RETURN)
        }), Err(Failure::TaskAttribution { .. })));
    }
}

#[test]
fn invalid_stack_extents_live_sp_and_memory_views_are_refused() {
    for (base, bytes) in [
        (SP.0, 0), (SP.0, u32::MAX), (0x1fff_fff0, 32),
        (0xffff_fff0, 32), (0x2000_0000, 4),
    ] {
        let mut memory = TaskMemory::new(KernelId::Primary, 1);
        let creation = memory.task(0, SP).unwrap().identity().creation.address();
        memory.put(Guest(creation.0 + 16), bytes);
        memory.put(Guest(creation.0 + 20), base);
        assert!(matches!(memory.task(0, SP),
            Err(TaskError::InvalidExtent { .. } | TaskError::InvalidAddress(_))));
    }
    let memory = TaskMemory::new(KernelId::Primary, 1);
    for sp in [Guest(SP.0 - 0x2004), Guest(SP.0 + 0x2004), Guest(SP.0 + 2)] {
        assert!(matches!(memory.task(0, sp), Err(TaskError::StackPointerOutside { .. })));
    }
    for error in [MemoryError::Unmapped, MemoryError::Translation, MemoryError::ActiveTcm,
        MemoryError::AliasMismatch, MemoryError::PhysicalMismatch, MemoryError::Read] {
        let mut memory = TaskMemory::new(KernelId::Primary, 1);
        memory.stack_error = Some(error);
        assert_eq!(memory.task(0, SP), Err(TaskError::Memory(error)));
    }
}

#[test]
fn same_task_owned_continuation_nesting_and_returns_remain_valid() {
    let layout = scoped_layout();
    let mut monitor = monitor::<2>(layout);
    let outer = entry(0, 75, SP, RETURN);
    enter_and_handoff(&mut monitor, layout, outer, Route::Owned75);
    assert_eq!(monitor.observe(Snapshot { pc: Guest(OWNED.0 + 4), entry_instruction: EntryInstruction::NotEntry, ..outer }), Ok(Observation::Continue));
    let inner = entry(0, 75, Guest(SP.0 - 0x100), Guest(OWNED.0 + 8));
    enter_and_handoff(&mut monitor, layout, inner, Route::Owned75);
    monitor.observe(returned(inner)).unwrap();
    monitor.observe(returned(outer)).unwrap();
    assert_eq!(monitor.finish().unwrap().returns, Count(2));
}

#[test]
fn foreign_task_cannot_continue_handoff_nest_or_return_with_identical_sp_and_lr() {
    let layout = scoped_layout();
    for core in [0, 1] {
        let kernel = if core == 0 { KernelId::Primary } else { KernelId::Secondary };
        let foreign = TaskMemory::new(kernel, 2).task(core, SP).unwrap();
        for pc in [SEAM, OWNED, Guest(OWNED.0 + 4), STOCK_ENTRY, RETURN] {
            let mut monitor = monitor::<2>(layout);
            let call = entry(core, 75, SP, RETURN);
            monitor.observe(call).unwrap();
            if pc != SEAM && pc != OWNED { monitor.observe(handoff(layout, call)).unwrap(); }
            let before = *monitor.receipt();
            assert_eq!(monitor.observe(Snapshot { pc, entry_instruction: instruction_at(pc), task: Ok(foreign), ..call }),
                Err(Failure::OwnerMismatch {
                    core: CoreId(core), expected: call.task.unwrap().identity(), observed: foreign.identity(),
                }));
            assert_eq!(*monitor.receipt(), before);
        }
    }
}

#[test]
fn migration_is_refused_even_with_an_occupied_same_route_destination() {
    let layout = scoped_layout();
    for occupied in [false, true] {
        for pc in [STOCK_ENTRY, OWNED, Guest(OWNED.0 + 4), RETURN] {
            let mut monitor = monitor::<2>(layout);
            let source = entry(0, 75, SP, RETURN);
            enter_and_handoff(&mut monitor, layout, source, Route::Owned75);
            if occupied {
                let destination = Snapshot {
                    task: TaskMemory::new(KernelId::Secondary, 2).stack_at(SECOND_SP).task(1, SECOND_SP),
                    ..entry(1, 75, SECOND_SP, RETURN)
                };
                enter_and_handoff(&mut monitor, layout, destination, Route::Owned75);
            }
            let before = *monitor.receipt();
            let migrated = Snapshot { core: CoreId(1), pc, entry_instruction: instruction_at(pc), ..source };
            assert_eq!(monitor.observe(migrated), Err(Failure::TaskAttribution {
                core: CoreId(1), error: TaskError::SampledCoreMismatch { sampled: CoreId(0), observed: CoreId(1) },
            }));
            assert_eq!(*monitor.receipt(), before);
        }
    }
}

#[test]
fn mutable_scheduler_fields_and_selected_pointer_do_not_change_ownership() {
    let layout = scoped_layout();
    let mut memory = TaskMemory::new(KernelId::Primary, 1);
    let task = memory.task(0, SP).unwrap();
    let call = Snapshot { task: Ok(task), ..entry(0, 75, SP, RETURN) };
    let mut monitor = monitor::<1>(layout);
    enter_and_handoff(&mut monitor, layout, call, Route::Owned75);
    for (offset, value) in [(12, 0x00ff_ff05), (24, 0xdead_beef), (28, 0xcafe_babe)] {
        memory.put(Guest(task.identity().node.address().0 + offset), value);
    }
    memory.put(Guest(KernelId::Primary.current_cell().0 + 8), 0);
    let changed = memory.task(0, SP).unwrap();
    assert_eq!(monitor.observe(Snapshot { pc: Guest(OWNED.0 + 4), entry_instruction: EntryInstruction::NotEntry, task: Ok(changed), ..call }),
        Ok(Observation::Continue));
    monitor.observe(Snapshot { task: Ok(changed), ..returned(call) }).unwrap();
    assert!(monitor.finish().is_ok());
}

#[test]
fn missing_or_inconsistent_task_attribution_never_admits_owned_execution() {
    let layout = scoped_layout();
    for pc in [STOCK_ENTRY, SEAM, OWNED, Guest(OWNED.0 + 4), RETURN] {
        let mut monitor = monitor::<1>(layout);
        let call = entry(0, 75, SP, RETURN);
        if pc != STOCK_ENTRY { monitor.observe(call).unwrap(); }
        if pc == Guest(OWNED.0 + 4) || pc == RETURN {
            monitor.observe(handoff(layout, call)).unwrap();
        }
        assert_eq!(monitor.observe(Snapshot { pc, entry_instruction: instruction_at(pc), task: Err(TaskError::Unavailable), ..call }),
            Err(Failure::TaskAttribution { core: CoreId(0), error: TaskError::Unavailable }));
    }
    let mut monitor = monitor::<1>(layout);
    let call = Snapshot { task: Err(TaskError::Unavailable), ..entry(0, 12, SP, RETURN) };
    enter_and_handoff(&mut monitor, layout, call, Route::StockFallback);
    monitor.observe(returned(call)).unwrap();
    assert_eq!(monitor.finish().unwrap().owned_handoffs, Count(0));
}

#[test]
fn changed_creation_metadata_cannot_continue_an_existing_owner() {
    let layout = scoped_layout();
    for (offset, value) in [
        (0, 0x1234), (4, 0x1234), (8, 0x1234), (12, 0x1234),
        (16, 0x5000), (20, SP.0 - 0x3000), (24, 0x1234), (28, 0x1234),
    ] {
        let mut memory = TaskMemory::new(KernelId::Primary, 1);
        let owner = memory.task(0, SP).unwrap();
        let mut monitor = monitor::<1>(layout);
        let call = Snapshot { task: Ok(owner), ..entry(0, 75, SP, RETURN) };
        enter_and_handoff(&mut monitor, layout, call, Route::Owned75);
        memory.put(Guest(owner.identity().creation.address().0 + offset), value);
        assert_eq!(monitor.observe(Snapshot {
            pc: Guest(OWNED.0 + 4), entry_instruction: EntryInstruction::NotEntry, task: memory.task(0, SP), ..call
        }), Err(Failure::IdentityMetadataChanged(owner.identity())));
    }
}

#[test]
fn active_identity_extents_cover_full_creation_and_only_node_backlink() {
    let layout = scoped_layout();
    let mut monitor = monitor::<1>(layout);
    let call = entry(0, 75, SP, RETURN);
    let owner = call.task.unwrap();
    let creation = owner.identity().creation.address();
    let node = owner.identity().node.address();
    let write = |address| PhysicalExtent::new(address, 1).unwrap();
    assert!(!monitor.overlaps_active_identity(write(creation)));
    enter_and_handoff(&mut monitor, layout, call, Route::Owned75);
    for offset in 0..40 {
        assert!(monitor.overlaps_active_identity(write(Guest(creation.0 + offset))));
    }
    for offset in 0..56 {
        assert_eq!(monitor.overlaps_active_identity(write(Guest(node.0 + offset))), (8..12).contains(&offset));
    }
    for address in [Guest(creation.0 - 1), Guest(creation.0 + 40)] {
        assert!(!monitor.overlaps_active_identity(write(address)));
    }
    monitor.observe(returned(call)).unwrap();
    assert!(!monitor.overlaps_active_identity(write(creation)));
}

#[test]
fn metadata_writes_refuse_equal_value_reuse_and_remain_sticky() {
    let layout = scoped_layout();
    for backlink in [false, true] {
        let mut monitor = monitor::<1>(layout);
        let call = entry(0, 75, SP, RETURN);
        enter_and_handoff(&mut monitor, layout, call, Route::Owned75);
        let owner = call.task.unwrap().identity();
        let address = if backlink { Guest(owner.node.address().0 + 8) } else { owner.creation.address() };
        let write = PhysicalExtent::new(address, if backlink { 4 } else { 40 }).unwrap();
        let before = *monitor.receipt();
        let expected = Failure::ActiveIdentityWrite { owner, write };
        assert_eq!(monitor.observe_identity_write(write), Err(expected));
        assert_eq!(monitor.observe(returned(call)), Err(expected));
        assert_eq!(*monitor.receipt(), before);
    }
}

#[test]
fn writes_to_scheduler_fields_and_inactive_identities_keep_valid_permits() {
    let layout = scoped_layout();
    let mut monitor = monitor::<1>(layout);
    let call = entry(0, 75, SP, RETURN);
    enter_and_handoff(&mut monitor, layout, call, Route::Owned75);
    let owner = call.task.unwrap().identity();
    for (offset, bytes) in [(0, 8), (12, 44)] {
        let write = PhysicalExtent::new(Guest(owner.node.address().0 + offset), bytes).unwrap();
        assert_eq!(monitor.observe_identity_write(write), Ok(()));
    }
    let inactive = TaskMemory::new(KernelId::Primary, 2).task(0, SP).unwrap();
    for extent in inactive.identity_extents() {
        assert_eq!(monitor.observe_identity_write(extent), Ok(()));
    }
    monitor.observe(returned(call)).unwrap();
    for extent in call.task.unwrap().identity_extents() {
        assert_eq!(monitor.observe_identity_write(extent), Ok(()));
    }
    assert!(monitor.finish().is_ok());
}

#[test]
fn original_scoped_and_restored_entries_share_one_continuous_monitor() {
    let layout = scoped_layout();
    let mut monitor = monitor::<2>(layout);
    for instruction in [EntryInstruction::OriginalPush, EntryInstruction::ScopedBranch, EntryInstruction::OriginalPush] {
        let call = Snapshot { entry_instruction: instruction, ..entry(0, 75, SP, RETURN) };
        let route = if instruction == EntryInstruction::ScopedBranch { Route::Owned75 } else { Route::StockFallback };
        enter_and_handoff(&mut monitor, layout, call, route);
        monitor.observe(returned(call)).unwrap();
    }
    let receipt = monitor.finish().unwrap();
    assert_eq!(receipt.entries, Count(3));
    assert_eq!(receipt.owned_handoffs, Count(1));
    assert_eq!(receipt.fallback_handoffs, Count(2));
    assert_eq!(receipt.returns, Count(3));
}

#[test]
fn existing_frame_route_survives_entry_instruction_publication_changes() {
    let layout = scoped_layout();
    let mut monitor = monitor::<2>(layout);
    let original = Snapshot {
        entry_instruction: EntryInstruction::OriginalPush, ..entry(0, 75, SP, RETURN)
    };
    enter_and_handoff(&mut monitor, layout, original, Route::StockFallback);
    let scoped = entry(0, 75, Guest(SP.0 - 0x100), Guest(STOCK_RESUME.0 + 8));
    enter_and_handoff(&mut monitor, layout, scoped, Route::Owned75);
    monitor.observe(returned(scoped)).unwrap();
    monitor.observe(returned(original)).unwrap();
    assert_eq!(monitor.finish().unwrap().returns, Count(2));
}

#[test]
fn inconsistent_entry_instruction_declarations_are_refused() {
    for (layout, pc, instruction) in [
        (scoped_layout(), STOCK_ENTRY, EntryInstruction::NotEntry),
        (Layout::stock_baseline(), STOCK_ENTRY, EntryInstruction::ScopedBranch),
        (scoped_layout(), OWNED, EntryInstruction::OriginalPush),
        (scoped_layout(), RETURN, EntryInstruction::ScopedBranch),
    ] {
        let mut monitor = monitor::<1>(layout);
        assert_eq!(monitor.observe(Snapshot {
            pc, entry_instruction: instruction, ..entry(0, 75, SP, RETURN)
        }), Err(Failure::InvalidEntryInstruction { pc, instruction }));
        assert_eq!(monitor.receipt().entries, Count(0));
    }
}

#[test]
fn restored_creation_values_do_not_revive_invalidated_ownership() {
    let layout = scoped_layout();
    let mut memory = TaskMemory::new(KernelId::Primary, 1);
    let owner = memory.task(0, SP).unwrap();
    let call = Snapshot { task: Ok(owner), ..entry(0, 75, SP, RETURN) };
    let mut monitor = monitor::<1>(layout);
    enter_and_handoff(&mut monitor, layout, call, Route::Owned75);
    let created = Guest(owner.identity().creation.address().0 + 36);
    let write = PhysicalExtent::new(created, 4).unwrap();
    let failure = Failure::ActiveIdentityWrite { owner: owner.identity(), write };
    assert_eq!(monitor.observe_identity_write(write), Err(failure));
    memory.put(created, 0);
    assert_eq!(memory.task(0, SP), Err(TaskError::NotCreated(0)));
    memory.put(created, 1);
    assert_eq!(monitor.observe(Snapshot { task: memory.task(0, SP), ..returned(call) }), Err(failure));
    assert_eq!(monitor.receipt().active_frames, Depth(1));
    assert_eq!(monitor.receipt().returns, Count(0));
}

#[test]
fn live_sp_outside_owned_stack_is_refused_even_with_an_unchanged_sample() {
    let layout = scoped_layout();
    let mut monitor = monitor::<1>(layout);
    let call = entry(0, 75, SP, RETURN);
    enter_and_handoff(&mut monitor, layout, call, Route::Owned75);
    assert!(matches!(monitor.observe(Snapshot {
        pc: Guest(OWNED.0 + 4), entry_instruction: EntryInstruction::NotEntry,
        sp: Guest(SP.0 + 0x2004), ..call
    }), Err(Failure::TaskAttribution { error: TaskError::StackPointerOutside { .. }, .. })));
    assert_eq!(monitor.receipt().active_frames, Depth(1));
}

#[test]
fn identity_write_query_includes_owners_on_both_cores_and_spanning_writes() {
    let layout = scoped_layout();
    let mut monitor = monitor::<1>(layout);
    let first = entry(0, 75, SP, RETURN);
    let second = Snapshot {
        task: TaskMemory::new(KernelId::Secondary, 1).stack_at(SECOND_SP).task(1, SECOND_SP),
        ..entry(1, 75, SECOND_SP, RETURN)
    };
    enter_and_handoff(&mut monitor, layout, first, Route::Owned75);
    enter_and_handoff(&mut monitor, layout, second, Route::Owned75);
    for call in [first, second] {
        let owner = call.task.unwrap();
        for extent in owner.identity_extents() {
            assert!(monitor.overlaps_active_identity(extent));
            assert!(monitor.overlaps_active_identity(
                PhysicalExtent::new(Guest(extent.start().0 - 1), 2).unwrap()
            ));
        }
        monitor.observe(returned(call)).unwrap();
        for extent in owner.identity_extents() {
            assert!(!monitor.overlaps_active_identity(extent));
        }
    }
    assert!(monitor.finish().is_ok());
}

#[test]
fn original_controller75_never_borrows_an_owned_permit_from_linked_layout() {
    let layout = scoped_layout();
    let mut monitor = monitor::<1>(layout);
    let call = Snapshot {
        entry_instruction: EntryInstruction::OriginalPush,
        task: Err(TaskError::Unavailable), ..entry(0, 75, SP, RETURN)
    };
    monitor.observe(call).unwrap();
    let creation = TaskMemory::new(KernelId::Primary, 1).task(0, SP).unwrap().identity_extents()[0];
    assert!(!monitor.overlaps_active_identity(creation));
    assert_eq!(monitor.observe(Snapshot {
        pc: OWNED, entry_instruction: EntryInstruction::NotEntry, ..call
    }), Err(Failure::WrongRoute {
        core: CoreId(0), expected: Route::StockFallback, observed: Route::Owned75,
    }));
    assert_eq!(monitor.receipt().owned_handoffs, Count(0));
}

#[test]
fn only_zero_context_irq_enabled_little_endian_a32_svc_execution_is_admitted() {
    for (core, kernel) in [(0, KernelId::Primary), (1, KernelId::Secondary)] {
        let mut memory = TaskMemory::new(kernel, 1);
        for cpsr in [Cpsr(0x13), Cpsr(0x53), TASK_CPSR, Cpsr(0xf800_0153)] {
            assert!(modal_task::sample(&memory, CoreId(core), SP, cpsr).is_ok());
        }
        for bit in [5, 7, 9, 24] {
            let cpsr = Cpsr(TASK_CPSR.0 | (1 << bit));
            assert_eq!(modal_task::sample(&memory, CoreId(core), SP, cpsr),
                Err(TaskError::InvalidExecutionCpsr(cpsr)));
        }
        for mode in [0, 0x10, 0x11, 0x12, 0x16, 0x17, 0x1b, 0x1f] {
            let cpsr = Cpsr((TASK_CPSR.0 & !0x1f) | mode);
            assert_eq!(modal_task::sample(&memory, CoreId(core), SP, cpsr),
                Err(TaskError::InvalidExecutionCpsr(cpsr)));
        }
        for depth in [1, 2, u32::MAX] {
            memory.put(kernel.context_cell(), depth);
            assert_eq!(memory.task(core, SP), Err(TaskError::ContextNonzero { kernel, depth }));
        }
        memory.put(kernel.context_cell(), 0);
        assert!(memory.task(core, SP).is_ok());
    }
}

#[test]
fn lazy_vfp_owner_words_are_unowned_or_created_tasks_and_the_current_task_needs_no_svc_context() {
    for (core, kernel) in [(0, KernelId::Primary), (1, KernelId::Secondary)] {
        let mut memory = TaskMemory::new(kernel, 7);
        memory.put(kernel.context_cell(), 1);
        let current = modal_task::current_task_id(&memory, CoreId(core)).unwrap();
        assert_eq!(current.get(), 7);
        assert_eq!(modal_task::vfp_owner(&memory, kernel, VfpOwner::UNOWNED), Ok(VfpOwner::Unowned));
        assert_eq!(modal_task::vfp_owner(&memory, kernel, 7), Ok(VfpOwner::Task(current)));
        for value in [0, 121, 0x8000_0007] {
            assert_eq!(modal_task::vfp_owner(&memory, kernel, value), Err(TaskError::InvalidVfpOwner(value)));
        }
        assert!(modal_task::vfp_owner(&memory, kernel, 8).is_err());
        memory.put(Guest(memory.creation.0 + 36), 0);
        assert_eq!(modal_task::vfp_owner(&memory, kernel, 7), Err(TaskError::NotCreated(0)));
        assert_eq!(modal_task::current_task_id(&memory, CoreId(core)), Err(TaskError::NotCreated(0)));
    }
    let memory = TaskMemory::new(KernelId::Secondary, 7);
    assert_eq!(modal_task::current_task_id(&memory, CoreId(0)),
        Err(TaskError::WrongKernel { core: CoreId(0), observed: KernelId::Secondary }));
}

#[test]
fn nonzero_svc_handler_context_cannot_enter_continue_or_return_owned_code() {
    let layout = scoped_layout();
    for pc in [STOCK_ENTRY, SEAM, OWNED, Guest(OWNED.0 + 4), RETURN] {
        let mut memory = TaskMemory::new(KernelId::Primary, 1);
        let call = Snapshot { task: memory.task(0, SP), ..entry(0, 75, SP, RETURN) };
        let mut monitor = monitor::<1>(layout);
        if pc != STOCK_ENTRY { monitor.observe(call).unwrap(); }
        if pc == Guest(OWNED.0 + 4) || pc == RETURN {
            monitor.observe(handoff(layout, call)).unwrap();
        }
        memory.put(KernelId::Primary.context_cell(), 1);
        let before = *monitor.receipt();
        assert_eq!(monitor.observe(Snapshot {
            pc, entry_instruction: instruction_at(pc), task: memory.task(0, SP), ..call
        }), Err(Failure::TaskAttribution {
            core: CoreId(0), error: TaskError::ContextNonzero { kernel: KernelId::Primary, depth: 1 },
        }));
        assert_eq!(*monitor.receipt(), before);
    }
}

#[test]
fn scheduler_current_stack_transition_gap_requires_irq_masking_to_end() {
    let memory = TaskMemory::new(KernelId::Primary, 2).stack_at(SECOND_SP);
    let masked = Cpsr(TASK_CPSR.0 | 0x80);
    for sp in [SP, SECOND_SP] {
        assert_eq!(modal_task::sample(&memory, CoreId(0), sp, masked),
            Err(TaskError::InvalidExecutionCpsr(masked)));
    }
    assert!(matches!(memory.task(0, SP), Err(TaskError::StackPointerOutside { .. })));
    assert!(memory.task(0, SECOND_SP).is_ok());
}

#[test]
fn changing_nzcv_does_not_change_task_ownership_after_handoff() {
    let layout = scoped_layout();
    let memory = TaskMemory::new(KernelId::Primary, 1);
    let call = entry(0, 75, SP, RETURN);
    let mut monitor = monitor::<1>(layout);
    enter_and_handoff(&mut monitor, layout, call, Route::Owned75);
    for flags in [0, 0x1000_0000, 0xa000_0000, 0xf000_0000] {
        let cpsr = Cpsr((TASK_CPSR.0 & !0xf000_0000) | flags);
        assert_eq!(monitor.observe(Snapshot {
            pc: Guest(OWNED.0 + 4), entry_instruction: EntryInstruction::NotEntry,
            cpsr, task: modal_task::sample(&memory, CoreId(0), SP, cpsr), ..call
        }), Ok(Observation::Continue));
    }
    monitor.observe(returned(call)).unwrap();
    assert!(monitor.finish().is_ok());
}

#[test]
fn copied_stack_migration_is_refused_despite_different_valid_identity_metadata() {
    let layout = scoped_layout();
    for occupied in [false, true] {
        for pc in [STOCK_ENTRY, OWNED, Guest(OWNED.0 + 4), RETURN] {
            let mut monitor = monitor::<1>(layout);
            let source = entry(0, 75, SP, RETURN);
            enter_and_handoff(&mut monitor, layout, source, Route::Owned75);
            if occupied {
                let destination = Snapshot {
                    task: TaskMemory::new(KernelId::Secondary, 2).stack_at(SECOND_SP).task(1, SECOND_SP),
                    ..entry(1, 75, SECOND_SP, RETURN)
                };
                enter_and_handoff(&mut monitor, layout, destination, Route::Owned75);
            }
            let copied = TaskMemory::new(KernelId::Secondary, 3).task(1, SP).unwrap();
            let before = *monitor.receipt();
            assert_eq!(monitor.observe(Snapshot {
                core: CoreId(1), pc, entry_instruction: instruction_at(pc), task: Ok(copied), ..source
            }), Err(Failure::CrossCoreStack {
                owner: source.task.unwrap().identity(), observed: copied.identity(),
                active_core: CoreId(0), observed_core: CoreId(1),
            }));
            assert_eq!(*monitor.receipt(), before);
        }
    }
}

fn helper_layout() -> Layout {
    scoped_layout().with_helpers([
        CodeRange::new(Guest(0x0100_2000), Guest(0x0100_2040)).unwrap(),
        CodeRange::new(Guest(0x0100_3000), Guest(0x0100_3040)).unwrap(),
    ]).unwrap()
}

#[test]
fn helper_layouts_require_linked_code_and_disjoint_extents() {
    let mut configured = helper_layout().helpers();
    let helpers = [configured.next().unwrap(), configured.next().unwrap()];
    assert!(configured.next().is_none());
    assert!(scoped_layout().helpers().next().is_none());
    assert!(Layout::stock_baseline().helpers().next().is_none());
    assert_eq!(Layout::stock_baseline().with_helpers(helpers), Err(LayoutError::HelpersWithoutLinkedLayout));
    assert!(scoped_layout().with_helpers(helpers).unwrap().helpers().eq(helpers));
    for overlap in [
        CodeRange::new(STOCK_ENTRY, STOCK_END).unwrap(),
        scoped_layout().seam().unwrap(), scoped_layout().owned().unwrap(), helpers[1],
    ] {
        assert_eq!(scoped_layout().with_helpers([overlap, helpers[1]]), Err(LayoutError::OverlappingCode));
    }
    for overlap in [
        CodeRange::new(STOCK_ENTRY, STOCK_END).unwrap(),
        scoped_layout().seam().unwrap(), scoped_layout().owned().unwrap(), helpers[0],
    ] {
        assert_eq!(scoped_layout().with_helpers([helpers[0], overlap]), Err(LayoutError::OverlappingCode));
    }
    let end = scoped_layout().owned().unwrap().end();
    assert!(scoped_layout().with_helpers([
        CodeRange::new(end, Guest(end.0 + 0x40)).unwrap(),
        CodeRange::new(Guest(end.0 + 0x40), Guest(end.0 + 0x80)).unwrap(),
    ]).is_ok());
}

#[test]
fn direct_helper_entries_and_interiors_require_a_permit() {
    let layout = helper_layout();
    for helper in layout.helpers() {
        for address in (helper.start().0..helper.end().0).step_by(4) {
            let pc = Guest(address);
            let mut monitor = monitor::<1>(layout);
            assert_eq!(monitor.observe(Snapshot {
                pc, entry_instruction: EntryInstruction::NotEntry, ..entry(0, 75, SP, RETURN)
            }), Err(Failure::MissingPermit { core: CoreId(0), pc }));
        }
        let mut monitor = monitor::<1>(layout);
        assert_eq!(monitor.observe(entry(0, 75, SP, helper.start())),
            Err(Failure::InvalidReturnSite(helper.start())));
    }
}

#[test]
fn helpers_cannot_resolve_an_owned_dispatch_handoff() {
    let layout = helper_layout();
    for helper in layout.helpers() {
        for pc in [helper.start(), Guest(helper.start().0 + 4), Guest(helper.end().0 - 4)] {
            let mut monitor = monitor::<1>(layout);
            let call = entry(0, 75, SP, RETURN);
            monitor.observe(call).unwrap();
            let before = *monitor.receipt();
            assert_eq!(monitor.observe(Snapshot {
                pc, entry_instruction: EntryInstruction::NotEntry, ..call
            }), Err(Failure::MissingPermit { core: CoreId(0), pc }));
            assert_eq!(*monitor.receipt(), before);
        }
    }
}

#[test]
fn fallback_frames_cannot_enter_owned_helpers_before_or_after_handoff() {
    let layout = helper_layout();
    for helper in layout.helpers() {
        for pc in [helper.start(), Guest(helper.end().0 - 4)] {
            for resolved in [false, true] {
                for original in [false, true] {
                    let mut monitor = monitor::<1>(layout);
                    let call = if original {
                        Snapshot { entry_instruction: EntryInstruction::OriginalPush, ..entry(0, 75, SP, RETURN) }
                    } else {
                        entry(0, 12, SP, RETURN)
                    };
                    monitor.observe(call).unwrap();
                    if resolved { monitor.observe(handoff(layout, call)).unwrap(); }
                    assert_eq!(monitor.observe(Snapshot {
                        pc, entry_instruction: EntryInstruction::NotEntry, ..call
                    }), Err(Failure::WrongRoute {
                        core: CoreId(0), expected: Route::StockFallback, observed: Route::Owned75,
                    }));
                }
            }
        }
    }
}

#[test]
fn running_owned_frames_can_execute_all_helper_words_and_nested_helper_returns() {
    let layout = helper_layout();
    for helper in layout.helpers() {
        let mut monitor = monitor::<2>(layout);
        let outer = entry(0, 75, SP, RETURN);
        enter_and_handoff(&mut monitor, layout, outer, Route::Owned75);
        for address in (helper.start().0..helper.end().0).step_by(4) {
            assert_eq!(monitor.observe(Snapshot {
                pc: Guest(address), entry_instruction: EntryInstruction::NotEntry, ..outer
            }), Ok(Observation::Continue));
        }
        for controller in [75, 12] {
            let inner = entry(0, controller, Guest(SP.0 - 0x100), Guest(helper.start().0 + 4));
            let route = if controller == 75 { Route::Owned75 } else { Route::StockFallback };
            enter_and_handoff(&mut monitor, layout, inner, route);
            assert_eq!(monitor.observe(returned(inner)), Ok(Observation::Return { core: CoreId(0), route }));
        }
        monitor.observe(returned(outer)).unwrap();
        assert_eq!(monitor.finish().unwrap().returns, Count(3));
    }
}

#[test]
fn helper_execution_requires_the_running_frames_validated_owner() {
    let layout = helper_layout();
    for helper in layout.helpers() {
        for unavailable in [false, true] {
            let mut monitor = monitor::<1>(layout);
            let call = entry(0, 75, SP, RETURN);
            enter_and_handoff(&mut monitor, layout, call, Route::Owned75);
            let task = if unavailable {
                Err(TaskError::Unavailable)
            } else {
                TaskMemory::new(KernelId::Primary, 2).task(0, SP)
            };
            let expected = if unavailable {
                Failure::TaskAttribution { core: CoreId(0), error: TaskError::Unavailable }
            } else {
                Failure::OwnerMismatch {
                    core: CoreId(0), expected: call.task.unwrap().identity(), observed: task.unwrap().identity(),
                }
            };
            assert_eq!(monitor.observe(Snapshot {
                pc: Guest(helper.start().0 + 4), entry_instruction: EntryInstruction::NotEntry, task, ..call
            }), Err(expected));
            assert_eq!(monitor.receipt().active_frames, Depth(1));
        }
    }
}

#[test]
fn owned_frame_query_tracks_pending_frames_and_owned_ancestors_under_fallback() {
    let layout = scoped_layout();
    for core in [0, 1] {
        for (outer_controller, outer_route, inner_controller) in [
            (75, Route::Owned75, 12),
            (12, Route::StockFallback, 75),
        ] {
            let mut monitor = monitor::<2>(layout);
            assert!(!monitor.has_owned_frame(CoreId(core)));
            assert!(!monitor.has_owned_frame(CoreId(2)));
            let outer = entry(core, outer_controller, SP, RETURN);
            monitor.observe(outer).unwrap();
            assert_eq!(monitor.has_owned_frame(CoreId(core)), outer_route == Route::Owned75);
            monitor.observe(handoff(layout, outer)).unwrap();
            let continuation = match outer_route {
                Route::Owned75 => Guest(OWNED.0 + 8),
                Route::StockFallback => Guest(STOCK_RESUME.0 + 8),
            };
            let inner = entry(core, inner_controller, Guest(SP.0 - 0x100), continuation);
            monitor.observe(inner).unwrap();
            assert!(monitor.has_owned_frame(CoreId(core)));
            assert!(!monitor.has_owned_frame(CoreId(1 - core)));
            monitor.observe(handoff(layout, inner)).unwrap();
            assert!(monitor.has_owned_frame(CoreId(core)));
            monitor.observe(returned(inner)).unwrap();
            assert_eq!(monitor.has_owned_frame(CoreId(core)), outer_route == Route::Owned75);
            monitor.observe(returned(outer)).unwrap();
            assert!(!monitor.has_owned_frame(CoreId(core)));
            assert!(monitor.finish().is_ok());
            assert_eq!(monitor.receipt().owned_handoffs, Count(1));
            assert_eq!(monitor.receipt().fallback_handoffs, Count(1));
        }
    }
}
