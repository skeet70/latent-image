# Calculation-library bindings

The composition root owns the bindings. This crate exposes typed Rust operations, not replicas of the stock forwarders. Ports are explicit dependencies; `MotionState` and `ExposureContext` replace stock global blocks. Addresses below identify X-M1 1.50 interfaces only. No owned-to-owned C ABI is needed.

Grounding: x70view `55436fc85c0560dd252b0410af6f2427fe198cd3`, with NONMATCHING definitions checked against id5 instructions. The 44 forwarder bodies were inspected for argument insertion, copying, narrowing and output placement. This is a static binding audit, not stock execution or subsystem qualification. Pending rows name implementation dependencies, not stock fallbacks provided by this crate.

## The shot's ExposureCalculation surface

The shot's adapter holds one `exposure::context::ExposureContext`. Public state fields are the read accessors; do not add a second state snapshot just to implement the shot trait.

| Shot operation | Calculation-library binding |
| --- | --- |
| selection | `record_policy::exposure_mode`; use its `ExposureMode` value and code |
| selects_primary / selects_secondary | `ExposureMode::is_primary` / `ExposureMode::uses_adjustment` |
| prepare | `exposure::prepare::prepare(ports, context, binding)`; map every `ExposureSetup` field to `Binding`, including both record handles and the selection as `exposure_mode` |
| calculation_mode | `Profile::chosen_word(record)`; 0xf51b50 returns the same value as 0xf4f16c |
| correction_mode | `Profile::selection_kind(record, refinement_pass)` |
| scaled_statistics | false; the stock 0xf5f3a8 returns zero |
| evaluation_areas | `stock::exposure::evaluation_areas(&context.areas)`; eight 20-byte records, even when the count is smaller |
| evaluation_area_count | `context.selection.count` |
| evaluated_count | `context.selection.evaluated_count()`; all eight slots, not just the current count or a leading prefix |
| select_measured / check_measured | `RowIndex::new(entry.0)` and `exposure::evaluation::measured_row`; its buffers, column, pairs, regions, reply and statistics go to the image task's check |
| accept_measured | `context.selection.enable_row(row.get())` |
| next_evaluation | `context.selection.open_row()`; map None to the shot's final index 0xff |
| request_evaluation | `exposure::evaluation::request_row` with a checked `RowIndex` and the completion callback |
| absorb_evaluation | `context.selection.apply(RowUpdate)`; convert the returned row and both range-limit pairs |
| correction_needed | `context.selection.range_is_valid(settings)` |
| request_correction | `exposure::correction::post`; map `CorrectionPost::{Posted,Skipped}` to the shot's result |
| levels | base = `context.levels.target`, reference = `context.levels.start as u16`; the reference participates in wrapping relative-level arithmetic |
| alternate | `context.outcome.treated` |
| variant | `u32::from(context.outcome.first_treatment)` |
| exporting | `context.outcome.focus_reused` |
| export | `diagnostics::export_correction(ports, frame, correction, &CorrectionExport { first_treatment: context.outcome.first_treatment, subjects: context.snapshot.focus.subject_count as u16, clear_samples: kind != Plain })` |
| clear_selection | `diagnostics::clear_selection(ports, frame, primary)` |
| finish_primary | `adjust::context_trace::trace(ports, context)` |

For the mode-7 half press, the shot deliberately skips preparation. Its adapter still updates `context.binding` before `correction::post`; retained tables are not a retained binding.

The remaining shot bindings are the existing typed request, calculation-input, grid, gain and finish interfaces. In particular, `exposure::calculation::build_input` takes the plan's kind and five exposure halves, `stock::exposure` in the interface crate encodes its `CalculationInput`, and `gains::focus_blend`, `lens_checks::point_zone` and `finish::target_selection` take the decoded shot queries directly.

## Selection flags

These are the enclosing functions, not functions named after their BL instructions:

