//! Synthetic register snapshots exercise the pure preexecution contract.

use fujimod_xm1::{modal_permit::*, provenance::Guest};
use std::num::NonZeroU64;

const SEAM: Guest = Guest(0x0100_0000);
const OWNED: Guest = Guest(0x0100_1000);
const RETURN: Guest = Guest(0x0020_0000);
const SP: Guest = Guest(0x0300_0000);

fn scoped_layout() -> Layout {
    Layout::new(
        CodeRange::new(SEAM, Guest(SEAM.0 + 56)).unwrap(),
        CodeRange::new(OWNED, Guest(OWNED.0 + 0x100)).unwrap(),
    ).unwrap()
}

fn monitor<const DEPTH: usize>(layout: Layout) -> Monitor<DEPTH> {
    Monitor::new(layout, ObservationLimit(NonZeroU64::new(1000).unwrap()))
}

fn entry(core: usize, controller: u32, sp: Guest, lr: Guest) -> Snapshot {
    let mut registers = std::array::from_fn(|index| 0x1234_0000 + index as u32);
    registers[0] = controller;
    Snapshot {
        core: CoreId(core), pc: STOCK_ENTRY, registers, sp, lr,
        cpsr: Cpsr(0xa800_01d3), saved_stock_frame: None,
    }
}

fn handoff(layout: Layout, entry: Snapshot) -> Snapshot {
    match layout.policy().route(ControllerId(entry.registers[0])) {
        Route::Owned75 => Snapshot { pc: layout.owned().unwrap().start(), ..entry },
        Route::StockFallback => {
            let mut words = [0; 9];
            words[..8].copy_from_slice(&entry.registers[4..12]);
            words[8] = entry.lr.0;
            Snapshot {
                pc: STOCK_RESUME, sp: Guest(entry.sp.0 - 36),
                saved_stock_frame: Some(StockFrame(words)), ..entry
            }
        }
    }
}

fn returned(entry: Snapshot) -> Snapshot { Snapshot { pc: entry.lr, ..entry } }

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
            let call = entry(core, controller, SP, RETURN);
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
    let outer = entry(0, 12, SP, RETURN);
    enter_and_handoff(&mut monitor, layout, outer, Route::StockFallback);
    let inner = entry(0, 75, Guest(SP.0 - 0x100), Guest(STOCK_RESUME.0 + 8));
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
    let second = entry(1, 3, SP, RETURN);
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
    assert_eq!(monitor.observe(other), Err(Failure::MissingPermit { core: CoreId(1), pc: OWNED }));
}

#[test]
fn direct_entries_into_any_protected_body_or_seam_require_a_permit() {
    for pc in [STOCK_RESUME, Guest(STOCK_RESUME.0 + 4), Guest(STOCK_END.0 - 4), OWNED, Guest(OWNED.0 + 4), SEAM, Guest(SEAM.0 + 4)] {
        let mut monitor = monitor::<2>(scoped_layout());
        assert_eq!(monitor.observe(Snapshot { pc, ..entry(0, 75, SP, RETURN) }), Err(Failure::MissingPermit { core: CoreId(0), pc }));
    }
    let mut baseline = monitor::<1>(Layout::stock_baseline());
    assert!(matches!(baseline.observe(Snapshot { pc: STOCK_RESUME, ..entry(0, 75, SP, RETURN) }), Err(Failure::MissingPermit { .. })));
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
        assert_eq!(monitor.observe(Snapshot { pc: SEAM, sp: Guest(SP.0 - 8), ..call }), Ok(Observation::Continue));
        monitor.observe(handoff(layout, call)).unwrap();
        assert_eq!(monitor.observe(Snapshot { pc: SEAM, ..call }), Err(Failure::MissingPermit { core: CoreId(0), pc: SEAM }));
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
            assert_eq!(monitor.observe(Snapshot { pc, ..call }), Err(Failure::WrongRoute { core: CoreId(0), expected, observed }));
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
            assert_eq!(monitor.observe(changed), Err(Failure::HandoffState { core: CoreId(0), component: StateComponent::Cpsr }));
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
    assert_eq!(monitor.observe(Snapshot { pc: Guest(RETURN.0 + 4), ..call }), Ok(Observation::Continue));
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
        let next = Snapshot { pc, sp: Guest(SP.0 - 0x100), ..call };
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
