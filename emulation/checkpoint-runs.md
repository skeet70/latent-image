# Full-system run starting states

This inventory covers executable launchers in `emulation/` and `replacement/`, including the retained upstream command scripts. A target that only checks a receipt, stages firmware or runs bounded native cases does not start a full-system emulator. A run's observation scope determines its starting state, not whether its final assertion happens after boot. In particular, omitting startup from a scheduler or allocation history changes the history under test.

`reset` means instruction-zero initialization followed by actual guest startup. An authenticated zero-instruction snapshot is equivalent for that purpose. Checkpoint-based `post-boot` means the producer runs startup once for the exact identity, then restores its saved state before attaching route observers. A board-clock Census post-boot route instead observes preparation continuously from reset; the route boundary is not a restored starting state.

Census reports use schema5, declare `observation_start_instructions` and `route_origin`, and retain setup failures in `census.json`. Non-boot routes default to `--start post-boot`. Without a board schedule they use the shared checkpoint. With a board schedule they require `--preparation-ticks` and observe the common preparation before the route, from actual zero instruction/tick origin. `prepare-boot` and `cold-boot` require reset and refuse checkpoints; the former ends at post-setup live view, while the latter also opens the shooting menu.

The post-boot store removes abandoned partial publications under its store lock. Publishing syncs the snapshot, record and directories. On lookup it evicts entries at least seven days old when all identity fields except `emulator_binary` match the current request and the executable digest differs. Other firmware/procedure identities are retained. One store lock replaces per-key locks.

## Stimulus clock

`saclac-census` presses/releases inputs, sets the mode dial and ends observation at schedule points. The default instruction clock can give different guest times to images executing different instruction counts. `--stimulus-ticks ROUTE_SCHEDULE` uses exact board ticks. A post-boot board route also requires `--preparation-ticks BOOT_SCHEDULE`; checked `TickSchedule::followed_by` appends the unchanged relative route deadlines to the common boot schedule. The observer attaches before startup and does not replace its recorder or restore an instruction-prepared checkpoint. A firmware-condition wait ends at the next scheduled point at the latest; a late point or a different stimulus refuses.

`census.json.stimuli` retains the combined clock origin and every actual point. `preparation` records the drained boundary, raw-hit ordinal, saclac event ordinal and complete pending context frames; `route_origin` is the actual transition. `key_pulses` includes debounced releases. `saclac-census schedule CENSUS_JSON SCHEDULE_JSON` derives an independent schedule from a stock receipt, retaining the existing720000-tick minimum after releases. Use a stock `prepare-boot` reset run for the boot clock and a stock instruction-clock post-boot route for its relative route clock; candidate measurements do not fit either clock.

Board-clock observations require `--context-interface CONTRACT`. Census reuses the shared timer/queue/in-flight-call Recorder with logical task capture, continuous classified stores and dynamically tracked returns. `context-traffic.jsonl` preserves every recorded raw hit; `pending-context.jsonl` is the declared boundary projection. Neither stream resets at preparation. The500000 raw-hit cap applies separately to boot and route; the aggregate instruction bound and existing traffic/delay limits are unchanged. `context` reports raw counts, coverage and pending calls. These files are working output under run-output; the final verdict retains their checked facts.

The supplied contract determines relevant context coverage. The stock `replacement/time_services/stock-interface.json` covers application tasks6/9, not the complete RTOS timer family or every whole-saclac obligation. Two pending queue receives do not prove empty queues. A whole-image comparison must bind all relevant declarations, compare the continuous histories/pending obligations under the existing independent bounds, preserve raw differences and reduce any remaining divergence to its first causal difference. A passing stock-only producer smoke establishes none of those equality or ownership claims.

## Bazel run targets

All labels without a package prefix are in `//emulation`. The declarations are in [BUILD.bazel](BUILD.bazel). Each implementation link identifies the launch and its observation contract.