| Stock behavior | Owned operation | Adapter obligations |
| --- | --- | --- |
| 0xf692dc, shared flag-input construction | internal to `exposure::flags` | Pass the complete `CalculationInput`; it already contains the record's decoded fields. `FlagQuery` supplies lens reference, readiness, positions, index and signed limit offset. |
| 0xf69418, including the call at 0xf69504 | `flags::selection_spread` | Pass `Levels` and `FlagQuery`; result is `(spread_bit, LimitLevels)`. Serialize no second limit as 0x8000 if a retained consumer needs the optional two-half output. `0xf60198` is the same behavior after unpacking the shot work block. |
| 0xf69514, including calls at 0xf69660 and 0xf69750 | `flags::area_flags` | Pass rows and the optional class-5 presence list with matching lengths. Each `AreaFlagRow<T>` carries its flag plus the caller-owned remainder. An absent area clears only its flag. The non-per-area path copies the entire first row, including that remainder. |
| 0xf60048 | `flags::evaluate_selection` | The same inputs plus the preferred level. It returns the selected, reference and baseline levels. The last present area selects the level, but the first row's flag controls subtraction of `record_20` and `record_level` (0xf5f550). Readiness is Ready: the stock wrapper passes 1 or 2, both nonzero. |
| 0xf740e8, including the call at 0xf74258 | `flags::adjusted_flags` | Decode the context through `stock::exposure::adjustment_flags`. Store returned `change` into the adjustment work result, return `level`, and write `flags`. The helper reads its capture profile and calibrated minimum through ports. |

For measured inputs, the supplied position is unchanged. An unmeasured input with a nonzero limit offset also keeps that position. With a zero offset, the position is rebased by `step(3) - levels.start` and the same quantity becomes the limit offset, except that the shared/single-row paths exempt scenes 45 and 55. The class-5 per-area path has no such scene exemption. Stock also updates a temporary start half at flag-block +0x18; neither flag evaluator reads it, so there is no owned state for that dead temporary.

The earlier declared auxiliary-word correction remains: `profile_mode::limit_levels` carries the selection's auxiliary into the limits request instead of reading the stack word 0xf5e72c leaves unwritten.

## All 44 forwarders

`a0` is the incoming first argument, followed by register then stack arguments. “Unchanged” means no argument transform in the forwarder, not that a Rust function receives raw guest pointers. The adapter decodes records, obtains the owned state, and stores only the outputs its retained consumers require. Generic stock targets specialized to their sole owned caller are identified explicitly.

