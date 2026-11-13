# Full-system run starting states

This inventory covers executable launchers in `emulation/` and `replacement/`, including the retained upstream command scripts. A target that only checks a receipt, stages firmware or runs bounded native cases does not start a full-system emulator. A run's observation scope determines its starting state, not whether its final assertion happens after boot. In particular, omitting startup from a scheduler or allocation history changes the history under test.

`reset` means instruction-zero initialization followed by actual guest startup. An authenticated zero-instruction snapshot is equivalent for that purpose. `post-boot` means the producer runs startup once for the exact identity, then subsequent runs restore its saved state before attaching scenario observers. Boot and post-boot coverage are recorded separately.

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
| `cardless-ram-push-bridge` | post-boot | Retained cardless attach checkpoint, [CardlessRamPush.hs](CardlessRamPush.hs). Explicit reset observations and `--fresh-boot` preserve startup. |
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
| `media-route-smoke` | post-boot | Media routes use the keyed card-backed attach boundary, [MediaRouteSmoke.hs](MediaRouteSmoke.hs). Committed-NAND reboot scenarios retain their reset path. |
| `verify-candidate` | mixed | Runs the existing candidate matrix, including updater cold boots and post-boot viewer trajectories, [VerifyCandidate.hs](VerifyCandidate.hs). |
| `critical-pair-witness` | reset | Driver startup witness, [CriticalPairWitness.hs](CriticalPairWitness.hs). |
| `//replacement/modal_runtime:menu-engine-qualify-bounded` | post-boot/post-install | Menu/render cohort, [QualifyBoundedMain.hs](../replacement/modal_runtime/QualifyBoundedMain.hs). Its installation cache is maintained with the shared guest-state identity. |

The raw FujiMod driver [main.rs](fujimod-xm1/src/main.rs) starts from reset for its boot and peer witnesses. The terminal first-stage observer [terminal_observer.rs](fujimod-xm1/src/terminal_observer.rs) also stays from reset. They are binaries exposed through the Nix SDK rather than additional local Bazel run labels. `root-compact`, `fujimod-xm1-record-medium` and `fujimod-xm1-provenance` consume inputs or receipts without executing full-system guest instructions. Bounded native/recovery-bootstrap engines do not cold-boot the full camera system.

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