| Target | Starting state | Scope and source |
| --- | --- | --- |
| `flash-boot-check` | reset | Exact candidate DAT cold boot and initial-settings route, [FlashBootCheck.hs](FlashBootCheck.hs). |
| `fujimod-flash-boot-check` | reset | Same cold-boot contract on FujiMod, [FujiModFlashBootCheck.hs](FujiModFlashBootCheck.hs). |
| `xm1-viewer`, interactive mode | reset, or explicitly supplied viewer snapshot | Interactive startup is visible, [Xm1Viewer.hs](Xm1Viewer.hs), [start_setup_viewer.py](xm1-upstream/tools/start_setup_viewer.py). |
| `xm1-viewer --ram-push-e2e` | post-boot/post-install | Attach and installed checkpoint selection, [Xm1RamPushE2E.hs](Xm1RamPushE2E.hs). `--fresh-boot` explicitly tests installation/startup instead; AF ownership and allocation keeper observers remain from reset. |
| `xm1-viewer --flash-candidate` | post-boot | Menu, modal, RAM-diff and trajectory observations use the candidate's attach checkpoint, [Xm1RamPushE2E.hs](Xm1RamPushE2E.hs). A flash-boot check is the distinct reset scenario. |
| `xm1-viewer --warm-checkpoint` | post-boot producer | Saves or restores attach state without a trajectory, [Xm1RamPushE2E.hs](Xm1RamPushE2E.hs). |
| `xm1-viewer --ff80-rehearsal` | post-boot | Endpoint rehearsal from attach state, [Ff80Rehearsal.hs](Emulation/Ff80Rehearsal.hs). |
| `cardless-ram-push-bridge` | post-boot | Retained cardless attach checkpoint, [CardlessRamPush.hs](CardlessRamPush.hs). `--fresh-boot` preserves startup. |
| `xm1-bulk-read-e2e` | post-boot | Endpoint reads and alias comparisons, [Xm1BulkReadE2E.hs](Xm1BulkReadE2E.hs). |
| `xm1-half-press-e2e` | post-boot | Shooting/menu input consumers, [Xm1HalfPressE2E.hs](Xm1HalfPressE2E.hs). |
| `xm1-af-ring-e2e` | post-boot | Ordinary half-press and recorder fallback, [Xm1AfRingE2E.hs](Xm1AfRingE2E.hs). |
| `xm1-viewer-inputs-e2e` | post-boot | Controls, modes, S2 and restoration isolation, [Xm1ViewerInputsE2E.hs](Xm1ViewerInputsE2E.hs). Explicit fresh cases remain reset. |
| `usb-toggle-emu` | mixed | Route operations use keyed post-boot state. Cable-at-reset and access-probe cases observe startup, [UsbToggleEmu.hs](UsbToggleEmu.hs). |
| `checkpoint-warmup` | post-boot producer | Delegates to the viewer and toggle runners, [CheckpointWarmup.hs](CheckpointWarmup.hs). |
| `ram-calibration` | post-boot | Captures post-attach RAM on either runtime, [RamCalibration.hs](RamCalibration.hs). |
| `fujimod-ram-push-check` | reset | Tests cold bridge start and empty installed-checkpoint list, [FujiModRamPushCheck.hs](FujiModRamPushCheck.hs). |
| `record-medium-boot-check` | reset | Record reads during fresh startup, [record-medium-boot-check.sh](record-medium-boot-check.sh). |
| `updater-f7` | reset | Recovery/updater installation and committed NAND boot, [UpdaterF7.hs](UpdaterF7.hs). |
| `fujimod-updater-f7` | reset | Recovery RAM, card and updater lifecycle, [fujimod-updater-f7.sh](fujimod-updater-f7.sh), [updater.rs](fujimod-xm1/src/updater.rs). |
| `saclac-census --scenario prepare-boot` / `cold-boot` | reset | Observed preparation to live view, or cold boot through opening the shooting menu, [saclac_census.rs](fujimod-xm1/src/saclac_census.rs). |
| `saclac-census`, instruction-clock non-boot routes | post-boot checkpoint | Saves/restores the exact post-setup boundary and observes the suffix; this is not evidence of matched pending obligations. |
| `saclac-census`, board-clock non-boot routes | continuous reset history | Requires common boot/route tick schedules and a relevant context contract; retains raw/projected history and pending calls across the drained boundary without checkpoint restoration. |
| `scheduler-pair-observe` | reset | Loader arming, code writes, startup call/return and pending service histories are observed before the packaged menu route, [scheduler_pair.rs](fujimod-xm1/src/scheduler_pair.rs). Its existing cohort is not a post-boot-only test. |
| `own-first-pair-observe` | reset | Loader installation, arming and scheduler startup are part of the comparison, [own_first_pair.rs](fujimod-xm1/src/own_first_pair.rs). |
| `rtos-census` | reset | Complete startup syscall and allocation history, with a preexecution checkpoint digest, [rtos_census.rs](fujimod-xm1/src/rtos_census.rs). |
| `startup-callers` | reset | Startup entry/frame witnesses, [startup_callers.rs](fujimod-xm1/src/startup_callers.rs). |
| `task-storage` | reset | Startup task-storage creation and injected refusal controls, [task_storage.rs](fujimod-xm1/src/task_storage.rs). |
| `task-constructors` | reset | Constructor traffic during natural startup, [task_constructors.rs](fujimod-xm1/src/task_constructors.rs). |
| `startup-tasks` | reset | Task composition and first dispatch, [startup_tasks.rs](fujimod-xm1/src/startup_tasks.rs). |
| `kernel-install-observe` | reset | Natural kernel installation and invalid-marker startup control, [kernel_install.rs](fujimod-xm1/src/kernel_install.rs). |
| `startup-dispatch` | reset | Natural/reserved-alias startup dispatch, [startup_dispatch.rs](fujimod-xm1/src/startup_dispatch.rs). |
| `startup-consumers` | reset | Joined installer/dispatcher startup, [startup_consumers.rs](fujimod-xm1/src/startup_consumers.rs). |
| `composed-startup` | reset | From-reset composition and startup mutation census, [composed_startup.rs](fujimod-xm1/src/composed_startup.rs). Its existing cache is initialized-reset, not post-boot. |
| `loader-continuations`, `loader-startup-capture` | reset | Loader continuation and image-handoff witnesses, [loader_continuations.rs](fujimod-xm1/src/loader_continuations.rs). |
| `idle-tasks` | reset | Root/idle creation and initial dispatch, [idle_tasks.rs](fujimod-xm1/src/idle_tasks.rs). |
| `heap-observe` | reset | First heap call and startup allocator history, [heap_observe.rs](fujimod-xm1/src/heap_observe.rs). |
| `heap-replay` | supplied checkpoint | Restores the recorded seed and replays its allocator history, [heap_replay_runtime.rs](fujimod-xm1/src/heap_replay_runtime.rs). No cold boot. |
| `property-pair-observe` | reset | From-reset property allocation/initialization and storage boot observer, [PropertyPairObserverMain.hs](PropertyPairObserverMain.hs), [SettingsStorageObserver.hs](Emulation/SettingsStorageObserver.hs). |
| `record-store-pair-observe` | reset | Startup record reads and later route traffic, [RecordStorePairObserverMain.hs](RecordStorePairObserverMain.hs). |
| `record-store-timing-control` | reset | Controlled startup timing and allocation history, [RecordStoreTimingControlMain.hs](RecordStoreTimingControlMain.hs). |
| `media-route-smoke` | post-boot | Media routes use the keyed card-backed attach boundary, [MediaRouteSmoke.hs](MediaRouteSmoke.hs). |
| `verify-candidate` | mixed | Runs the existing candidate matrix, including updater cold boots and post-boot viewer trajectories, [VerifyCandidate.hs](VerifyCandidate.hs). |
| `critical-pair-witness` | reset | Driver startup witness, [CriticalPairWitness.hs](CriticalPairWitness.hs). |
| `first-stage-witness` | reset | Driver boot witness, [FirstStageWitness.hs](FirstStageWitness.hs). |
| `peer-stage-witness` | reset | Driver peer witness, [PeerStageWitness.hs](PeerStageWitness.hs). |
| `//replacement/modal_runtime:menu-engine-qualify-bounded` | post-boot/post-install | Menu/render cohort, [QualifyBoundedMain.hs](../replacement/modal_runtime/QualifyBoundedMain.hs). Its installation cache is maintained with the shared guest-state identity. |