| Forwarder | Target | Owned binding and argument disposition |
| --- | --- | --- |
| 0xf4c3b8 | 0xf49504, 0xf49c04, 0xf49e44 | `library_state::stop(memory)`; the latter two targets have no behavior. |
| 0xf4c3d0 | 0xf88dcc | Target arguments `(a0,a1,a2,a3,0,a4,a5)`. `sample_grid::grid_pair(samples, selection)` uses `Cell([cell[0],cell[1]])` for mode 2 or 5, otherwise `Rings { limit: a2, weights: None }`. Tuple first goes to a4 (second sample half), tuple second to a5 (first sample half). |
| 0xf4c730 | 0xfa364c | Unchanged. `MotionState::prepare(ports, lists, PreparationInput, frame)` replaces the state/output pointers; the input contains record, lens, load-test setting and focus request. `Preparation` exposes output flags, frame delay and image-buffer order. |
| 0xf4c740 | 0xf99ad0 | Unchanged. `MotionState::lay_out_samples(ports)` reads the prepared state; `layout()` exposes the result to the pipeline. |
| 0xf4c750 | 0xf9a6ac | Unchanged. Owned callers use `MotionState::measurement_settings(ports)` and keep `MeasurementSettings`; retained request adapters use `stock::measurement::configure_request(request, state, ports)`. Its seven output pointers are request words 2..8; a null outer request returns 0x84. `set_up_measurement(records, ports)` writes explicitly supplied records. |
| 0xf4c760 | 0xfa0130 | Unchanged. Implementation dependency: ExposurePlan's plan decision. Plan record types alone do not implement the decision. |
| 0xf4c770 | 0xf9c96c | Unchanged. Implementation dependency: motion/plan result incorporation. |
| 0xf4c780 | 0xf9cd00 | Unchanged. Implementation dependency: motion/plan fallback result. |
| 0xf4c790 | 0xf9d3dc | Unchanged. `MotionState::pipeline_flag(memory)` uses the state's dump section. |
| 0xf4c7a0 | 0xf9cdd8 | Unchanged. `MotionState::channel_status()` uses the prepared search state. |
| 0xf4c7b0 | 0xf9cdfc | Unchanged. Decode the calc result with `stock::motion::measured_levels`, then `MotionState::check_measured_levels(ports, measured)`. |
| 0xf4c7c0 | 0xf9cda4 | Unchanged. `MotionState::trace_elapsed(ports)` uses its preparation time and dump section. |
| 0xf4c81c | 0xf8cc9c | Unchanged. `rate_samples::set_up(memory, RecorderSetup)` takes the input's buffer, results and seven table pointers. |
| 0xf4c8f0 | 0xf8d754 | Unchanged. `rate_samples::mark_recording(memory)`; no additional input. |
| 0xf4c900 | 0xf8d778 | Unchanged. `rate_samples::recorded(memory)` returns the count byte. |
| 0xf4c910 | 0xf8d7a0 | Constant zero; no implementation needed. |
| 0xf4c920 | 0xf9ed44 | Unchanged. `MotionState::peak_reached(ports)` uses the current plan and results. |
| 0xf4c930 | 0xfa3534 | Six unchanged arguments: `queries::step_offset(ports, lens, step, position, side, reference, kind)`; position is signed. |
| 0xf4ca3c | 0xfaf8a0 | Target receives `(0x5ffb6120,a0,a1,a0)`; it consumes only the first three. `metering::metered_levels` reads `CellSums` from the fixed buffer and `MeteringInputs` from a0: exposure/gain words +0/+4, level/offset halves +0x64/+0x66, profile/lens/reference words +0x74/+0x78/+0x7c. Store mean/weighted halves through a1. The duplicated r3 is not a second input. |
| 0xf4f0fc | 0xf9d414 | Unchanged. `queries::second_channel_available(ports, record)`. |
| 0xf4f10c | 0xf9d49c | Unchanged. `MotionState::search_mode().code()`. |
| 0xf4f11c | 0xfa2e58 | Unchanged. `MotionState::targets(ports, position, frame)`; `SampleTargets::words()` gives the ordered output halves. |
| 0xf4f12c | 0xfa30c0 | Unchanged. `MotionState::position_at_frame(frame)`. |
| 0xf4f13c | 0xfa30f0 | Unchanged. `MotionState::replan_from_run(ports, earlier, in_force, positions)`; decode the frame/speed schedule and position observations. |
| 0xf4f14c | 0xf8d7b8 | Unchanged. `rate_samples::record_column(memory, samples, column)`. |
| 0xf4f15c | 0xf8d8c8 | Unchanged. `rate_samples::scan_extremes(memory, count)`. |
| 0xf526fc | 0xf7a9ec | Five unchanged arguments: row, column, index, table, output. `profile_adjustment::halfword_offset` locates the two table bytes. `CellTable::mode_cell(rate)` is the profile-mode caller's row-7/index-0 specialization, not a replacement for every row. |
| 0xf5ecb0 | 0xf6fa5c | Unchanged selection/scene/flags inputs become `SceneSelection`; `scene_settings::curve_table` returns the property key. |
| 0xf5f6c0 | 0xf6ec88 | Nine unchanged arguments: scene, flags, measured level, policy, armed, selector, choice, low output, high output. `scene_settings::nibbles(properties, NibbleInputs)` returns the two output nibbles. |
| 0xf6022c | 0xf69788 | Five unchanged arguments: lens reference, area, metered-plus-offset, offset, output row. `exposure::grid::area_distances` takes the lens reference and metered-minus-offset through the context and the decoded `AreaReading`. The sole shot caller passes `(context.metered_level + offset)` and `offset`, so they cancel in halfword arithmetic (0xf1e1c0..0xf1e1e8). If the output row's flag is clear, use `AreaDistances::default()` instead. Store its code at +4 and four halves at +8..+14. |
| 0xf6024c | 0xf69884 | Eight unchanged arguments: record, levels, cell, bank, index, lens reference, refinement, output. `exposure::grid::target_reference` is the shot's bank-0/index-4 specialization; its only consumed output is the near step. Levels/refinement come from the context; cell and lens reference are explicit. |
| 0xf60284 | 0xf6baa8 | Unchanged. `adjust::context_trace::trace(ports, context)`; this is the shot's finish-primary operation. |
| 0xf60294 | 0xf6c8fc | Eight unchanged arguments. `shot_trace::trace_plain_selection(ports, block, cells, PlainTrace)` takes frame, count from the count pointer, levels, record, lens reference and skip-grids flag; the calculation block and selected-cell view remain explicit. |
| 0xf602cc | 0xf6d870 | Unchanged. `shot_trace::trace_selected_cell` with `Pass::Plain` and the incoming first/repeat slot. |
| 0xf602dc | 0xf6d934 | Unchanged. `shot_trace::trace_lens_levels` with block, record, frame, slot and lens reference. |
| 0xf602ec | 0xf6da20 | Five unchanged arguments. `shot_trace::trace_selection(ports, block, cells, frame, count, levels)`; selected-cell view replaces the stock selection address arithmetic. |
| 0xf6030c | 0xf6e484 | Unchanged. `trace_selected_cell` in the First slot with `shot_trace::mode_pass(block)`. |
| 0xf6031c | 0xf6e540 | Five unchanged arguments. `shot_trace::trace_second_selection(ports, block, cells, frame, count, levels)`. |
| 0xf6033c | 0xf6e91c | Unchanged. `shot_trace::trace_area_grid(memory, block, frame, TraceSlot::Repeat)`. |
| 0xf6034c | 0xf6eb34 | Unchanged. `shot_trace::clear_slot(memory, frame, slot)`. |
| 0xf60ab8 | 0xee92e4 | Target arguments `(6,a0+0x35c8,0xe0e4,0)`. External ProfileGeometry `write_dump(DumpKind::Sae, Some(StockData(a0+0x35c8)), 0xe0e4, None)`. |
| 0xf60eb4 | 0xf699e4 | Ten arguments, with a9 sign-extended from i16. `exposure::grid::target_level(properties, LevelQuery)` takes scene=a0, flags=a1, alternate=a2, variant=a3; its `LimitQuery` takes index=a4, policy=a5, armed=a6, set=`LimitSet::of_stage(a7)`, choice=a8, minimum=a9 as i16. |
| 0xf61280 | 0xf6efcc | Six unchanged arguments become `EnableInputs`: selection, scene, flags, auxiliary, hold, selector. `scene_settings::adjustment_enabled` returns the decision. |
| 0xf625cc | 0xf6f734 | Five unchanged arguments become `SceneMaskInputs`: measurement selection, scene, callback armed, treated, subjects. `scene_policy::scene_enabled` already implements this target. The treated input is the owned boolean's 0/1 code. |

