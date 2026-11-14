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
| RTOS family scheduler, timer and flag-helper calls | rtos_scheduler, rtos_timer, rtos_objects → rtos_objects, rtos_semaphore, rtos_data_queue, rtos_fixed_pool, rtos_cyclic, rtos_task_records | 49 `kind: owned` externals per domain in `rtos_platform/carriers.json` (scheduler insert/remove/notify/switch, timer insert/remove, and the event-flag wait helpers the semaphore, fixed-pool and S3b bodies call) | Each family links the scheduler, timer and object crates directly and the aggregate carries stock comparisons for both sides | required |

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
| `xm1_kernel_console9`, `xm1_kernel_critical_enter` and `xm1_kernel_critical_exit` in `develop.ld` | trampoline into retained stock | loader-A console writer 0x41a34 and cross-core critical pair 0x7235d8/0x7236b0 through the linked property service's kernel backend | no | develop's property service uses owned console and cross-core services | required |
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

## kernel

The kernel crate: typed handles over the image's interface slots, with the per-core slot table as its first backend.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `SlotTable` in `src/slot_table.rs` and `src/slot_table/messages.rs` | kernel interface slot | the kernel bodies behind the root blocks at 0x96bd20 and 0x96bd24, the loader-A console writer 0x41a34, the error-callback word at 0x4089ea8c, the post hook at 0x4089ea94, task/queue classes and message-pool capacity words | partly | every image whose `.symbols` lists an `xm1_kernel` item also lists the kernel family entry for each slot offset in `SlotTable`'s table | required |
| `CoreLock` in `src/slot_table.rs` | trampoline into retained stock | the stock cross-core critical section at 0x7235d8 and its release 0x7236b0 | partly | no stock caller of 0x7235d8 remains | required |
| Object construction and deletion in `src/slot_table/messages.rs` | trampoline into retained stock | queue constructor 0x724cdc and deleter 0x724f08, binary-semaphore allocator 0x7247ac and deleter 0x724974, semaphore poll 0x724ad8 | no | the kernel crate owns queue backing allocation and resource lifetime, and semaphore allocation/deletion/poll, in every linked image | required |
| Dispatch bracket in `src/slot_table/messages.rs` | trampoline into retained stock | dispatch disable 0x723efc and enable 0x723f7c | no | the kernel crate owns the dispatch wrappers and their error reporting in every linked image | required |

## kernel_install

The kernel installer and its cache maintenance.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_kernel_install` and `xm1_kernel_install_entry` in `entry.S`/`src.rs` | veneer or carrier at a stock address | the stock phase entry and installer callers | no | the phase entry and installer callers are owned | required |
| `xm1_kernel_cache` retained service | trampoline into retained stock | the stock cache-maintenance helper | no | cache maintenance is owned | required |

## saclac/lens_drive

saclac's lens children (machines 1, 2 and 35). Its cores take typed events and leave through port traits; `src/stock.rs` holds the stock codes and entries the retained engine and reply layer still use.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `live_event`, `s1_event`, `bring_up_event` and `outcome_word` in `src/stock.rs` | guest-memory layout view | the event codes the row functions `0xefd2fc`, `0xf0194c` and `0xefd624` route from machines 11, 3 and 0, and the action return words 0 to 3 | no | the lens children are entered only through `children::LensChildren` from owned routers linked in the same image | required |
| `reply_call` and `reply_target` in `src/stock.rs`: each reply's client and argument words, which `saclac_root` passes to `saclac_shared`'s `Commands` in place of the reply entries `0xefb104`, `0xefb194`, `0xefb1dc`, `0xefb604`, `0xefb654`, `0xefb6ac`, `0xefa348` and `0xefa3a8` | guest-memory layout view | the client callbacks the queue-43 producers store in the command blocks | no | every queue-43 client is owned | required |

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
| `rb_menu_engine_call_*` retained services (availability, initial row, kind switch, notify, seek) | trampoline into retained stock | stock menu services | no | the tree and typed settings service answer them | required |
| `rb_menu_engine_guard_failure` and `__aeabi_*` in `adapter/entry.S` | retained stock seam in assembly | stock ABI helper routines and the guard path | no | the engine no longer needs the stock ABI helpers | required |
| block mirror at `0x430` bytes and `0x01467580/84` (`src/state.rs`) | guest-memory layout view | stock readers and writers of the menu block | no | no stock code reads or writes the block | required |
| `rb_ui_probe_visit` page-entry replay | callback registry for guest words | the installed bank's `rb_bank_page_entry` hook | no | the bank's page-entry work is an owned engine consumer | required |

## menu_value

The menu value transaction, published at the dispatcher's IMAGE SIZE jump-table word.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `rb_menu_value_size_arm` in `adapter/entry.S`, placed by `adapter/image.ld` | veneer or carrier at a stock address | the stock dispatcher's jump-table word for property `0xf6` | no | the owned dispatcher calls the transaction directly | required |
| `rb_menu_value_begin`, `rb_menu_value_size_apply`, `rb_menu_value_retained`, `rb_menu_value_cancel`, `rb_menu_value_confirm` exports in `src/lib.rs` | stock-ABI export | stock stage consumers and controller arms | no | every stage consumer and controller arm is owned | required |
| `rb_menu_value_call_*` retained services (aspect/size cells, change marker, current property, display update, restore aux, snapshot cells/state) | trampoline into retained stock | stock property, cell and display services | no | the typed settings service and owned display own those services | required |
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
| `xm1_kernel_console9`, `xm1_kernel_critical_enter` and `xm1_kernel_critical_exit` in `native/record-helpers.ld` | trampoline into retained stock | loader-A console writer 0x41a34 and cross-core critical pair 0x7235d8/0x7236b0 through the linked settings core's kernel backend | no | the settings core uses owned console and cross-core services | required |
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
| base-fixed accessors, named default leaves, bank-0 bracket callees and the carrier in retired setter arms (`src/`, `entry*.S`, `property-service*.ld`, `menu/recipe_bank_ram_engine.ld`) | guest-memory layout view | retained bank/previous-value layout and default leaves, and the owned image's link script, which binds `0x6439e0`, the six default leaves and the three backup callbacks | no | owned storage carries the data and defaults | required |
| The property service carried twice: the stock-visible carrier at `0x62c120` (`property-service-public.ld`) and the `property-service-arm` rlib the owned menu adapters link | veneer or carrier at a stock address | stock callers of `0x62c120` | no | no stock caller enters the `0x62c120` carrier | required |

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
| `rb_render_call_*` retained services (draw leaves, hooks, menu-table accessors) | trampoline into retained stock | the stock draw layer | no | the owned renderer owns the draw layer | required |
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
| `xm1_fixed_pool_stock_wait_release`, `_wait_cancel`, `_wait_enqueue`, `_wait_enqueue_timed` externs at A `0x44568`/`0x44744`/`0x44800`/`0x448e4` (B `0xd4528`/`0xd4704`/`0xd47c0`/`0xd48a4`) | C-ABI bridge between two owned units | rtos_objects event-flag carriers | yes | the pool crate links the object crate directly and both sides compare with stock in one image | required |
| `xm1_fixed_pool_stock_mask`, `_restore`, `_save_irq`, `_restore_irq`, `_uidivmod` externs | trampoline into retained stock | retained mask/restore and divide helpers | no | the RTOS platform and owned division own those helpers | required |

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
| Thirteen event-flag carrier entries per domain in the aggregate `carriers.json` and `veneer-a.S`/`veneer-b.S`: eight flag services and the `family_release`, `helper_release` (`0x44744`/`0xd4704`), `helper_block`, `helper_block_timed` and `helper_check` bodies | veneer or carrier at a stock address | stock flag callers, and retained stock mailbox send, receive and timed receive that branch into the helper extents | no | every flag caller is owned or migrated | required |
| `xm1_rtos_scheduler_*` externs (acquire, insert, notify, prepare, release, remove, reset, save) | C-ABI bridge between two owned units | rtos_scheduler | yes | the object crate links the scheduler crate directly and both sides compare with stock in one image | required |
| `xm1_rtos_objects_stock_insert_timer`/`_remove_timer` externs | C-ABI bridge between two owned units | rtos_timer | yes | the object crate links the timer crate directly and both sides compare with stock in one image | required |
| `xm1_rtos_objects_stock_timeout` (`0x44318`/`0xd42d8`) | C-ABI bridge between two owned units | the owned scheduler timeout body behind the aggregate veneer | yes | the object crate names the owned timeout callback directly | required |

## rtos_own_first

The 17 own-first root slots.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_rtos_own_first_slot_*` exports placed by `own-first-a.ld`/`own-first-b.ld` at the stock root-slot addresses | stock-ABI export | stock root callers that enter the 17 slots | no | every root-slot caller is owned or migrated | required |
| root descriptor and task-record views used by the slots (`src/lib.rs`) | kernel interface slot | the stock kernel root descriptors and task records | no | the RTOS platform owns the root services | required |