The raw FujiMod driver [main.rs](fujimod-xm1/src/main.rs) starts from reset for its boot and peer witnesses. The terminal first-stage observer [terminal_observer.rs](fujimod-xm1/src/terminal_observer.rs) also stays from reset. `root-compact`, `fujimod-xm1-record-medium` and `fujimod-xm1-provenance` consume inputs or receipts without executing full-system guest instructions. Bounded native/recovery-bootstrap engines do not cold-boot the full camera system.

## Run scripts

| Script | Starting state | Source |
| --- | --- | --- |
| `emulation/fujimod-updater-f7.sh` | reset | [Updater wrapper](fujimod-updater-f7.sh). |
| `emulation/record-medium-boot-check.sh` | reset | [Record-medium boot wrapper](record-medium-boot-check.sh). |
| `emulation/menu-engine-pair-trajectories/run-render-pairs.sh` | post-boot/post-install | [Viewer cohort arguments](menu-engine-pair-trajectories/run-render-pairs.sh); does not request `--fresh-boot`. |
| `emulation/xm1-upstream/tools/start_setup_viewer.py` | reset or supplied snapshot | [Viewer command](xm1-upstream/tools/start_setup_viewer.py). Cold mode displays startup; snapshot mode restores explicitly. |
| `emulation/xm1-upstream/tools/continue_rec_scanout.py` | post-boot, supplied snapshot, or explicit reset/recovery | [Scanout command](xm1-upstream/tools/continue_rec_scanout.py). The Haskell launchers select keyed bridge checkpoints; reset/recovery options preserve boot coverage. |
| `emulation/xm1-upstream/tools/probe_setup_menus.py` | supplied post-boot snapshot | [Probe command](xm1-upstream/tools/probe_setup_menus.py), requires `--snapshot`. |