The grid rows above supersede the incorrect names in the earlier private table: 0xf69788 is area distances, 0xf69884 is target reference, 0xf699e4 is target level, and 0xf6c8fc is the plain-selection trace. Function addresses and typed semantics, not old table labels, determine the binding.

## Scene classification and measurement dependencies

`SceneRecognition::request_classification` implements the retained 0xf4eae0 entry through `ClassificationControl + CalcSub`. The adapter supplies `Frame` from arguments 0, 1 and 4..9, splits argument 2 into high/low aspect halves, forwards argument 3 as the orientation reading and argument 10 as the callback word. Queue 46 operation 0x3e00 uses block 56, the running task's sender byte, and the complete 48-byte interface encoder with no exclusions. Enable classification through retained 0xfb236c(0) before the post. The core clears its changed mark even on refusal; the retained caller ignores the post result. The sole retained caller supplies completion 0x102df28.

The measurement settings consume a typed plan; implementing their seven records does not implement the missing plan decision or its result incorporation/fallback. The three pending forwarder rows above remain dependencies until that source arrives.

## Qualification boundary

Host tests and the standalone synthetic consumer exercise the new APIs and wire output. They do not execute stock or establish whole-subsystem ownership. Image linkage, ordered subsystem-interface scenarios with demonstrated stock execution and a detected wrong variant, and the single independent review remain prerequisites of the cutover. Unreached scene/style combinations, adjustment paths and retained callers stay in the final-pass queue.