## rtos_platform

The platform aggregate: 126 veneer entries per domain and 60 externals per domain declared in `carriers.json`, with `veneer-a.S`/`veneer-b.S` generated from it.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| 49 `kind: owned` externals per domain in `carriers.json` (scheduler/timer calls into other RTOS family units, plus the event-flag wait helpers) | C-ABI bridge between two owned units | rtos_scheduler, rtos_timer, rtos_objects and the family units | yes | the aggregate links the family crates directly and both sides compare with stock in one image | required |
| 11 `kind: retained` externals per domain in `carriers.json` (stock IRQ raise/clear, mask/restore, divide helpers) | trampoline into retained stock | stock IRQ, mask/restore and divide helpers | no | the RTOS platform owns those helpers | required |
| 126 veneer entries per domain in `veneer-a.S`/`veneer-b.S`, placed by `platform-a.ld`/`platform-b.ld` | veneer or carrier at a stock address | stock callers of the replaced root slots | no | every root-slot caller is owned or migrated | required |

## rtos_scheduler

The owned scheduler groups S1, S2, S3a-S3d.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| S1/S2/S3 stock-ABI exports placed at the stock entry addresses by the `s3*-a.ld`/`s3*-b.ld` and `scheduler-*.ld` scripts | stock-ABI export | stock scheduler, task and exception callers | no | every scheduler caller is owned or migrated | required |
| `s3a_stock_*`, `s3b_*`, `s3c_*`, `s3d_*` externs marked `kind: owned` in `carriers.json` (insert, remove, notify, switch, timer insert/remove, and `s3b_release_wait` at `0x44568`/`0xd4528` bound to the event-flag family release carrier) | C-ABI bridge between two owned units | the scheduler's own S1/timer groups, rtos_objects and the RTOS family units | yes | the groups link as Rust items and both sides compare with stock in one image | required |
| `xm1_rtos_scheduler_stock_raise_irq` and `xm1_rtos_scheduler_stock_raise_irq`-family retained externs | trampoline into retained stock | the stock IRQ raise helper | no | the RTOS platform owns IRQ raising | required |

## rtos_semaphore

