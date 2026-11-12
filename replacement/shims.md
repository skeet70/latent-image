# Shim register

[Replacement index](README.md). [Specification § Ownership stages](../spec/11-incremental-firmware-replacement.md#ownership-stages) requires every compatibility layer with retained code, including ABI bridges, delegations and adapters, to be a named shim with a stated removal condition, and requires the shim and obsolete production paths to be removed when Rust owns both sides.

This file is the register. One table covers each owned unit under `replacement/` that contains Rust or assembly sources and builds a firmware artifact. A row is one shim family: a place where owned code takes or presents a stock shape only because of code on the other side. Symbols are grouped where they share a mechanism, the mechanism file is named, and counts are of the current tree.

Columns:

- **Shim**: the family name and the file that defines the mechanism.
- **Kind**: `stock-ABI export`, `veneer or carrier at a stock address`, `trampoline into retained stock`, `callback registry for guest words`, `kernel interface slot`, `guest-memory layout view`, `retained stock seam in assembly`, or `C-ABI bridge between two owned units`.
- **Other side**: the stock code or owned unit that still needs the stock shape.
- **Owned now**: `no`, `partly` or `yes`.
- **Removal condition**: one sentence; the condition that makes the shim unnecessary.
- **State**: `required`, `condition met` or `removed`.

Rows whose other side is an owned unit appear first in their unit table and again in [Owned on both sides](#owned-on-both-sides). A cutover is not complete while a table shows `condition met` on a shim that is still present. A shim is removed only after both sides of it have been compared with stock and linked into the same image. `//tests:research-contracts` checks that every firmware unit has a table and that each row uses a known kind and state.

## Owned on both sides

These shims exist only because two owned units meet through a stock-shaped or C-ABI interface. Removing one requires both sides compared with stock and linked into one image; a native comparison of one side alone does not discharge it. The mechanism is the named export in the first unit and the `extern "C"` declaration or carrier entry in the second.

| Shim | Units | Mechanism | What must be true before removal | State |
| --- | --- | --- | --- | --- |
| Power leaf C-ABI services | power_lifecycle leaf → display_scanout, lens_transport, application_nand, vendor_protocol | `#[no_mangle] extern "C"` exports in `power_lifecycle/src/osal.rs`, `resource_route.rs`, `record_native_exports.rs` and the audio/serial modules; 40+ symbols including `xm1_power_interrupt_lock`, `xm1_power_current_task`, `xm1_power_serial_*`, `xm1_power_timer_*`, `xm1_power_semaphore_*` | Both crates link as Rust items (no C ABI), and both sides have stock comparisons in one image | required |
| NAND application exports | application_nand → power_lifecycle, vendor_protocol | 14 `xm1_app_nand_*` exports in `application_nand/src/lib.rs` | Both callers use a typed NAND API in one image after stock comparison | required |
| Scanout exports | display_scanout → power_lifecycle, vendor_protocol | `xm1_scanout_*` and `xm1_power_lcd_operation_action` exports in `display_scanout/src/abi.rs` and the panel modules | Both callers link the scanout crate directly in one image after stock comparison | required |
| Settings storage exports | settings_storage → power_lifecycle | `xm1_getter_owned`, `xm1_setter_owned`, `xm1_storage_initialize` and the custom entries in `settings_storage/src/lib.rs` | Power links the settings crate directly and both sides compare with stock in one image | required |
| Property service exports | property_service → develop, power_lifecycle | `xm1_property_get` and `xm1_property_set` in `property_service/src/lib.rs` | Both callers use the typed property service in one image after stock comparison | required |
| Record store exports | record_store → settings_storage, power_lifecycle | `record_store_*` exports in `record_store/src/lib.rs` over the guest adapter | Both clients link the owned store and both sides compare with stock in one image | required |
| Calibration exports | calibration_liveview → power_lifecycle | `xm1_calibration_u16` and `xm1_calibration_i16` in `calibration_liveview/src/abi.rs` | Power links the calibration crate directly in one image after stock comparison | required |
| Shared callback registry | power_lifecycle, develop → callback_core | `CallbackRegistry`, `KernelSlot` and the admitted extents in `callback_core/src/lib.rs`, re-exported with each unit's critical section | Both units link the shared crate and neither keeps a private extents table or kernel-slot reader; both sides have stock comparisons in one image | required |
| Pipeline image exports | develop → power_lifecycle | `xm1_pipeline_image_current_task`, `xm1_pipeline_image_send_message`, `xm1_pipeline_image_receive_message` in `develop/src` | Power links the develop pipeline directly in one image after stock comparison | required |
| Menu value transaction exports | menu_value → modal_runtime | `rb_menu_value_cancel` and `rb_menu_value_confirm` in `menu_value/src/lib.rs` | Modal dispatch calls the menu-value crate directly in one image after stock comparison | required |
| Modal dispatch export | modal_runtime → power_lifecycle | `rb_modal_dispatch` in `modal_runtime/src/lib.rs` | Power links modal dispatch directly in one image after stock comparison | required |
| Task submission exports | task_submission → power_lifecycle | `xm1_task_submit` and `xm1_task_release` in `task_submission/src.rs` | Power links the submission crate directly in one image after stock comparison | required |
| Save facade export | save → develop | `xm1_save_facade` in `save/src/lib.rs` | Develop calls the typed save facade directly in one image after stock comparison | required |
| Capture boundary exports | capture → develop | `rb_capture_*` exports in `capture/src/lib.rs` and `rb_capture_stock.S` | Develop calls the capture crate directly in one image after stock comparison | required |
| RTOS family scheduler and timer calls | rtos_scheduler, rtos_timer → rtos_objects, rtos_semaphore, rtos_data_queue, rtos_fixed_pool, rtos_cyclic, rtos_task_records | 37 `kind: owned` externals per domain in `rtos_platform/carriers.json` (scheduler insert/remove/notify/switch, timer insert/remove) | Each family links the scheduler and timer crates directly and the aggregate carries stock comparisons for both sides | required |

## abi

The bounded ARM ABI pilot. Its exports exist for the qualification harness, not for production firmware.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_pilot_*` probe exports and `xm1_abi_probe` in `pilot.rs`/`pilot-entry.S` | stock-ABI export | the ABI qualification harness and `pilot.ld`, not stock firmware | no | the pilot is superseded and no harness entry remains | required |
| `xm1_panic_loop` retained loop in `pilot-entry.S` | retained stock seam in assembly | stock panic behaviour used by the pilot | no | the pilot is retired | required |

## af_result

The autofocus result pool, published at the five stock entry addresses in `af-result.ld`.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_af_select_adapter`, `xm1_af_rotate`, `xm1_af_reserve`, `xm1_af_release` and the secondary consumer in `src/abi.rs`, placed by `af-result.ld` at `0x103f190`, `0x103f1fc`, `0x103fb50`, `0x103fd38`, `0x103fd88` | stock-ABI export | stock AF callers and pointer tables that enter the five addresses | no | every AF result caller is owned or migrated | required |
| `xm1_af_select` entry veneer in `entry.S` | veneer or carrier at a stock address | the stock selector entry | no | the selector is called directly | required |
| `xm1_af_critical_enter`, `xm1_af_critical_leave`, `xm1_af_secondary_mode` retained services | trampoline into retained stock | stock RTOS critical-section and secondary-core services | no | the RTOS platform and secondary-core state are owned | required |

## application_nand

The NAND controller and profile-map implementation. Its exports are consumed by owned units; the raw device shape is the remaining seam.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_app_nand_*` exports (14 functions in `src/lib.rs`) | C-ABI bridge between two owned units | power_lifecycle and vendor_protocol | yes | both callers link the NAND crate directly and both sides compare with stock in one image | required |
| NAND register and profile-map layout (`NAND = 0xfff50000`, profile base `0x380`, `src/physical.rs`, `src/memory.rs`) | guest-memory layout view | the stock raw-NAND clients and the boot-time map that the controller must reproduce | partly | every stock raw-NAND client uses the owned flash layer and the map is initialized by owned startup | required |
| `xm1_power_resource_route_request` and the other `xm1_power_*` externs in `src/lib.rs` | C-ABI bridge between two owned units | power_lifecycle leaf | yes | the leaf is linked directly and both sides compare with stock in one image | required |

## application_pump

The application-phase pump hooks, staged at the stock prehook, normalizer and posthook addresses.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_application_prehook`, `xm1_application_normalizer`, `xm1_application_posthook` in `entry.S` | veneer or carrier at a stock address | the stock application loop at `0xbfb1b8`, `0xbfb20c`, `0xbfb1e0` | no | the owned task1 actor calls the pump directly | required |
| the same three stock hooks as `extern "C"` retained services in `src.rs` | trampoline into retained stock | the stock application loop | no | the application loop is owned | required |

## bank

The RAM-push package crate root and its publication at stock menu hooks. It composes owned modules and declares the stock-call trampolines the package still needs.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| 54 `RB_TRAMPOLINE` declarations in `menu/recipe_bank_arm.S` (stage open, transition, redraw, postlude, key left/right, modal query, name drawing, lifecycle helpers) | trampoline into retained stock | the stock menu/application services the package calls | no | the engine, renderer and controller units own every called service | required |
| 130 `rb_*` entries in `menu/recipe_bank_allowlist.json` published at stock hook addresses | stock-ABI export | stock callers that enter the package through the allowlisted hook addresses | no | every stock caller enters an owned hook or the owned engine directly | required |
| `bank`/`bank-modal`/`bank-lifecycle`/`bank-modal-lifecycle` scoped publication in `BUILD.bazel` | stock-ABI export | the stock bank loader and RAM-push installer | no | the owned installer publishes the package and no stock loader remains | required |

## calibration_core

A crate root that re-exports `develop/src/calibration_core` for power_lifecycle and vendor_protocol. It presents no stock shape of its own; the compatibility rows live in `develop`.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |

## calibration_liveview

The calibration cache consumer and live-view publication. Its exports sit at the stock cache/publication entries; its retained trampolines enter stock calibration services.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| 19 `xm1_calibration_*` and `xm1_liveview_*` exports in `src/abi.rs` and `src/publication.rs`, placed by `calibration-liveview.ld` | stock-ABI export | stock calibration and live-view callers | no | every calibration and live-view caller is owned or migrated | required |
| `retained_primary`, `retained_backup`, `retained_fixed`, `retained_copy`, `retained_wb_convert`, `retained_wb_publish`, `retained_halfword`, `retained_lock`, `retained_rebuild` in `entry.S` | trampoline into retained stock | stock calibration storage, white-balance and rebuild services | no | the settings/calibration services and white-balance path are owned | required |
| fixed calibration cache at `0x41acd800` and the 504-byte defaults resource | guest-memory layout view | retained calibration consumers that read the same cache | no | every cache consumer is owned | required |
| `xm1_calibration_u16`/`xm1_calibration_i16` exports | C-ABI bridge between two owned units | power_lifecycle | yes | power links the crate directly and both sides compare with stock in one image | required |

## callback_core

The shared admission table, shape-typed registry and kernel-slot reader. The Power services leaf and the develop unit both compile it; the leaf re-exports it with the Power interrupt lock, develop with its own, and neither keeps a private extents table or kernel-slot reader.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `CallbackRegistry` in `src/lib.rs` and the nine image text extents in `IMAGES`/`EXCLUDED`/`ADMITTED` | callback registry for guest words | stock image 4, 5 and 6 callback words and the owned registrants in the Power leaf, develop, display_scanout, application_nand and vendor_protocol | no | every stock callback word is replaced by an owned entry with a typed Rust signature | required |
| `KernelSlot::read` and `kernel_slot` in `src/lib.rs` | kernel interface slot | the kernel interface block at 0x96bd20 read by the Power leaf and develop | no | the kernel interface is owned and entered through typed Rust functions | required |

## capture

The develop-step completion, cancellation and scheduler-release body. It exports its boundary to develop and trampolines into retained capture services.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `rb_capture_*` exports in `src/lib.rs` and `rb_capture_stock.S` | C-ABI bridge between two owned units | develop | yes | develop calls the capture crate directly and both sides compare with stock in one image | required |
| `rb_capture_stock_advance`, `rb_capture_stock_check`, `rb_capture_stock_log`, `rb_capture_stock_release` and the wait/step-plan trampolines in `rb_capture_stock.S` | trampoline into retained stock | stock capture/develop services at the image-5 boundary | no | the capture service and its scheduler events are owned | required |
| `capture.ld`, `high.ld`, `low.ld` placements and the `deps/` low/high fault variants | veneer or carrier at a stock address | stock callers of the capture step and the qualification controls | no | the capture entry is called directly | required |

## completion_delay

A loader-A completion-delay carrier entered by a patched `bl` and returning to the original site.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_completion_delay_entry`, `_end`, `_release` in `entry.S`, placed by `completion-delay.ld` | veneer or carrier at a stock address | the stock loader-A instruction at `0x40a00` that branches into the carrier | no | loader-A is owned and no patched branch remains | required |

## critical_pair

Task103 critical-section boundaries over retained loader services.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_critical_enter_adapter`, `xm1_critical_exit_adapter` and `xm1_critical_enter_continue`/`xm1_critical_exit` in `entry.S`/`src.rs` | stock-ABI export | stock task103 callers | no | the task103 critical pair is called from owned code only | required |
| `xm1_critical_retained_call` in `entry.S` | trampoline into retained stock | the retained loader critical-section service | no | the loader service is owned | required |

## develop

The development pipeline, including `font_core` and `calibration_core`. It is the largest unit: owned pipeline bodies call retained stock callees and the retained font/calibration data views, and 43 `_stock.S` files hold stock-shaped seams.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_pipeline_image_*` exports | C-ABI bridge between two owned units | power_lifecycle | yes | power links the pipeline directly and both sides compare with stock in one image | required |
| `xm1_property_get`/`xm1_property_set` externs | C-ABI bridge between two owned units | property_service | yes | develop links the property service directly and both sides compare with stock in one image | required |
| `xm1_save_facade` extern | C-ABI bridge between two owned units | save | yes | develop links the save facade directly and both sides compare with stock in one image | required |
| `rb_capture_*` externs in `src/pipeline` and `develop/src` | C-ABI bridge between two owned units | capture | yes | develop links the capture crate directly and both sides compare with stock in one image | required |
| Shared callback registry and kernel-slot reader in `callback_core`, re-exported by `src/pipeline/callbacks.rs` | callback registry for guest words | stock callback words and the kernel interface block | no | every caller uses typed callbacks and the kernel interface is owned | required |
| 1,610 retained `extern "C"` declarations in `src/` (image conversion, blend, finish, chroma, sensor calibration, configuration, diagnostics, tracing) | trampoline into retained stock | stock develop services and device adapters | no | the develop service, sensor/calibration and device paths are owned | required |
| 43 `*_stock.S` files (`Buffers_stock.S`, `FontOutlineVm_stock.S`, `RedEye_stock.S`, `WarpEngine_stock.S`, and the rest) | retained stock seam in assembly | stock develop bodies kept for comparison and for callers not yet owned | no | the corresponding owned body is qualified and every caller is migrated | required |
| `font_core` asset and glyph layout (`src/font_core`, `FontFdl_stock.S`, `FontBuiltin_stock.S`, `FontScripts_stock.S`) | guest-memory layout view | retained stock font resources and text consumers | no | owned assets and every text consumer are owned | required |
| `calibration_core` configuration/curve layout (`src/calibration_core`) | guest-memory layout view | retained calibration data and power/vendor consumers | partly | the calibration image and every consumer are owned | required |
| `develop.ld`, `services.ld` and `caller_observer.ld` placements | veneer or carrier at a stock address | stock callers of develop stages | no | owned code is reached directly | required |

## display_scanout

The display scanout CPU driver, including the panel component. Its exports are at stock entry addresses and its S7-S9 seams are defined in [the target design](../reports/display-render-target-design.md#25-shims) and [the scanout contract](../reports/display-render-contracts/scanout.md).

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_scanout_*` and `xm1_power_lcd_operation_action` exports (31 families in `src/abi.rs`, panel and formatter modules) | C-ABI bridge between two owned units | power_lifecycle and vendor_protocol | yes | both callers link the scanout crate directly and both sides compare with stock in one image | required |
| S7 stock publication below the owned renderer (`display_set_item_configuration`, `display_queue_secondary_callbacks`) | trampoline into retained stock | stock VGLIB front/back buffer publication | no | the owned scanout driver publishes frames | required |
| S8 stock per-frame scheduler (`FUN_006c7318` registration) | callback registry for guest words | retained VGLIB one-shots and live-view clients | no | no retained client registers, or the owned driver serves the same request contract | required |
| S9 stock panel bring-up (timing, formatter and reset sequences) | retained stock seam in assembly | retained callers and ungrounded L1-L5 device laws | no | timing, formatter and reset laws are grounded, caller migration is complete and owned bring-up passes | required |
| `display-scanout.ld` and `display-scanout-panel.ld` placements, plus the `scanout_*` critical/IRQ/yield externs | veneer or carrier at a stock address | stock display callers and the retained IRQ/queue services | no | the display entry is called directly and the IRQ/queue services are owned | required |

## embassy_trial

A trial Embassy executor image. It presents no stock shape; the trial exists to qualify a runtime base.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |

## first_stage

The owned first-record stage, entered at the stock record slot and presenting stock's record layout, media controller, handoff words and loader entry.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_first_stage_entry`, `xm1_first_stage_contract` and `first-stage.ld` placing the image at `0x6000` in the first record's slot | veneer or carrier at a stock address | the stock boot ROM's mode-4 handoff and the record-1 consumers that enter the slot | no | the boot ROM and every record-1 consumer are owned | required |
| the 20-byte compressed header, the 36-byte segment entries and 12-byte sub-list header at `0x61c000`, the publication cell and the 32-byte trailer with `0xaaaa` at offset 30 | guest-memory layout view | the stock update container's record 1 and image 4 | no | owned update records replace the stock container layout | required |
| the NAND controller at `0xfff52000` with 128 KiB units and 512-byte sectors | guest-memory layout view | the stock controller and the boot ROM's mode-4 staging | no | the owned flash layer and its device model replace the stock controller | required |
| the memory DMA block at `0xfff02000` the stage fills the application bss through, with the eight-word descriptor at `0x400042c0` and completion on `0xfff02024` bit 6 | guest-memory layout view | the stock record's own `0x3844` fill and the block's device models | no | the application's bss is owned memory that no device has to clear | required |
| `xm1_first_stage_handoff`, `xm1_first_stage_handoff_vector` and `xm1_first_stage_completion_zero`: the handoff reads stock's `0x4300`/`0x4304` words and jumps, the completion export zeroes stock's completion word, and `xm1_first_stage_stage_images` writes the two handoff words | guest-memory layout view | stock loader A's entry word and the record-2 companion wait | no | loader A and the record-2 companion startup are owned | required |
| the loader-A entry at `0x40020` and the image-4 argument the stage reads at the fixed `0x61c40c`, image 4's own header word and not the record table, and jumps to | trampoline into retained stock | stock loader A and the application image the stage hands off to | no | the loaders and application are owned and entered by owned code | required |

## idle_task

The shared idle task body.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_idle_task` export in `src.rs`, placed by `idle-task.ld` | stock-ABI export | stock task records for tasks 102/103 that enter the stock idle address | no | the task records and scheduler are owned | required |

## image5_init

The image-5 synchronous initializer, calling 19 retained stock services.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| the image-5 initializer export and `entry.S` placement in `image5-init.ld` | veneer or carrier at a stock address | the stock image-5 initializer caller | no | the image-5 caller is owned | required |
| 19 `service_*` retained externs in `src.rs` and `entry.S` | trampoline into retained stock | stock image-5 constructors, task-60 adapter and settings/calibration initializers | no | those services are owned | required |

## image5_load

The owned image-5 loader.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_image5_load` and `xm1_image5_load_entry` in `entry.S`/`src.rs` | veneer or carrier at a stock address | the stock loader caller at `0x4c22b8` | no | the loader caller is owned | required |
| `xm1_image5_*` retained externs (cache, CPU fill, decoder init/finish, feed, fill request/done, marker, mode, read, segment) | trampoline into retained stock | stock image-5 decoder and cache services | no | the decoder and cache services are owned | required |

## interface_initializer

The core-1 interface publication after the loader-B completion wait.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_interface_initializer_entry` and `xm1_interface_initializer_publish` in `entry.S`/`src.rs` | veneer or carrier at a stock address | the stock loader-B completion caller | no | the loader-B caller is owned | required |

## kernel_install

The kernel installer and its cache maintenance.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_kernel_install` and `xm1_kernel_install_entry` in `entry.S`/`src.rs` | veneer or carrier at a stock address | the stock phase entry and installer callers | no | the phase entry and installer callers are owned | required |
| `xm1_kernel_cache` retained service | trampoline into retained stock | the stock cache-maintenance helper | no | cache maintenance is owned | required |

## lens_transport

The lens transport and control actor. Its stock-ABI exports and retained task/queue/IRQ services are the current seam; power leaf calls are already owned on both sides.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_power_current_task`, `xm1_power_interrupt_lock`, `xm1_power_interrupt_unlock`, `xm1_power_service_post`, `xm1_power_set_task_flags`, `xm1_power_resource_route_request` externs | C-ABI bridge between two owned units | power_lifecycle leaf | yes | the leaf is linked directly and both sides compare with stock in one image | required |
| 14 `xm1_lens_*` exports in `src/abi.rs` and `src/control_native.rs`, placed by `control-roots.ld` and `codec.ld` | stock-ABI export | stock lens task and control callers | no | every lens caller is owned or migrated | required |
| `xm1_lens_task99_entry` in `native/control-entry.S` | veneer or carrier at a stock address | the stock task99 entry | no | the task99 entry is owned | required |
| retained task/queue/IRQ externs (`task_create`, `task_delete`, `activate`, `queue_create`, `queue_delete`, `send`, `send_irq`, `receive`, `delay`, `cyclic_*`, `xm1_rtos_irq_*`) | trampoline into retained stock | stock RTOS task, queue and IRQ services | no | the RTOS platform owns those services | required |
| `Callbacks` C-ABI struct and callback registration in `src/abi.rs` | callback registry for guest words | stock callers that pass function pointers and context words | no | callers use the typed completion sink | required |
| `lens_actor_request`, `lens_actor_wake`, `lens_actor_timer`, `lens_actor_device_callback` and the fixed stock addresses in `src/actor_abi.rs`, built only with feature `native-actor` and placed by `native/actor.ld` | stock-ABI export | the native comparison harness, not stock firmware | no | the actor is compared with stock through its owned interface in a linked image | required |

## loader_orchestration

The loader orchestration step.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_loader_orchestration` and `xm1_loader_orchestration_entry` in `entry.S`/`src.rs`, placed by `loader-a.ld`/`loader-b.ld` | veneer or carrier at a stock address | the stock loader continuation | no | the loader continuation is owned | required |
| 21 `xm1_loader_*` retained externs (continue, prepare, activate timers, auto-ready, clear, identity, log, peer, private initialize, publish callback, schedule) | trampoline into retained stock | stock loader and kernel startup services | no | those services are owned | required |

## loader_prefix

The owned loader-B core-1 entry.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_loader_prefix_entry`, `xm1_loader_prefix_exit`, `xm1_loader_prefix_install`, `xm1_loader_prefix_clear` and the fault variants in `entry.S`/`src.rs` | veneer or carrier at a stock address | the stock loader-B entry and the record-2 handoff | no | loader-B startup is owned | required |
| `xm1_loader_prefix_clear` retained service | trampoline into retained stock | the stock workspace-clear helper | no | the clear is owned | required |

## media_permit

The media-write permit resolver and its effects over retained frame services.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| 11 exports in `src/lib.rs` (permit resolution, cancel, effects) | stock-ABI export | stock callers of `FUN_00e77168` and the permit entry | no | every permit caller is owned or migrated | required |
| `rb_mp_stock_*` trampolines in `rb_media_permit_stock.S` (bit set, cancel, command, context, count/kind, fatal, frame kind/of-kind, map subframe, mirror read, subframe, subframe count) | trampoline into retained stock | stock frame, event and slot services | no | the frame services are owned | required |
| `media_permit.ld` and the `cancel`/`command`/`count`/`fatal2`/`low`/`side_a`/`side_b` placements | veneer or carrier at a stock address | stock permit callers | no | owned code is called directly | required |
| `rb_mp_forbidden_body_probe` in `rb_media_permit_probe.S` | retained stock seam in assembly | the retired resolver body used as a negative control | no | the probe is removed with the qualification controls | required |

## menu_engine

The address-independent menu engine and its stock-state mirror. Its shims are listed in [the engine design](../reports/owned-menu-engine-design.md#shims-for-this-unit).

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `rb_menu_engine_entry` and the fallback push in `adapter/entry.S`, published at `0xbf95c8` | veneer or carrier at a stock address | the stock stage consumer and `0xbfa234` fallback | no | the owned key layer and stage consumers call the engine directly | required |
| `rb_menu_engine_call_*` retained services (availability, initial row, kind switch, notify, property write, seek) | trampoline into retained stock | stock menu services | no | the tree and typed settings service answer them | required |
| `rb_menu_engine_guard_failure` and `__aeabi_*` in `adapter/entry.S` | retained stock seam in assembly | stock ABI helper routines and the guard path | no | the engine no longer needs the stock ABI helpers | required |
| block mirror at `0x430` bytes and `0x01467580/84` (`src/state.rs`) | guest-memory layout view | stock readers and writers of the menu block | no | no stock code reads or writes the block | required |
| `rb_ui_probe_visit` page-entry replay | callback registry for guest words | the installed bank's `rb_bank_page_entry` hook | no | the bank's page-entry work is an owned engine consumer | required |

## menu_value

The menu value transaction, published at the dispatcher's IMAGE SIZE jump-table word.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `rb_menu_value_size_arm` in `adapter/entry.S`, placed by `adapter/image.ld` | veneer or carrier at a stock address | the stock dispatcher's jump-table word for property `0xf6` | no | the owned dispatcher calls the transaction directly | required |
| `rb_menu_value_begin`, `rb_menu_value_size_apply`, `rb_menu_value_retained`, `rb_menu_value_cancel`, `rb_menu_value_confirm` exports in `src/lib.rs` | stock-ABI export | stock stage consumers and controller arms | no | every stage consumer and controller arm is owned | required |
| `rb_menu_value_call_*` retained services (aspect/size cells, change marker, current property, display update, property read/write, restore aux, snapshot cells/state) | trampoline into retained stock | stock property, cell and display services | no | the typed settings service and owned display own those services | required |
| `rb_menu_value_cancel`/`rb_menu_value_confirm` exports | C-ABI bridge between two owned units | modal_runtime | yes | modal dispatch calls the crate directly and both sides compare with stock in one image | required |

## menu_widgets

The owned tree, pane and picker widgets and their native adapters.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `owned_pane_drawer`, `tree_list_entry` and the picker/pane/tree native entries in `native/*.rs`, placed by `picker.ld`, `pane_drawer.ld`, `tree.ld` | stock-ABI export | stock draw callers that enter the widget addresses | no | the owned renderer calls the widgets directly | required |
| `rb_picker_call_*`, `rb_tree_call_*` and `rb_pane_call_*` retained services (draw leaves, hooks, menu-table accessors, setting reads, snapshot readers) | trampoline into retained stock | stock draw layer and menu table | no | the owned renderer, tree and settings service own those services | required |
| side state `0x15e64d8` and the palette/report block kept for stock readers | guest-memory layout view | stock readers `FUN_011662a0`, `FUN_01165aa0` and the bank root drawer | no | those readers are owned | required |
| installed label-id bound `0x849` in the native adapters | guest-memory layout view | the installed package's label table | no | the package's labels are declared in the tree | required |

## modal_runtime

Modal dispatch, controller 75 and the cancellation hooks.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `rb_menu_value_cancel`/`rb_menu_value_confirm` externs | C-ABI bridge between two owned units | menu_value | yes | modal dispatch calls the crate directly and both sides compare with stock in one image | required |
| `rb_modal_entry` in `src/entry.S`, published at `0xbfb028`, with the stock+4 fallback for every controller other than 75 | veneer or carrier at a stock address | the stock dispatcher and 173 stock controllers | no | the owned dispatcher owns all 174 controllers | required |
| `rb_modal_dispatch` export | C-ABI bridge between two owned units | power_lifecycle | yes | power links the dispatch crate directly and both sides compare with stock in one image | required |
| `rb_modal_call_prehook`, `rb_modal_call` and `dispatch.S` callback glue | callback registry for guest words | the retained prehook `0xd0cc68` and stock action/transition callbacks | no | every callback is owned | required |
| `rb_call_modal_query`, `rb_call_redraw`, `rb_call_stage_open`, `rb_call_transition`, `rb_bank_key_left/right`, `rb_lifecycle_*` retained services | trampoline into retained stock | stock modal and bank services | no | those services are owned | required |

## owned_menu

The owned controller-75 editor and menu-frame snapshot service, with its stock address exports and retained lifecycle services.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `rb_ui_set`, `rb_ui_leave`, `rb_ui_editor_cancel`, `rb_ui_editor_confirm` and the nine `rb_menu_frame_*` accessors | stock-ABI export | stock controller arms and frame snapshot callers | no | every controller arm and snapshot caller is owned | required |
| `rb_lifecycle_*`, `rb_call_*`, `rb_open_edit*` retained services in `src/lib.rs` | trampoline into retained stock | stock lifecycle, modal query, redraw, stage and transition services | no | those services are owned | required |
| editor buffers, frame snapshots, mode/gate cells and slot tables | guest-memory layout view | stock readers of the editor and frame state | no | no stock code reads the editor or frame state | required |

## peer_wait_control

The loader peer-wait control carrier.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_peer_wait_control_entry`, `_poll`, `_acknowledged`, `_failure`, `_end` in `entry.S` | veneer or carrier at a stock address | the stock loader peer-wait site | no | the loader peer wait is owned | required |

## plain_load

The owned plain-image loader.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_plain_load` and `xm1_plain_load_entry` in `entry.S`/`src.rs` | veneer or carrier at a stock address | the stock plain-loader caller | no | the caller is owned | required |
| `xm1_plain_delay`, `xm1_plain_extents`, `xm1_plain_give`, `xm1_plain_mode`, `xm1_plain_progress`, `xm1_plain_read`, `xm1_plain_segment`, `xm1_plain_take` retained services | trampoline into retained stock | stock plain-loader and scheduler services | no | those services are owned | required |

## power_lifecycle

The Power Services leaf crate and the power application crate. The leaf exports 360 record helpers at the stock ABI through `stock_export!`; the application crate calls other owned units through C-ABI exports and keeps stock record-helper residue.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_app_nand_*`, `record_store_*`, `xm1_calibration_*`, `xm1_property_*`, `xm1_getter_owned`, `xm1_setter_owned`, `xm1_storage_initialize`, `xm1_pipeline_image_*`, `xm1_task_submit/release`, `rb_modal_dispatch`, `xm1_scanout_*` externs | C-ABI bridge between two owned units | application_nand, record_store, calibration_liveview, property_service, settings_storage, develop, task_submission, modal_runtime, display_scanout | yes | every caller links the owning crate directly and both sides compare with stock in one image | required |
| 360 stock-ABI record-helper exports through `stock_export!` in 45 files under `src/` (`record_native_exports.rs` defines the macro) | stock-ABI export | stock record consumers that call the helper addresses | no | every record-helper caller is owned and the helpers are linked as Rust items | required |
| 14 `.S` residue files in `src/` (`native.S`, `record_task43_*`, `capture_profile_*`, `mode_*`, `serial_write_residue.S`, and the rest) | retained stock seam in assembly | stock bodies and register/stack frames the owned code must reproduce | no | the residue is no longer needed for stock callers or ABI parity | required |
| ~1,200 retained `extern "C"` declarations in `src/` (record bodies, audio, capture profile, serial, timers) | trampoline into retained stock | stock record, audio, capture and device services | no | those services are owned | required |
| `record-helpers.ld` placement of the helper exports | veneer or carrier at a stock address | stock callers of the record helpers | no | owned callers link directly | required |
| `KernelSlot`-based root service calls in `callback_core` (re-exported by `src/callback_registry.rs`) and the audio modules | kernel interface slot | the stock kernel roots at `0x4004dae0`/`0x400ddaa0` | no | the RTOS platform owns the root services | required |
| callback registries for guest words in `callback_core` (re-exported by `src/callback_registry.rs`) and the audio modules | callback registry for guest words | stock callers that pass function pointers and context words | no | callers use typed callbacks | required |

## primary_initializer

The loader-A interface publisher.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_primary_initializer_entry` and `xm1_primary_initializer_publish` in `entry.S`/`src.rs`, placed at `0x40430` by `primary-initializer.ld` | veneer or carrier at a stock address | the stock loader-A caller | no | the loader-A caller is owned | required |
| the 97-slot root descriptor table written by the publisher | kernel interface slot | retained stock root-service callers | no | the RTOS platform owns the root services | required |

## property_service

The property classifier, table generator, service and public entries. Its compatibility contract is [the property shim table](settings-storage.md#property-compatibility-shims).

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| repointed table words selecting `xm1_property_get`/`xm1_property_set` | stock-ABI export | the stock entry/dispatch tables | no | owned entry and dispatch replace both stock dispatchers | required |
| `xm1_getter_stub`/`xm1_setter_stub` stock-frame stubs and the shared exits/fallbacks | veneer or carrier at a stock address | the stock prologues, shared exits and out-of-range ids | no | owned entry and dispatch handle every id and frame | required |
| `xm1_storage_store8` and `xm1_recipe_edit_get` retained services | trampoline into retained stock | the retained bank/previous-value storage and recipe getter | no | owned settings storage and the recipe getter own those paths | required |
| `xm1_property_get`/`xm1_property_set` exports | C-ABI bridge between two owned units | develop and power_lifecycle | yes | both callers link the crate directly and both sides compare with stock in one image | required |
| base-fixed accessors, named default leaves, bank-0 bracket callees and the carrier in retired setter arms (`src/`, `entry*.S`, `property-service*.ld`) | guest-memory layout view | retained bank/previous-value layout and default leaves | no | owned storage carries the data and defaults | required |

## recipe_ui

The recipe-bank UI route entry points.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `rb_ui_route_page`, `rb_ui_route_slot`, `rb_ui_route_item` exports | stock-ABI export | stock bank route callers | no | the owned bank route calls them directly | required |

## record_store

The owned record store core and guest adapter. Its shim list is [the record-store design](../reports/owned-record-store-design.md#7-shims).

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `record_store_*` exports | C-ABI bridge between two owned units | settings_storage and power_lifecycle | yes | both clients link the store directly and both sides compare with stock in one image | required |
| nine entry veneers at the stock wrapper addresses and `record_store_carrier/entry.S` | veneer or carrier at a stock address | stock callers of `0x6ac2b4`-family entries | no | every client calls the owned API | required |
| stock statuses, ignored write results and stock program lengths | stock-ABI export | stock clients that consume stock statuses and `0x600` kind-3 programs | no | clients consume typed results and the owned NAND partial-program law is qualified | required |
| staging buffers at the stock addresses, handles at `0x89f8b4 + 12i` and the client words | guest-memory layout view | stock clients that read buffers, handles and client words | no | no stock client reads them | required |
| `record_raw_*`, `record_semaphore_*` externs | trampoline into retained stock | the stock raw NAND service and semaphore `0x4b` | no | the owned `NandFlash`/`FlashMap` and RTOS semaphore are qualified | required |
| stock check/prepare callbacks of the five clients | callback registry for guest words | stock area callbacks | no | each client's seal and check are owned | required |

## record_store_carrier

The record-store carrier ELF: entry veneers and import stubs.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `record_store_entry_*` in `entry.S` and `record-store.ld` | veneer or carrier at a stock address | stock callers of the retired record-service span | no | every client enters the owned store directly | required |
| `imports.ld` import stubs | trampoline into retained stock | the retained raw NAND and semaphore services | no | the owned NAND and RTOS services replace them | required |

## render_adapter

The render adapter that presents owned widget drawing at stock draw addresses.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `rb_render_picker_apply`, `rb_render_tree_apply`, `rb_render_pane_apply` in `mod.rs`, with `entry.S` and `build.sh` placements | stock-ABI export | stock draw callers that enter the adapted widget entries | no | the owned renderer calls the widgets directly | required |
| `rb_render_call_*` retained services (draw leaves, hooks, menu-table accessors, setting reads) | trampoline into retained stock | the stock draw layer | no | the owned renderer owns the draw layer | required |
| `rb_render_remainder` in `entry.S` | retained stock seam in assembly | the stock remainder path after an unhandled class | no | every class is owned | required |

## render_commands

The address-independent command, raster and publisher model. It has no stock entry, but it classifies stock resource-catalog ids and layout.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| stock resource-catalog id classification (`src/resources.rs`, `classify_stock_resource`, ids `0xff000000..=0xff0008c5`) | guest-memory layout view | the retained stock resource catalog and bitmaps | no | an owned asset pack covers every owned screen | required |
| stock fifty-unit text window and script/glyph layout in `src/raster.rs` | guest-memory layout view | retained stock font and script resources | no | owned fonts and scripts cover every owned screen | required |

## root_task

The owned root task, calling 28 retained stock services.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| the root-task export in `src.rs`, placed by `root-task.ld` | veneer or carrier at a stock address | the stock root-task entry | no | the root task is entered from owned startup | required |
| 28 `root_*` retained externs (checked init, cleanup, config query/write, resource/worker creation, descriptor/table install, platform/runtime init, periodic callback, later submission) | trampoline into retained stock | stock kernel, resource and configuration services | no | the RTOS platform and owned configuration own those services | required |
| `xm1_app_register_callback` retained service | callback registry for guest words | the stock callback registry | no | the owned application registration owns it | required |

## rtos_cyclic

The owned cyclic-handler family.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_rtos_cyclic_*` exports placed at `0x4be3c`-family addresses by `cyclic-a.ld`/`cyclic-b.ld` | stock-ABI export | stock callers of the cyclic entries | no | every cyclic caller is owned or migrated | required |
| `app-callback.ld` app-callback carrier | veneer or carrier at a stock address | the stock domain-A app caller | no | the app caller is owned | required |
| `xm1_rtos_cyclic_timer_insert`/`_remove` externs | C-ABI bridge between two owned units | rtos_timer | yes | the cyclic crate links the timer crate directly and both sides compare with stock in one image | required |

## rtos_data_queue

The owned data-queue family.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_rtos_data_queue_*` exports placed by `dq-a.ld`/`dq-b.ld` at the stock entries | stock-ABI export | stock queue callers | no | every queue caller is owned or migrated | required |
| `xm1_rtos_scheduler_*` externs (acquire, insert, notify, prepare, release, remove, reset, save) | C-ABI bridge between two owned units | rtos_scheduler | yes | the queue crate links the scheduler crate directly and both sides compare with stock in one image | required |
| `xm1_rtos_data_queue_stock_insert_timer`/`_remove_timer` externs | C-ABI bridge between two owned units | rtos_timer | yes | the queue crate links the timer crate directly and both sides compare with stock in one image | required |

## rtos_exception

The owned exception entries, vectors and ack/EOI wrappers.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_exception_*` vector slots, dispatcher, ack/EOI wrappers and data in `stage/carrier.S`, placed by `exception-a.ld`/`exception-b.ld` at the stock vector addresses | veneer or carrier at a stock address | the stock vector table and exception callers | no | the vector table and every exception entry are owned | required |
| `xm1_exception_stock_identity`, `_log`, `_release`, `_try` retained services | trampoline into retained stock | stock exception identity, logging and release services | no | those services are owned | required |

## rtos_fixed_pool

The owned fixed-pool family.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_rtos_fixed_pool_*` exports in `stage/entries.S`, placed by `fp-a.ld`/`fp-b.ld` | stock-ABI export | stock pool callers | no | every pool caller is owned or migrated | required |
| `xm1_fixed_pool_stock_acquire`, `_release`, `_notify`, `_dispatch` externs | C-ABI bridge between two owned units | rtos_scheduler | yes | the pool crate links the scheduler crate directly and both sides compare with stock in one image | required |
| `xm1_fixed_pool_stock_mask`, `_restore`, `_save_irq`, `_restore_irq`, `_wait_*`, `_uidivmod` externs | trampoline into retained stock | retained mask/restore, wait and divide helpers | no | the RTOS platform and owned division own those helpers | required |

## rtos_global_lock

The owned global lock.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_rtos_global_lock_try`, `_release`, `_observe` and `helper_try`, `helper_release`, `helper_observe` exports placed at `0x412cc`, `0x412e8`, `0x41304`, `0x427d4`, `0x427fc`, `0x42828` | stock-ABI export | stock lock callers | no | every lock caller is owned or migrated | required |

## rtos_irq_control

The owned IRQ control and handler registration.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_rtos_irq_*` exports placed at `0x41108`, `0x41124`, `0x4d8fc`-family addresses by `irq-a.ld`/`irq-b.ld` | stock-ABI export | stock IRQ callers | no | every IRQ caller is owned or migrated | required |

## rtos_objects

The owned flag and mailbox family.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_rtos_objects_*` exports placed by `family-a.ld`/`family-b.ld` at the stock entries | stock-ABI export | stock flag/mailbox callers | no | every object caller is owned or migrated | required |
| `xm1_rtos_scheduler_*` externs (acquire, insert, notify, prepare, release, remove, reset, save) | C-ABI bridge between two owned units | rtos_scheduler | yes | the object crate links the scheduler crate directly and both sides compare with stock in one image | required |
| `xm1_rtos_objects_stock_insert_timer`/`_remove_timer` externs | C-ABI bridge between two owned units | rtos_timer | yes | the object crate links the timer crate directly and both sides compare with stock in one image | required |

## rtos_own_first

The 17 own-first root slots.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_rtos_own_first_slot_*` exports placed by `own-first-a.ld`/`own-first-b.ld` at the stock root-slot addresses | stock-ABI export | stock root callers that enter the 17 slots | no | every root-slot caller is owned or migrated | required |
| root descriptor and task-record views used by the slots (`src/lib.rs`) | kernel interface slot | the stock kernel root descriptors and task records | no | the RTOS platform owns the root services | required |

## rtos_platform

The platform aggregate: 113 veneer entries per domain and 57 externals per domain declared in `carriers.json`, with `veneer-a.S`/`veneer-b.S` generated from it.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| 37 `kind: owned` externals per domain in `carriers.json` (scheduler/timer calls into other RTOS family units) | C-ABI bridge between two owned units | rtos_scheduler, rtos_timer and the family units | yes | the aggregate links the family crates directly and both sides compare with stock in one image | required |
| 20 `kind: retained` externals per domain in `carriers.json` (stock IRQ mask/restore, wait, divide helpers) | trampoline into retained stock | stock IRQ and wait helpers | no | the RTOS platform owns those helpers | required |
| 113 veneer entries per domain in `veneer-a.S`/`veneer-b.S`, placed by `platform-a.ld`/`platform-b.ld` | veneer or carrier at a stock address | stock callers of the replaced root slots | no | every root-slot caller is owned or migrated | required |

## rtos_scheduler

The owned scheduler groups S1, S2, S3a-S3d.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| S1/S2/S3 stock-ABI exports placed at the stock entry addresses by the `s3*-a.ld`/`s3*-b.ld` and `scheduler-*.ld` scripts | stock-ABI export | stock scheduler, task and exception callers | no | every scheduler caller is owned or migrated | required |
| `s3a_stock_*`, `s3b_*`, `s3c_*`, `s3d_*` externs marked `kind: owned` in `carriers.json` (insert, remove, notify, switch, timer insert/remove) | C-ABI bridge between two owned units | the scheduler's own S1/timer groups and the RTOS family units | yes | the groups link as Rust items and both sides compare with stock in one image | required |
| `xm1_rtos_scheduler_stock_raise_irq` and `xm1_rtos_scheduler_stock_raise_irq`-family retained externs | trampoline into retained stock | the stock IRQ raise helper | no | the RTOS platform owns IRQ raising | required |

## rtos_semaphore

The owned semaphore family.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_rtos_semaphore_*` exports placed by `sem-a.ld`/`sem-b.ld` at the stock entries | stock-ABI export | stock semaphore callers | no | every semaphore caller is owned or migrated | required |
| `xm1_semaphore_stock_acquire`, `_release`, `_notify`, `_dispatch` externs | C-ABI bridge between two owned units | rtos_scheduler | yes | the semaphore crate links the scheduler crate directly and both sides compare with stock in one image | required |
| `xm1_semaphore_stock_mask`, `_restore`, `_wait_*` externs | trampoline into retained stock | retained mask/restore and wait helpers | no | the RTOS platform owns those helpers | required |

## rtos_task_records

The owned task-record create/delete entries.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_rtos_task_create`/`xm1_rtos_task_delete` exports placed by `task-records-a-native.ld`/`task-records-b-native.ld` | stock-ABI export | stock task-creation callers | no | every task caller is owned or migrated | required |
| `xm1_task_mask_save`/`xm1_task_mask_restore` externs | trampoline into retained stock | the stock mask helpers | no | the RTOS platform owns mask handling | required |

## rtos_timer

The owned timer heap.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_rtos_timer_initialize`, `_insert`, `_remove`, `_tick` exports placed by `timer-a.ld`/`timer-b.ld` at `0x44a50`-family addresses | stock-ABI export | stock timer callers | no | every timer caller is owned or migrated | required |

## save

The save facade and its retained image-5 services.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_save_facade`, `xm1_save_completion`, `xm1_save_release_completion`, `svc_*` exports in `src/lib.rs`/`save_entry.S` | stock-ABI export | stock save callers | no | every save caller is owned or migrated | required |
| `xm1_save_facade` export | C-ABI bridge between two owned units | develop | yes | develop links the facade directly and both sides compare with stock in one image | required |
| `rb_save_*` retained services in `save_stock.S` (current task, extent copy/name/preset, prepare metadata, register release, cached state, service byte/present/profile, signal, stage, storage direct/kind-three, wait) | trampoline into retained stock | stock image-5 save and storage services | no | those services are owned | required |
| `save.ld`, `stage.ld`, `storage.ld`, `wait.ld` placements and the `xm1_save_*_probe` entries | veneer or carrier at a stock address | stock save callers and the qualification probes | no | owned code is called directly | required |

## secondary_stage

The owned record-2 startup.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_secondary_entry`, `xm1_secondary_handoff`, the handoff vector and the log read/store in `entry.S`/`src.rs` | veneer or carrier at a stock address | the stock record-2 vector and handoff | no | the record-2 vector and handoff are owned | required |
| `xm1_retained_companion_*`, `xm1_retained_config_record`, `xm1_retained_conversion`, `xm1_retained_delay`, `xm1_retained_peripheral_*`, `xm1_retained_segment_lookup`, `xm1_retained_structure_init`, `xm1_secondary_log_timestamp` | trampoline into retained stock | retained companion, configuration and peripheral services | no | those services are owned | required |

## service

The retained row-availability probe binding.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `rb_service_field_row_state` export | stock-ABI export | stock menu callers of the row probe | no | the menu engine declares availability itself | required |
| `rb_call_stock_probe` in `rb_service_stock.S` | trampoline into retained stock | the stock probe `FUN_00d52684` | no | the probe is owned | required |

## settings_storage

The owned settings storage, bank projections, custom record lifecycle and header mirror.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `record_store_append`/`record_store_register` externs | C-ABI bridge between two owned units | record_store | yes | settings links the store directly and both sides compare with stock in one image | required |
| `xm1_storage_output8`/`_output16`/`_status32` externs | C-ABI bridge between two owned units | property_service | yes | settings links the property service directly and both sides compare with stock in one image | required |
| `xm1_getter_owned`, `xm1_setter_owned`, `xm1_storage_initialize` exports | C-ABI bridge between two owned units | power_lifecycle | yes | power links the settings crate directly and both sides compare with stock in one image | required |
| `settings_bank_*` and `settings_custom_*` entries in `bank_entry.S`/`custom_entry.S`, placed by `settings-storage.ld`/`custom-settings.ld` | veneer or carrier at a stock address | stock bank/custom callers | no | every bank/custom caller is owned or migrated | required |
| 28 retained `xm1_stock_*`, `xm1_bank_*`, `xm1_storage_*` and `settings_custom_*` externs | trampoline into retained stock | stock bank, header, storage and custom-record services | no | owned storage replaces those services | required |
| header mirror, previous-value snapshot `0x89ec00`, backup memory `0x409c281c` and the 16-byte shadow `0x409c29b8` | guest-memory layout view | retained stock readers and the bank-0 backup contract | no | no stock code reads the mirror or backup | required |

## startup_dispatch

The phase dispatcher.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_startup_dispatch` and `xm1_startup_dispatch_entry` in `entry.S`/`src.rs`, placed by `startup-dispatch.ld` | veneer or carrier at a stock address | the stock phase entry at `0x69cdf8` | no | the phase entry is owned | required |
| `xm1_startup_clear`, `xm1_startup_install`, `xm1_startup_mode`, `xm1_startup_submit` retained services | trampoline into retained stock | stock startup clear/install/mode/submit services | no | those services are owned | required |

## task1_actor

The owned task1 UI actor and its pump.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| the actor export and `task1-actor.ld` placement | veneer or carrier at a stock address | the stock task1 entry at `0xc22f60` | no | the task1 entry is owned | required |
| 29 retained `extern "C"` declarations (queue receive/service, pending predicates, controller setup, pump, registration, diagnostics) | trampoline into retained stock | stock image-4/image-5 application and queue services | no | those services are owned | required |

## task_constructors

The owned task constructors.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| 13 `xm1_ctor_*` entries in `entry.S`, placed by `task-constructors.ld` at the stock constructor addresses | veneer or carrier at a stock address | the stock constructor caller list | no | the constructor list is owned | required |
| 21 `xm1_sac_*`, `xm1_create_queue`, `xm1_record_*`, `xm1_submit_task`, `xm1_usb_clear`, `xm1_diag_initialize` retained services | trampoline into retained stock | stock subsystem constructors | no | those subsystems are owned | required |

## task_storage

The owned task-storage transition.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_task_storage_entry` and `xm1_task_storage_transition` in `entry.S`/`src.rs`, placed by `task-storage.ld` | veneer or carrier at a stock address | the stock task-storage caller | no | the caller is owned | required |

## task_submission

The owned task submission and release wrappers.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_task_submit_entry`, `xm1_task_release_entry` in `entry.S` and `xm1_task_submit`/`xm1_task_release` in `src.rs` | veneer or carrier at a stock address | the stock submit/release callers | no | every caller is owned or migrated | required |
| `xm1_task_submit`/`xm1_task_release` exports | C-ABI bridge between two owned units | power_lifecycle | yes | power links the submission crate directly and both sides compare with stock in one image | required |
| `xm1_task_cache`, `xm1_task_critical_enter/exit`, `xm1_task_error_callback_dispatch`, `xm1_task_fill`, `xm1_task_log`, `xm1_task_reservation`, `xm1_task_try_lock`, `xm1_task_unlock` retained services | trampoline into retained stock | stock kernel and cache services | no | the RTOS platform owns those services | required |

## validator

The area-3 record validator replacement and its stock-ABI adapter.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `area3_validate` export in `validator.rs`/`faulty.rs`, placed by `validator.ld` | stock-ABI export | the stock area-3 check callback address | no | the record store calls the Rust validator directly | required |
| `adapter.rs` host C-ABI shim | stock-ABI export | the native qualification adapter | no | the adapter is no longer needed for qualification | required |

## vendor_protocol

The vendor, wireless, filesystem and device-protocol unit. Most of its externals are owned units; the remaining seam is the stock USB/wireless and filesystem surface.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_app_nand_*`, `xm1_power_*`, `xm1_scanout_*`, `xm1_rtos_own_first_slot_*` externs (owned units) | C-ABI bridge between two owned units | application_nand, power_lifecycle, display_scanout, rtos_own_first | yes | each caller links the owning crate directly and both sides compare with stock in one image | required |
| `vendor_session_initialize`, `vendor_session_control`, `vendor_session_context_size`, `vendor_session_context_alignment`, `xm1_vendor_usb_task`, `xm1_vendor_usb_interrupt` exports | stock-ABI export | stock USB/session callers | no | every USB/session caller is owned or migrated | required |
| `xm1_wireless_driver_task`, `xm1_wireless_supplicant_task` retained services | trampoline into retained stock | stock wireless driver and supplicant tasks | no | the wireless subsystem is owned | required |
| filesystem and media-task ABI views (`filesystem_*`, `media_parser`, `metadata_*`) | guest-memory layout view | retained stock filesystem, media and metadata services | no | the owned file service and filesystem are qualified | required |

## Not registered

`replacement/rtos_layout` and `replacement/rtos_reservation` contain Rust but no firmware artifact target; they are shared host views, not units. `replacement/toolchain` contains build rules only. `replacement/record5_staging`, `startup_artifact`, `startup_composition` and `startup_consumers` contain no Rust or assembly sources; they are host orchestration.
