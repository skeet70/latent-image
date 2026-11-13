# Full-system run starting states

This inventory covers executable launchers in `emulation/` and `replacement/`, including the retained upstream command scripts. A target that only checks a receipt, stages firmware or runs bounded native cases does not start a full-system emulator. A run's observation scope determines its starting state, not whether its final assertion happens after boot. In particular, omitting startup from a scheduler or allocation history changes the history under test.

`reset` means instruction-zero initialization followed by actual guest startup. An authenticated zero-instruction snapshot is equivalent for that purpose. `post-boot` means the producer runs startup once for the exact identity, then subsequent runs restore its saved state before attaching scenario observers. Boot and post-boot coverage are recorded separately.

Post-boot census reports use schema 3 and declare `observation_start_instructions`; the comparator refuses unequal observation boundaries. `--start reset` explicitly observes startup for a comparison baseline; non-boot scenarios default to `--start post-boot`. A cold-boot census refuses `--checkpoints` and post-boot starts. Setup failures retain their milestones and failure in `census.json`.

The post-boot store removes abandoned partial publications under its store lock. Publishing syncs the snapshot, record and directories. On lookup it evicts entries at least seven days old when all identity fields except `emulator_binary` match the current request and the executable digest differs. Other firmware/procedure identities are retained. One store lock replaces per-key locks.

## Stimulus clock

`saclac-census` presses and releases its inputs, sets the mode dial and ends its observation at schedule points. By default each point is due after an instruction budget, so an image that executes a different number of instructions meets the same press at a different guest time. `--stimulus-ticks SCHEDULE` fires every point at its board tick counted from the observation origin: reset, or the restored post-boot checkpoint. Board ticks are the guest time the device models count, the system timer included. On that clock a wait for a firmware condition (a debounced release, a menu state) ends at the next point's tick at the latest, and the run fails when a point is not reached exactly at its tick or the schedule names another stimulus there.

`census.json` records `stimuli`: the clock, the origin's board tick and instruction count, and each point's stimulus, tick ordinal and instruction count. `key_pulses` records every key press with its debounced release. `saclac-census schedule CENSUS_JSON SCHEDULE_JSON` turns a receipt's `stimuli` into a schedule, delaying each point that follows a release, and every later point with it, to at least `SCANNED_HOLD_TICKS` (720,000 ticks, two scan intervals plus 10 ms) after the release. The post-boot checkpoint identity does not include the clock; both clocks restore the same checkpoint. Other drivers take the same timing from `fujimod_xm1::stimulus`.

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
| `saclac-census --scenario cold-boot` | reset | Dispatcher and action traffic during cold start and initial setup, [saclac_census.rs](fujimod-xm1/src/saclac_census.rs). |
| `saclac-census`, all other scenarios | post-boot | Saves the post-initial-setup live-view boundary; observes only the following route. Reports the checkpoint digest and observation-start instruction count, [saclac_census.rs](fujimod-xm1/src/saclac_census.rs). |
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