The owned semaphore family.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_rtos_semaphore_*` exports placed by `sem-a.ld`/`sem-b.ld` at the stock entries | stock-ABI export | stock semaphore callers | no | every semaphore caller is owned or migrated | required |
| `xm1_semaphore_stock_acquire`, `_release`, `_notify`, `_dispatch` externs | C-ABI bridge between two owned units | rtos_scheduler | yes | the semaphore crate links the scheduler crate directly and both sides compare with stock in one image | required |
| `xm1_semaphore_stock_wait_release`, `_wait_cancel`, `_wait_enqueue`, `_wait_enqueue_timed` externs at A `0x44568`/`0x44744`/`0x44800`/`0x448e4` (B `0xd4528`/`0xd4704`/`0xd47c0`/`0xd48a4`) | C-ABI bridge between two owned units | rtos_objects event-flag carriers | yes | the semaphore crate links the object crate directly and both sides compare with stock in one image | required |
| `xm1_semaphore_stock_mask`, `_restore` externs | trampoline into retained stock | retained mask/restore helpers | no | the RTOS platform owns those helpers | required |

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

## saclac

The task-43 driver and engine use stock's 37 machine tables. Every machine action remains a stock action word. The three boundary entries are the dispatcher, machine-0 router and task loop.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| three four-byte image-5 veneers at `0xefca6c`, `0xefcb88` and `0xf01bc8`, with bodies in the shared fixed image beginning at `0x00ba0020`; unused retired extent bytes carry trap word `0xe7f000f0` | veneer or carrier at a stock address | retained row-function callers, task constructor and nested stock action calls | no | every machine and retained entry caller is owned | required |
| `StockTables`, `StockPorts` and the guest addresses in `src/stock.rs`, with the four per-machine count arrays `SACLAC_CELL_COUNTS`, `SACLAC_ACTION_COUNTS`, `SACLAC_SECOND_COUNTS` and `SACLAC_ROW_COUNTS` and machine 0's row map `SACLAC_ROW_MAP` exported for the layout check | guest-memory layout view | the cell, action and second tables at `0xf12f70`, `0xf133ec` and `0xf12074`, the state counts at `0xf1158c`, and the two halfword arrays | no | every machine owned | required |
| the trace entry `0xf46c38` called through the literal word at `0xefcce4`, declared in `Replacement.Saclac.Artifact.retainedExternals` | trampoline into retained stock | the stock trace body at `0xf46c38` | no | the trace recorder is owned | required |
| stock action words and second-table words entered through `StockCodeWord` with r2 = the machine's table base word and r3 = the entered word, the values stock's dispatcher holds at `0xefcb20` and `0xefcb58` | trampoline into retained stock | the 813 distinct action words the cells reference, all image-5 code addresses, of which 701 are declared function starts and 112 are entries the decomp does not declare; no cell selects a second-table word; no entry reads r2, and 224 save r3 in a store-multiple prologue, the only r3 read the walk finds | no | every machine owned | required |
| `xm1_saclac_driver_entry` at `0xf01bc8` | stock-ABI export | retained task constructor | no | the task constructor calls owned Rust directly | required |
| `DriverPorts` receive scratch at `0x169d374` with payload at `0x169d380`, command blocks and command-present flags | guest-memory layout view | stock machine actions and queue-43 producers | no | every machine and all queue-43 producers are owned | required |
| fault reaction `0xf46c20` | trampoline into retained stock | retained SACLAC fault reporting | no | subsystem fault reporting is owned | required |
| `XM1_REQUIRES_LENS_SAFETY_CONTRACT`, four bytes with value 1 in the linked fixed body | guest-memory layout view | fixed-image stager derives the emulator-only admission contract from this marker | no | a lens safety contract qualifies the image's lens-reaching effects | required |

The clean controllers crate's Root port requires Lens3c's completion fields in the Root record for status notification `0xf46f28`, followed by current Root-context release. Ordinary Started completions use the Live record instead. Command24 has no deferred-option root binding: it forwards the original event to Live. The obsolete root `LiveSequence`, `RefreshDeferred` and `SetDeferredOption` variants are removed. The production composition owner binds these ports and qualifies ordered boundary scenarios in the linked image before retiring the corresponding stock actions. S1's explicit zero definitions and retained-reader limits are recorded in [the boundary documentation](README.md#saclac).

## saclac_shared

The helpers several saclac modules call (`replacement/saclac/shared`), over the stock services and memory other tasks own. All rows are in `src/stock.rs`.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `Stock<Guest>`'s `CommandMemory`: parameter-block words through the interface's block table (read and written; the lens children write LensDrive word 3 from Cancel word 1 as `0xf14c20` does) and the pending bytes at `0x169cf24`, one per message code, commands and notification 0x1b alike | guest-memory layout view | retained queue-43 producers and stock machine actions that read the blocks and flags | no | queue-43 producers and every machine are owned | required |
| `Stock<Guest>`'s `Clients`: client callback words entered with the status, or the status and four words | trampoline into retained stock | client callbacks stored in the command blocks by other tasks | no | every queue-43 client is owned | required |
| `Stock<Guest>`'s `LensControl` (`0x76d9a8`, `0x76da38`, `0x76dba4`, `0x76da48`, `0x76b8b4`, `0x772f28`, runtime word `0x942ab0`+0x98) and `LensNotices` (block `0x954dbc`: notices +4/+8/+0xc, notifier +0x1c, enable +0x20; notifier entered with a four-word record) | trampoline into retained stock | lens control and the tasks that write the notice block (104, 59) | no | lens control is owned | required |
| `Stock<Guest>`'s `Rendezvous`: current task `0x723cb8` stored at `0x16c2214`, wait `0x7243a8` on event 2, wake callback `0xf46be0` | trampoline into retained stock | the kernel and saclac's own retained wake callback | no | the driver supplies a kernel-backed rendezvous with an owned wake entry | required |
| `Stock<Guest>`'s `Adjustments` (`0x69f8fc`, with the lens adjustment halfword at `0x3e0243` and the unselected-mode byte at `0x887c`), `ChannelRemap` (`0x74947c`), `Conversions` (`0x73958c`, `0x734540`, `0x73469c`) | trampoline into retained stock | the settings and conversion services | no | the settings services are owned | required |
| `Stock<Guest>`'s `RegisterSnapshot`: `0x6deb08` over the descriptor tables `0x82158c`..`0x821808`, the fourteen retained packers `0x6913a8`..`0x693aec` | trampoline into retained stock | the image register driver | no | the image pipeline's register layer is owned | required |
| the register snapshot at `0x14c6b24` and a pipeline configuration: snapshot words at +0x3c and +0x40, read and written at `0x691f00` and `0x691f0c`, and configuration halfwords at +0x3e, +0x40 and +0x42, read at `0x691e40` and `0x691e4c` | guest-memory layout view | the image pipeline's register snapshot and configuration records | no | the image pipeline's register layer is owned | required |
| `Stock<Guest>`'s `LensComClient`: client word and request number at `0x8e1f3c`+8/+0xc, completion `0x6b6f9c`, failure report `0x6b92cc` | trampoline into retained stock | lenscom's completion path | no | lenscom requests complete through an owned callback | required |
| `Stock<Guest>`'s `Display`: `0x6c68c0`, `0x6c4bec` | trampoline into retained stock | the display driver | no | the display driver is owned | required |
| the display driver's 60-byte status record: byte 7 gates the outputs on route 2 (`0x6c5df4`) and bytes 16..19 are the output bits (`0x6c693c`, `0x6c6948`, `0x6c6954`, `0x6c6960`) | guest-memory layout view | the display driver's status record | no | the display driver is owned | required |

## saclac_root

saclac's composition root (`replacement/saclac/root`): `Services`, `StageDriver`, `GeometryBindings` and operation-scoped `Calculation` bind module ports to the merged modules; `saclac_peers::PeerPorts` supplies kernel queue posts through a persistent poster with per-operation task/interrupt proofs. Calculation borrows the shared services and geometry, not a second exposure context. Retained calls and layouts are explicit `saclac_shared::Machine` boundaries in `src/stock.rs`, `src/stock/` and the controller service adapters. Static leaf interpretation is recorded in E-SACLAC-ROOT-LEAF-ABI-150; it is not composed-image acceptance. The rows below register those boundaries.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `LensControl::send` in `src/lens.rs`: a lens drive (0x4207) is not posted while `0x76d9a8`, entered through `saclac_shared`'s `LensControl::probe`, reports 1, as lens control's retired sender `0x76dec8` refuses with 0x81 then (`0x76def4..0x76df00`); every lens request posts through `LensDc::request` | trampoline into retained stock | lens control's state behind `0x76d9a8` | no | lens control is owned and decides whether a drive is admitted | required |
| lens control's resource index `0x76d9c8`, the configuration lookup selection `0x740b18` | trampoline into retained stock | lens control and the configuration services | no | lens control and configuration lookup are owned | required |
| the drive result words `0x954dbc` (live, stored by `0xf14be4`) and `0x16b750c` (S1, stored by `0xf28e94`) | guest-memory layout view | the retained lens completion callbacks | no | the lens completion callbacks are owned | required |
| the focus controller's profile-record handle at `0x16be01c`, under which the round invalidates the focus history (`0xf32a48`) | guest-memory layout view | the stock focus controller that opens and stores the handle | no | the focus controller's record handle is owned by the controllers | required |
| the lens request callback word `0x96bd04`, which config's `LensRequests::store_callback` writes in `src/lens_com.rs` as `0x742a6c` does (`0x742a7c`) before the request goes to `saclac_shared`'s lenscom request | guest-memory layout view | stock memory; no `movw`/`movt` reader of it in images 4 and 5 | no | a reference census of images 4, 5 and 6 shows no retained reader | required |
| `stock/stages.rs` CCDIF acquire/release, enable, mode, reset and register parts: `0x65df68`, `0x65e0c8`, `0x65fcb4`, `0x65f3d8`, `0x66049c`, `0x65fd70`, `0x66032c`, `0x65f51c`, `0x660040`, `0x65f9c8`; client/session record is pointer-based except release's two by-value words | trampoline into retained stock | CCDIF shared by image-pipeline callers | no | CCDIF is owned and linked | required |
| `stock/geometry.rs` pipeline stop/release/settle, statistics and residual payloads: `0xd9f504`, `0xd9ef3c`, `0xd9f0a0`, `0xda035c`, `0xd9ffe8`, `0xda0ab0`, `0xda09f4`, `0xdab900` | trampoline into retained stock | image pipeline shared by tasks 11..24 | no | pipeline service and residual payload entries are owned | required |
| `stock/geometry.rs` residual scan setters, channel-code lookup `0xd5dce8`, settings/geometry exporters `0xd5e0d4`/`0xd5dfc4`, selection loaders `0xd5e0f8`/`0xd5dfd4` | trampoline into retained stock | shared scan library; owned config supplies applied-parts packing, defaults, compilation and value tables | no | residual scan library is owned | required |
| `stock/geometry_radial.rs`: model `0x6e3b0c`, fit `0x6e3744`, bound `0x6e3488`, centre `0x6e3400`, extent `0x6e35f4`, table `0x6e384c` | trampoline into retained stock | image-4 radial geometry model | no | radial geometry is owned and compared | required |
| handler register/detach `0x7411c4`/`0x74140c`, exported `frame_handler`, and stored operation/geometry callback words | stock-ABI export | interrupt handler service and retained callers storing completion words | no | handler service and every callback producer are owned | required |
| geometry dump region reserve/register/release `0x75e6d8`/`0x75e704`/`0x75e6f0` | trampoline into retained stock | dump region service | no | region service is owned | required |
| diagnostic strings/tables `0xeeb460`, `0xeeb3a8`, `0xeeb3d0`, `0xeeb2c4`, `0xeeb2d0`, `0xeeb2d4`; geometry blocks through `layout::address`, raw statistics/MMIO, vector-page reads and point buffer `0x014c59e8` | guest-memory layout view | retained profile, pipeline and diagnostic users | no | shared layout users are owned | required |
| geometry retained band calls `0xef4bf4`, `0xef54fc`, `0xef5190`, `0xef5d44`, `0xef743c`, `0xef74e4`, `0xedae88`, `0xeea014`, `0xef4274`, `0xef3f04`, `0xef74ac`, `0xef73e4`; result dump `0xd6b3f8` | trampoline into retained stock | L4 band portions and result snapshot service | no | each called portion is owned and linked | required |
| geometry output/configured-rate `0x1004618`/`0x1007ce0`, power level `0x64982c`, ADC `0x741ee0`, task setup/delay `0x723a14`/`0x723ce8`, peak service `0x6b7830`/`0x6b7840`/`0x6b7820`, profile resource `0x750474` | trampoline into retained stock | capture, power, kernel, interface, peak and profile services | no | each service is owned and linked | required |
| geometry display calls `0x6cfed0`, `0x6cfe14`, `0x6d0000`, `0x6d1b24`, `0x6cffbc`, `0x6d0244`, `0x6d0ee4`, `0x6d01bc`, `0x6cfcf4` | trampoline into retained stock | retained display service; select uses direct owned display_scanout `mode_frontends::apply_selected` | no | each residual display service is owned and linked | required |
| geometry file operations `0x70fbfc`, `0x742250`, `0x742260`, `0x71c648`, `0x71c75c`, `0x71c7bc`, `0x71c938`, `0x70fb68`, exported `file_opened`/`file_done` | stock-ABI export | storage service and its completion callbacks | no | storage API and all callback producers are owned | required |
| floating-point logarithm `0x894778` and square root `0x894df0` | trampoline into retained stock | soft-float runtime | no | owned numeric implementation is qualified | required |
| scene classification gate `0xfb236c(0)` from root `scene::ClassificationPorts` | trampoline into retained stock | retained classifier gate before queue46 post | no | classifier gate is owned | required |
| `root/search_bindings::stock` lens state/limits: `0x76da84`, `0x76d9c8`, `0x76f340`, `0x76dbb4`, `0x746054`, `0x76f360`, `0x76da38`, `0x76b8b4`, `0x76d9a8`, `0x76d9ec`, `0x74bd64`; configuration derivations use the registered config/shared stock services | trampoline into retained stock | retained lens control and settings services | no | each lens/settings service is owned and linked | required |
| search capture/timing/numerics: `0x1007e44`, `0x1005f60`, `0x1004d04`, `0x1004ce0`, `0x1004d68`, `0xee9ab0`, `0xee99f0`, `0xee97f0`, `0xee9928`, `0xfea7ac`, `0xfea7bc`, `0xfeaacc`, `0xfeaadc` | trampoline into retained stock | capture pipeline and numeric/runtime services | no | each service is owned and joined | required |
| search selection/program helpers: `0xf5206c`, `0xef74e4`, `0x772f28`, `0xf77ef8`, `0xf784f8`, `0xf77c8c`, `0xf78d24` | trampoline into retained stock | profile/adjustment helpers whose retained argument shapes remain shared | no | typed owned services supply the full caller contract | required |
| search diagnostics: fault `0xf46c20`, reset log `0xf47468`, ticks `0x7257a8`, angular rate `0x6b7830`/`0x6b7840`, debug text `0x1110918`, trace `0x64744c` | trampoline into retained stock | fault, clock, angular-rate and diagnostic services | no | those services are owned and qualified | required |
| search log base/index `0xf49358`/`0x16c2228` (301 records of108 bytes), shot position `0x16cb404`, trace strings `0xffb104`/`0xf46abc`/`0xf46ac0`, profile/shared record fields and callbacks `0xf3c104`/`0xf25758`/`0xf3c14c`/`0xf25728` | guest-memory layout view | retained diagnostic/shot readers and calc/imgdc completion producers | no | every retained reader and completion producer uses owned typed state | required |
| Acquisition record-derived services: `0xfa7634(record)`, `0xfa7954(record)`, `0xfa7984(record)`, `0xfa6bb4(record)`, `0xf5f9fc(record)`, `0xef74e4(profile_index)`, `0xf51b94(record, 0xffff)` | trampoline into retained stock | record conversions and the capture-profile table | no | owned record conversions and capture profiles supply these values through typed ports | required |
| Acquisition lens services: `0xf4f16c(record, lens_or_ffff)`, `0xfa7edc(adjustment)`, `0xef7820(kind)`, `0x746804(step)`, `0x1007e44(code, selector)` | trampoline into retained stock | lens-word composition, gain selection, kind classification and calibration tables | no | the owned lens and capture-profile services expose these operations | required |
| Program settlement: `0xf797f0` (8 arguments), `0xf79b94` (7), `0xf7a0cc` (17, including three in/out halfwords and output code/selector) | trampoline into retained stock | adjustment-profile target and range settlement | no | calc's owned settlement and resolution expose the acquisition's target, timing and in/out level contract; selection, profile metadata, percentages and lower bounds already call owned calc/config | required |
| Acquisition grid/rate helpers: `0xfab250` (4), `0xfa7e20` (3), `0xfa7bb8` (2), `0xfa7604` (0), `0xf625cc` (5), `0xfa6934` (6; last two zero), `0xf5ecb0` (3), `0xf5ecc0` (4), `0xf5f368` (6), `0xf4ddbc` (2), `0xf624a8` (9; branch last) | trampoline into retained stock | calc's grid count, profile class, grid builder, position capture, rate and clamp contracts | no | those owned calc APIs expose the request's argument/result shapes; configuration indexing/limits, treatment admission, class/offset policy already call owned calc | required |
| Rate source and diagnostics: `0xf4dd50(buffer)`, `0xfa95b8(record, result, flag, out_first, out_second)`, `0xfa8520(record, flag)`, `0xfa91b0(result, flag)`, `0xf521d8(level, word, flag)`, `0xfaa7b0(record, rate_record, out_word, out_halfword)` | trampoline into retained stock | scene recognition and rate diagnostics' retained result-record interface | no | the composition root joins the owned scene-recognition/diagnostic state and adapts the retained result record into its inputs | required |
| Capture/pipeline calls: `0xfea79c(slot)`, `0xfea7ac(slot, out_5c4)`, `0xfeaacc(0)`, `0xfeaadc(0, out_6c)`, `0xfea7bc(out)`, `0xfea7cc(out)`, `0xfa95a8(timing, out_interval)`, `0x103b064()`, `0x103af30(level)`, `0xef5310(timing)` | trampoline into retained stock | imaging capture slots and capture-profile timing/level tables | no | imaging and capture-profile services expose these operations through the root's machine boundary | required |
| Shot selection calls: `0xf162e4(defaults, record, 0, 1, 0)`, `0xf1fc5c(setting, lens)`, `0xf17abc(copy)`, `0xf1fbe8(shot)`, `0xf1d134(saved)`, `0xf1d0f4(record, position)`, `0xf17af8(1)`, `0xf1d5d4(position, index, byte0, byte1, flag, word, lens_position, level)`, `0xf1c8fc` (9 in/out arguments) | trampoline into retained stock | the shot sequence's saved selection, settings and grid-reference interface | no | shot sequence is joined and these calls become typed owned operations | required |
| `0x7407b0()`, `0x74081c()`, `0x740888(0, out_clock, 0, 0)`, `0x740e3c(address, bytes)`, `0x740e74(address, bytes)`, `0x72872c(point)`, fault `0xf46c20()` | trampoline into retained stock | hardware state/clock, cache visibility, diagnostic trace and task fault handling | no | the platform owns these interfaces; grid visibility cleans 1204 bytes after placing all 1236 | required |
| Calc1 completion callbacks `0xf33568`/`0xf241a8` (0x87/0x9b), `0xf33520`/`0xf241d4` (0x88/0x9c); profile-record selection/rate hook words in groups 20 `AT_0158`/`AT_016C` (13/5 arguments) | trampoline into retained stock | calc1 and the record's retained consumers | no | completion and record consumers use typed owned events; no callback registry is introduced | required |
| `stock/acquisition.rs`: the catalog-backed focus result (132 bytes), selection (44), exposure mirror (76), rate (40), program choice (24), position pairs and grid publications; calc1 result/shot/saved records and live/S1 window/setting regions via `saclac_interface::stock::round_acquisition` | guest-memory layout view | retained profile, calc1 and shot consumers | no | all consumers accept owned typed records; exposure mirror writes require mode flag exactly 1 and preserve unmodified bytes | required |
| Acquisition shared memory: calc1 grid `0x16d4280`; live/S1 grid `0x16bf370`/`0x16b6030`; shot reference/word/defaults `0x16b3d38`/`0x16ad738`/`0x16bfc30`; history `0x5ffee314`..`0x5ffee618`; release classes/offsets/extra/copy `0x5ffbf930`/`0x5ffbece0`/`0x5ffbf4e0`/`0x5ffb6120`, extra source `0x5ffc6340`, snapshot copy source `0x5ffec1b4`; imgdc selection/focus tables `0x16f4488`/`0x16f224c` | guest-memory layout view | calc1, imgdc, capture pipeline and shot sequence | no | those consumers share owned storage and typed publications | required |
| Controller channel hold/claim/publication services `0x74046c`, `0x740404`, `0x740d50`, `0x740cf8`, `0x740cc4`, `0x740bd4`, `0x740b80`, `0x740ad8`; curve level/interpolation/combine `0x7499a0`, `0x746018`, `0x74714c` | trampoline into retained stock | shared configuration channel and lens table services; result adoption, measured level and curve positions call owned Rust directly | no | the shared services are owned and qualified | required |
| Controller lens queries `0x76da48`, `0x76dad4`, `0x76fc4c`, `0x76fbac` in addition to the registered shared probes; `0x76fbac` writes separate channel and fraction outputs | trampoline into retained stock | shared lens control state | no | lens control is owned and linked | required |
| Library diagnostic buffers at `0x1fea2280`, `0x5fead820`, snapshot `0x5ffdb9ec`, memory copy/fill `0x74213c`/`0x742154`; controller Prepare's four-word source pointer | guest-memory layout view | retained diagnostic readers and the command producer; published selection kind/speed/area/exposure are root-owned state | no | all diagnostic and command consumers use owned typed storage | required |
| Root motion-plan services `0x76e4b0`, `0x742088`, `0x10538a4`, `0x10538c4`, `0x1110a84`; remap uses the registered shared `ChannelRemap` | trampoline into retained stock | drive checks, load registers, capture cache and auxiliary display; plan speed and minimum speed call owned config `selected_limit` and `shifted_floor`, not `0x7492ac`/`0x747af8` | no | each residual service is owned and qualified; `0x10538c4` writes nothing, so its formerly unwritten output is explicitly zero-initialized, not a claimed stock snapshot | required |
| Root `ShotMemory` over `shot_sequence::stock::state` locations | guest-memory layout view | the one persistent ShotState and retained calculation/exposure readers; natural-width access and ascending overlapping copies | no | all readers and writers use the same owned state | required |
| Root controller level scale `0xee9ab0`/`0xee99f0`, held lens reference `0x76d9b8`, current lens position `0x76da84` | trampoline into retained stock | numeric runtime and shared lens state; calibration offsets use owned config tables and pending-offset reference uses the persistent ShotState | no | numeric and lens services are owned and qualified | required |
| Calculation image-six levels `0x1006000`/`0x1005df8`, device readiness `0x74081c`, image report `0x740888` with four independently nullable output pointers; logarithm `0x894778` | trampoline into retained stock | image-six exposure services, configuration status and numeric runtime; profile classes/offsets and cell class use owned ProfileGeometry, lens calibration and limits use owned config | no | each residual service is owned and qualified | required |
| Controller reply parameter blocks from `PARAMETER_BLOCKS`, shared pending flags through `CommandMemory`, client callback code words | stock-ABI export | the command's client; owned controller Composer selects echo/probe/notice words and command-ending transitions before the callback | no | every client uses the owned typed reply path and storage | required |

## saclac_config

saclac's configuration layers L3, L8 and L11 (`replacement/saclac/config`), over the stock derivations, scan library, pipeline hardware and face tracking other tasks share. All rows are in `src/stock.rs`.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `StockDerivations`: `0x7498e0`, `0x747aac`, `0x749254`, `0x748818`, `0x74888c`, `0x746120`, `0x7461a8`, `0x747408`, `0x747584`, `0x749be4`, `0x7462e8`, `0x74b05c` | trampoline into retained stock | the configuration derivations tasks 3, 50, 104 and others reach, and the curves at `0x40a6c7ec` tasks 1..72 write | no | the settings and configuration services are owned | required |
| `StockFloatLog`: `0x894778` | trampoline into retained stock | the runtime library | no | an owned logarithm is qualified against it | required |
| `StockKernelDelay`: `0x723ce8` | trampoline into retained stock | the kernel tick delay the shot sequence's adjusted waits enter | no | the kernel crate owns the delay | required |
| `StockLensStatus`: `0x6c6b08` | trampoline into retained stock | the lens status byte at `0x93c544 + 0x64` the lens status query reads | no | the lens status service is owned | required |
| `StockPipelineHardware`: `0xda1b4c`, `0xda1d48` | trampoline into retained stock | the image pipeline driver tasks 11..24 share | no | the image pipeline driver is owned | required |
| `StockFaceTracking`: `0x106d02c`, `0x106a2f0` | trampoline into retained stock | face tracking's shared state, which code outside L11 stores at `0x106d850`, `0x106d990`, `0x106f150` and `0x106f2fc`; the census reaches the two entries from task 43 only | no | face tracking is owned | required |
| `StockScanBlocks` (configuration fields `0xdc`..`0x1ea` with the setters `0xd78400`..`0xd79364` and the defaults `0xd791b0`, applied parts `+0x14c` read, marked and cleared as `0xd5e1b8` clears them, configured window `+0x1c8`, request flags and window `+0x3e`, clear through `0x7221c8`) and `StockScanLibrary` (`0xd6415c`..`0xd663e0`, `0xd79178`, `0xd79194`, and the reads `0xd64840`, `0xd649e8`, `0xd64940`, `0xd64e2c` entered with the configuration block) | guest-memory layout view | the scan library tasks 11..24 share, which reads both blocks | no | the scan library is owned | required |
| `StockStageBlock`: the region block (`0xda027c`), parameter block (`0xd9f7c0`, `0xd9f828`; the L4 work block `0x1699a08`) and geometry block (`0xd9fd54`; `0x16996b8`) by guest address: part offsets, stage mode words `+0x14`/`+0x30`/`+0x40`/`+0x88`, enable keep/invert/set words, path enable `+0x5c`, second gate `+0x254`, mode level `+0x78` | guest-memory layout view | the L4 profile layer, which writes the blocks in stock memory | no | the L4 caller passes typed parts | required |
| `StockCompiledScan` (the scan library's configuration from `+0x101c`: output kind, option bytes `+4`/`+8`) over the scan library's stores `0xd64850`, `0xd649f8`, `0xd64950`, `0xd6537c` | guest-memory layout view | the scan library tasks 11..24 share, which reads the compiled configuration | no | the scan library is owned | required |
| `StockStreamSnapshot`: tag and snapshot block (`stream + 0xbd4e4` for `0xd6b460`), register read `0x693dcc`, packer `0x73ed94` over descriptors `0xd6baac`, stream services `0xd6ae18`, `0xd6afe4`, option words `stream + 0xbd668 + 0x370/0x374` | trampoline into retained stock | the stream snapshot `0xd6b3f8`, which task 43 reaches through the `0xf2cad8` completion of the queue-48 0x4005 request (`0x1039b48`), so its stock body runs in that completion's context | no | the stream services are owned | required |
| `StockDumpDerivations`: `0x7468b8`, `0x74695c`, `0x76f340`, `0x748398` | trampoline into retained stock | the configuration derivations and lens position flags other tasks also reach | no | the settings and configuration services are owned | required |
| `StockDumpBuffer`: the shot dump buffer's configuration section at `+0xb174c` (tags `ELTS`/`ELAL`, step columns, adjustment words, limits, selection words) | guest-memory layout view | ShotSequence's dump buffer, written to the card by stock code | no | the dump buffer is owned and written by owned code | required |
## saclac_imaging

saclac's image-4 ISP units RAID0, MVREC and AUTO, and its capture-pipeline layer (`replacement/saclac/imaging`). The unit registers are hardware; the rows are the stock memory and entries `src/stock.rs` and `src/stock/capture.rs` bind.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `StockIsp` session records at `0x820f30` (MVREC), `0x820f40` (AUTO) and `0x820f88` (RAID0): holder, last operation, active session (0xffff free), next session | guest-memory layout view | retained stock entries of the same units called from unowned image-5 pipeline configuration (`0xd9e88c`, the L8 callers until owned) | no | no retained code calls a unit's stock entries | required |
| ISP event log `0x684908` (client, unit, operation, fault) and `0x684948` (holder, requester, unit, operation, fault) | trampoline into retained stock | `hw_log_event` `0x684988`: console stream 0x2b and the five-entry ring at `0x96ca0c` | no | the ISP driver's event log is owned | required |
| `stock::arguments`: the argument blocks and words of the RAID0 (`0x685208`..`0x688c44`), MVREC (`0x671eec`..`0x67395c`) and AUTO (`0x6768f8`..`0x67b194`) entries, read into the units' typed values; a word an entry refuses is a `ParameterFault` | guest-memory layout view | the stage blocks of image-5 pipeline configuration (`0xd9ea08`, `0xd9f828`, `0xd9fd54`), whose parts `xm1_saclac_config::stock::StockStageBlock` addresses | no | the stage blocks are owned and pass typed values | required |
| `StockCapture` control tables in image 5: readout pairs `0x101e014`, references `0x101de8c` (0x1c bytes), shutter rates `0x10099c8`, decades `0x100a60c`, rate divisors `0x1009988`, scale keys `0x1022f54` | guest-memory layout view | retained stock control code reading the same tables (`0x1006000`, `0x1006360`, `0x1007ce0` and the control band) | no | no retained code reads the tables | required |
| `StockCapture` control state: configured frame lines at `0x16f8668 + 0x8be`, the sample period `0x1006360` | guest-memory layout view | the control subsystem, which writes that state | no | the control subsystem is owned | required |
| `StockCapture` services: adjustment byte `0x69f8fc`, profile kind `0xef5448`, rate threshold `0xef5360`, crop `0xef5470`, load test `0x742054`/`0x742064`, logarithm `0x894778`, reference ratio `0x10034b4`, class gains `0xef3434`, custom gains `0xef4fc8`, forced class `0xef4fd8`, temperature offset `0x772f28` | trampoline into retained stock | the stock adjustment, profile, test-interface, runtime and gain services; `0xef5470` and `0xef4fd8` are saclac's profile layer | no | each service is owned, or for the profile layer, bound to its owned part | required |
| `StockCapture` as `CapturePipeline` and `CaptureResults`: `0xfe90a0`, `0xfe90c0`, `0xff9bb8`, `0xff6f94`, `0xff6fc0`, `0xfef8e8`, `0xfef910`, `0xfef938`, `0xff2c5c`, `0xfec984`, `0xfeda78`..`0xfedaf8`, `0xffcfd0`, `0x10029bc`, `0x1005044`, `0x1028adc`, `0x1054388`, `0x10543ac`, `0x10538a4`, `0x105386c` | trampoline into retained stock | the capture pipeline state the imaging task (47) and capture tasks write (`0x16f08c0`, `0x16f1028`, `0x16f247c`, `0x16f2e50`, `0x16f0f5c`, `0x16f8008`, `0x16f8150`, `0x16f82fc`, `0x16f8f30`), the result state `0x16fec34` and the dispatch and history words at `0x1706a34`/`0x1706a3c` | no | the capture pipeline and result tasks are owned | required |
| `StockCapture` as `PixelGeometry`: shared divisors `0xef5768`, profile divisors `0xef5b5c`, channel pixels `0xef56cc`, aspect `0xef5310` | trampoline into retained stock | saclac's profile layer (L4) | no | bound to the profile layer's owned part | required |
| `stock::statistics`: the imaging pipeline's raw statistics buffer (planes `0x850`.., grid channels at `0x850`, `0xc50`, `0x1050`, `0xc50`, regions `0x1850`, black sums `0x2190`, 0x29b0 bytes), the capture parameters (0x98 bytes), the raw stores `0x5ffb9120`/`0x5ffbe480` and `0x5ffbbad0`/`0x5ffc0e30`, the statistics stores and output grid the callers name, the readout selection's previous-buffer words (+0x24, +0x28) and the caller's dump (+0x9e564.., +0x9ced0..) | guest-memory layout view | the retained readers of those buffers (the registrations at `0xedf558`, the calc library's requests, `0xedb05c`) and the capture steps that pass them | no | the readers and the capture steps are owned and take typed statistics | required |
| `stock::statistics` services: `0xed8af4` (publication defaults), `0xedb05c` (publication), `0xedba78`, `0xedb0ac`, `0xedb9fc`, `0xef743c`/`0xef73e4`, `0x772f28`, `0xee9ab0` | trampoline into retained stock | saclac's profile and storage layers (L4) and the calibration table | no | each is owned, or bound to its owned part | required |
| `StockDetection` and the `stock::detection` call bindings: detector result `0x10451ac` (task 65's `0x4171b7fc` under lock `0xa7`), crop `0xef6030`, centring `0xedb0dc`, the 0x42c-byte result buffer, the 0x18-byte crop request, the callers' area blocks and output words | trampoline into retained stock | the detector task (65), saclac's profile and storage layers (L4), and the callers of `0x10510e8`, `0x10512cc`, `0x10515f4`, `0x10517a0` (`0xf1582c`, `0xf17838`, `0xf17984`) | no | the detector is owned or behind a typed port, L4 is bound to its owned part, and the callers pass typed blocks | required |

## saclac_interface

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| Command, parameter-block and callback descriptors in `src/stock.rs` | guest-memory layout view | retained queue-43 command producers, their 46 shared parameter blocks and 106 completion callbacks | no | queue-43 producers and completion callbacks are owned and no retained consumer reads the parameter blocks | required |
| `lens::LensPayload` with `lens_callback_word`, `lens_payload` and `lens_dc_request` in `src/stock.rs` | guest-memory layout view | retained lens control's queue-50 requests 0x4202 to 0x4212 (senders `0x76dbe4` to `0x770080`) and the retained lens completion callbacks `0xf14ae4`, `0xf14b14`, `0xf14b50`, `0xf14b8c`, `0xf14bc8`, `0xf14be4`, `0xf150c4`, `0xf150f0` and `0xf28e94` | no | lens control and the lens children's completion callbacks are owned | required |
| `stock::peers`: each peer's queue (lensdc 50, imgdc 47, calc 44, calc1 45, statistics 48, flash 10, lenscom 98, image processor 55), the sender byte its stock senders write and the block size each queue was created with (`0x724cdc`) | guest-memory layout view | the retained peer tasks and their queues | no | every peer task saclac posts to is owned | required |
| Frame-classification codec in `src/stock/calc_sub.rs` | guest-memory layout view | retained queue-46 classifier `0xf49eb4` reads eleven inputs and enters the twelfth word as its completion | no | the classifier and its completion consumers use a typed owned interface | required |

## saclac_profile_records

saclac's view of the profile records (`replacement/saclac/profile_records`), shared in stock with tasks 3, 50, 59 and 104.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `StockMemory`, the registry, record and shared-block addresses `0x915d38`, `0x96ebd0` and `0x91904c` in `src/stock/mod.rs`, the layout in `src/stock/layout.rs` and the field catalog in `src/stock/record.rs` and `src/stock/shared.rs` | guest-memory layout view | the stock accessors tasks 3, 50, 59 and 104 reach (0x75a910, 0x75a8c0, 0x75e618, 0x75b054.. and the open, close and initialize entries), which read and write the same objects | no | every task that reaches the records is owned | required |
| the handle word codec `stock::handle` and `stock::record_handle` | guest-memory layout view | handles the stock open (`0x75a63c`) issues and the retained gates decode | no | the registry and its gates are owned | required |
| `Stock`, the `StockEntries` binding: header byte readers `0x74e5c0`, `0x74e648`, `0x74f9a4`, `0x74fa2c`, `0x74fc64`, `0x7503ec`, lens designation `0x758c68`, header replace `0x74e074`, open `0x75a63c` with grant table `0x81aa6c`, close `0x75a6cc`, initialize `0x756720` | trampoline into retained stock | the stock bodies at those addresses, which tasks 50, 59 and 104 also call | no | tasks 50, 59 and 104 no longer reach them | required |

## shot_sequence

The task 43 half-press and live preparation and capture pipeline behind the saclac child contract. Its steps reach peers, records and replies through one `Ports` trait and narrow layer traits that its composition root binds; the crate itself links no stock symbol.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `half_press_mode`, `half_press_notifications` and `abort_status` in `src/stock.rs` | guest-memory layout view | words +4 and +0x10 of the command-6 parameter block written by the retained command function `0xef8fa0`, and the capture status word at `0x169d380` | no | the half-press command producer and the capture status writer are owned | required |
| `MeasurementProfiles::profile_extent`, bound to `0xef5d44` as `src/capture_stock.rs` names it in `MEASUREMENT_PROFILES` | trampoline into retained stock | the profile geometry's retained `0xef5d44`, which tasks 3 and 7 also reach | no | the profile geometry owns `0xef5d44` and `profile_extent` binds to its Rust function | required |

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
| 25 retained `xm1_stock_*`, `xm1_bank_*`, `xm1_storage_*` and `settings_custom_*` externs | trampoline into retained stock | stock bank, header, storage and custom-record services | no | owned storage replaces those services | required |
| header mirror, previous-value snapshot `0x89ec00`, backup memory `0x409c281c` and the 16-byte shadow `0x409c29b8` | guest-memory layout view | retained stock readers and the bank-0 backup contract | no | no stock code reads the mirror or backup | required |

## fixed_image

The fixed-range test image preserves the retained startup phase call while observing entry and initialized storage.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `xm1_fixed_test_entry` in `probe-entry.S`, published at `0x69cdf8` by `ProbeMain.hs` | stock-ABI export | retained startup phase callers | no | the placement probe is replaced by the subsystem image's entries | required |
| `xm1_fixed_test_resume` at `0x69cdfc`, bound by `ProbeMain.hs` | trampoline into retained stock | the retained startup phase body after its entry push | no | the placement probe is replaced by the subsystem image's entries | required |

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

## saclac_profile_geometry

saclac's layer L4 (`replacement/saclac/profile_geometry`): the DCF controller, profile tables, geometry operation, services and record mode of the image-5 band `[0xed7d64, 0xef8c08)`. `saclac_root` owns port binding and linked-image qualification; the proposed bindings are not evidence of execution. The rows below are the crate's own stock boundary, all in `src/stock/`.

| Shim | Kind | Other side | Owned now | Removal condition | State |
| --- | --- | --- | --- | --- | --- |
| `layout.rs` with `memory::Memory`: the state blocks `0x1698a34`..`0x169cefc` (changes, controller, parameters, status, requests, counters, work, selection, policy, dump, diagnostics, display, geometry operation, operation callbacks, record mode), the geometry service state at `0x41717764` and the statistics buffers `0x5ffb0dc0` and `0x5ffb3770` | guest-memory layout view | the band functions no census root reaches, which read and write the blocks (the channel and indexed starters `0xee39c4` and `0xee3dac` with their completions `0xedb25c` and `0xedb3f8`, the latch `0xed8368`, `0xed9040`, `0xed8e24`, `0xedb630`), the geometry service stop `0xeecd94`, and the pipeline and storage services that fill the work, dump and record-mode blocks | no | every band function that reads or writes the blocks is owned or removed | required |
| `records.rs` (parameter record and status snapshot), `statistics.rs` (shot-record summary of `0xedb05c`, destination record of `0xda035c`), `dumps.rs` (the device registers the frame dumps `0xedb554` and `0xedb564` copy, and their dump-buffer offsets), `areas.rs` (the area query and answer of `0x106638c` and the caller's area list) | guest-memory layout view | the controllers and shot callers that copy the records, the shot record's and frame dump's stock readers, the pipeline's `0xda035c` and the geometry service | no | the callers, readers and services are owned and take typed values | required |
| `tables.rs`, `StockTables`: the image-5 read-only tables at `0xef6de4`, `0xef6aa4`, `0xef66d4`, `0xef6dd4`, `0xef6ddc`, `0xee9ef4`, `0xee9d60`, `0xef50c4`, `0xef70b4`, `0xed7e08`, `0xef66d0`, `0xee8664`..`0xee8864`, `0xede9a8`, `0xeef628` | guest-memory layout view | the retained band getters other tasks reach (`0xef5448`, `0xef5190`, `0xef5a68` ..), which read the same tables | no | the band's retained getters are owned | required |
| `StockFloatLog` in `mod.rs`: `0x894778` | trampoline into retained stock | the runtime library | no | an owned logarithm is qualified against it | required |

Retained area-list destinations use `stock::areas::convert_into` for a service answer or `AreaList::write_into` for an owned geometry result. Both take an initialized `[u8; AREA_LIST_BYTES]` destination, retain present-area bytes `+0x27` and `+0x33`, and clear absent records. These are untouched destination bytes, not an undefined-source-read correction. Root bindings must preserve those bytes from the caller's existing destination and publish successful results back to that same buffer; they must not serialize through a fresh zeroed destination or publish a refused query.

## Not registered

`replacement/rtos_layout` and `replacement/rtos_reservation` contain Rust but no firmware artifact target; they are shared host views, not units. `replacement/toolchain` contains build rules only. `replacement/record5_staging`, `startup_artifact`, `startup_composition` and `startup_consumers` contain no Rust or assembly sources; they are host orchestration.