Other shell scripts under `replacement/` build, relink or execute host/native tests; the full-system replacement cohort is the Haskell qualifier above. Private `run-saclac-census.sh` wrappers invoke the public census target; their scenario determines whether boot is observed. The measured cohort has five routes per firmware: cold boot, half press, Fn, Fn down and Fn auto-open. Cold boot remains a reset run on both sides.

## Measured census saving

E-XM1-POSTBOOT-CHECKPOINT-REUSE-150 records five routes on each of the stock and unchanged saclac candidate firmware: cold boot, half press, Fn, Fn down and Fn auto-open. Both cold-boot runs remain from reset. The first half-press run per firmware creates one checkpoint at 283,250,000 instructions; each following menu route restores it.

| Route, per side | Previous executed instructions | Current executed instructions | Cache |
| --- | ---: | ---: | --- |
| Cold boot | 383,750,000 | 383,750,000 | Not used |
| Half press | 309,750,000 | 309,750,000 | Created |
| Fn | 435,120,000 | 151,870,000 | Hit |
| Fn down | 474,650,000 | 191,400,000 | Hit |
| Fn auto-open | 550,210,000 | 266,960,000 | Hit |

The ten-run total falls from 4,306,960,000 to 2,607,460,000 guest instructions: 1,699,500,000 saved, or 39.46%, including both checkpoint creations. The saved checkpoint is reused by six runs. All ten match their last recorded counterparts' input digests, final cumulative instruction counts, menu milestones and ordered dispatcher-event suffixes after the declared observation boundary. These are checkpoint-equivalence measurements, not a new stock-versus-owned subsystem qualification.

Summed recorded wall times are 169 seconds before and 66 seconds after. Those timestamps include slot waits and shared-host load and have one-second resolution; the instruction reduction is the attributable measurement.


Four further stock scenarios each restore the same 283,250,000-instruction boundary and equal newly executed from-reset observations after it: shoot-without-lens (687,910,000 executed), capture (679,660,000), playback (107,530,000) and mode-dial (90,000,000). Their cumulative final counts are 971,160,000, 962,910,000, 390,780,000 and 373,250,000 respectively. All four are cache hits, saving 1,133,000,000 instructions. E-XM1-POSTBOOT-CHECKPOINT-REUSE-150 binds the input, suffix and boot-prefix counter comparisons.
