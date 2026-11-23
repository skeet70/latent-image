# Shim register

[Replacement index](README.md). Generated from owning-unit `shims.json` declarations and `shim-images.json` by `//replacement/shim_check:shim-check`. Edit declaration data, not this presentation.

A removal requires established caller closure, direct owned API use and a qualified comparison of both sides in the same checked image. Ownership alone establishes none of these predicates. Link admission is not removal acceptance. Reference-only families remain required and are not checked production coverage.

## Linked compositions

| Image | Classification | Owning units | Bound families |
| --- | --- | --- | --- |
| `//host/firmware:bank-elf-00c40000` | Production | bank, capture, media_permit, menu_engine, modal_runtime, owned_menu, recipe_ui, save, service | 15 |
| `//host/firmware:bank-elf-00c4fc88` | Production | bank, capture, media_permit, menu_engine, modal_runtime, owned_menu, recipe_ui, save, service | 15 |
| `//host/firmware:bank-modal-elf-00c40000` | Production | bank, capture, media_permit, menu_engine, modal_runtime, owned_menu, recipe_ui, save, service | 18 |
| `//host/firmware:bank-modal-elf-00c4fc88` | Production | bank, capture, media_permit, menu_engine, modal_runtime, owned_menu, recipe_ui, save, service | 18 |
| `//host/firmware:engine-elf-modal75-menu-engine` | Production | bank, capture, kernel, menu_engine, modal_runtime, owned_menu, property_service, recipe_ui, record_store, service, settings_storage | 28 |
| `//host/firmware:engine-elf-modal75-menu-engine-render` | Production | bank, capture, kernel, menu_engine, menu_widgets, modal_runtime, owned_menu, property_service, recipe_ui, record_store, render_adapter, render_commands, service, settings_storage | 37 |
| `//replacement/abi:pilot-image` | Qualification | abi | 3 |
| `//replacement/af_result:af-result` | Production | af_result | 4 |
| `//replacement/application_pump:application-pump-image` | Production | application_pump | 3 |
| `//replacement/calibration_liveview:calibration-liveview` | Production | calibration_liveview | 5 |
| `//replacement/capture:capture-candidate` | Production | capture | 4 |
| `//replacement/completion_delay:completion-delay-image` | Production | completion_delay | 2 |
| `//replacement/critical_pair:critical-pair-image` | Production | critical_pair, interface_initializer, loader_prefix, secondary_stage | 8 |
| `//replacement/dcf_numbering:dcf-numbering-image` | Production | dcf_numbering | 3 |
| `//replacement/develop:develop-candidate` | Production | callback_core, capture, develop, kernel, media_permit, property_service, record_store, save, settings_storage | 27 |
| `//replacement/display_scanout:display-scanout` | Production | display_scanout | 4 |
| `//replacement/display_scanout:display-scanout-panel` | Production | display_scanout | 4 |
| `//replacement/first_stage:first-stage-fixed-image` | Production | first_stage | 7 |
| `//replacement/first_stage:first-stage-image` | Production | first_stage | 7 |
| `//replacement/fixed_image:fixed-bank-image` | Production | display_scanout, file_service | 10 |
| `//replacement/fixed_image:probe-image` | Qualification | fixed_image | 5 |
| `//replacement/general_heap:general-heap-fixed-image` | Production | general_heap | 5 |
| `//replacement/general_heap:general-heap-image` | Production | general_heap | 5 |
| `//replacement/general_heap:general-heap-recipe-image` | Production | general_heap | 5 |
| `//replacement/idle_task:idle-task-image` | Production | idle_task | 2 |
| `//replacement/image5_init:image5-init-image` | Production | image5_init | 3 |
| `//replacement/image5_load:image5-decoder-image` | Qualification | image5_load | 3 |
| `//replacement/image5_load:image5-load-image` | Production | image5_load | 3 |
| `//replacement/interface_initializer:interface-initializer-image` | Production | interface_initializer, loader_prefix, secondary_stage | 6 |
| `//replacement/kernel_install:kernel-install-image` | Production | kernel_install | 3 |
| `//replacement/lens_transport:actor` | Qualification | lens_transport | 1 |
| `//replacement/loader_orchestration:loader-a-orchestration-image` | Production | loader_orchestration | 3 |
| `//replacement/loader_orchestration:loader-b-orchestration-image` | Production | loader_orchestration | 3 |
| `//replacement/loader_prefix:loader-prefix-image` | Production | loader_prefix, secondary_stage | 5 |
| `//replacement/media_permit:media-permit-candidate` | Production | media_permit | 5 |
| `//replacement/peer_wait_control:peer-wait-control-image` | Production | peer_wait_control | 2 |
| `//replacement/plain_load:plain-load-image` | Production | plain_load | 3 |
| `//replacement/playback_actor:playback-actor-image` | Production | playback_actor | 3 |
| `//replacement/power_lifecycle:record-helpers` | Qualification | power_lifecycle | 6 |
| `//replacement/primary_initializer:primary-initializer-image` | Production | primary_initializer | 3 |
| `//replacement/property_service:property-service-image` | Production | kernel, property_service, record_store, settings_storage | 12 |
| `//replacement/property_service:property-service-linked-image` | Production | kernel, property_service, record_store, settings_storage | 10 |
| `//replacement/property_service:property-service-public-image` | Production | kernel, property_service, record_store, settings_storage | 10 |
| `//replacement/record_store_carrier:record-store-image` | Production | record_store, record_store_carrier | 8 |
| `//replacement/root_task:root-task-image` | Production | root_task | 4 |
| `//replacement/rtos_cyclic:app-callback` | Production | rtos_cyclic | 4 |
| `//replacement/rtos_exception:exception-a` | Production | rtos_exception | 3 |
| `//replacement/rtos_exception:exception-b` | Production | rtos_exception | 3 |
| `//replacement/rtos_irq_control:irq-a` | Production | rtos_irq_control | 2 |
| `//replacement/rtos_irq_control:irq-b` | Production | rtos_irq_control | 2 |
| `//replacement/rtos_platform:platform-a` | Production | rtos_cyclic, rtos_data_queue, rtos_fixed_pool, rtos_global_lock, rtos_objects, rtos_own_first, rtos_platform, rtos_scheduler, rtos_semaphore, rtos_task_records, rtos_timer | 31 |
| `//replacement/rtos_platform:platform-b` | Production | rtos_cyclic, rtos_data_queue, rtos_fixed_pool, rtos_global_lock, rtos_objects, rtos_own_first, rtos_platform, rtos_scheduler, rtos_semaphore, rtos_task_records, rtos_timer | 31 |
| `//replacement/saclac:saclac-image` | Production | saclac, saclac_interface | 5 |
| `//replacement/save:save-owned` | Qualification | save | 6 |
| `//replacement/secondary_stage:secondary-stage-image` | Production | secondary_stage | 3 |
| `//replacement/settings_storage:custom-settings-image` | Production | kernel, record_store, settings_storage | 14 |
| `//replacement/settings_storage:custom-settings-published-image` | Production | kernel, record_store, settings_storage | 13 |
| `//replacement/settings_storage:settings-storage-image` | Production | kernel, property_service, record_store, settings_storage | 10 |
| `//replacement/startup_dispatch:startup-dispatch-image` | Production | startup_dispatch | 3 |
| `//replacement/task1_actor:task1-actor-image` | Production | task1_actor | 3 |
| `//replacement/task_constructors:task-constructors-image` | Production | task_constructors | 3 |
| `//replacement/task_storage:task-storage-image` | Production | task_storage | 2 |
| `//replacement/task_submission:task-submission-image` | Production | task_submission | 4 |
| `//replacement/time_services:image` | Production | kernel, time_services | 14 |
| `//replacement/validator:area3-arm` | Production | validator | 1 |
| `//replacement/vendor_protocol:session-native` | Qualification | vendor_protocol | 1 |

## Nonlinked and frozen references

| Artifact | Owning units | Classification requirement |
| --- | --- | --- |
| `//replacement/develop:develop-services` | develop | Native comparison support artifact, not a current production composition. |
| `//replacement/develop:develop-caller-observer` | develop | Native comparison support artifact, not a current production composition. |
| `//replacement/capture:capture-deps-high` | capture | Native comparison support artifact, not a current production composition. |
| `//replacement/capture:capture-deps-low` | capture | Native comparison support artifact, not a current production composition. |
| `//replacement/embassy_trial:embassy-trial` | embassy_trial | Native comparison support artifact, not a current production composition. |
| `//replacement/rtos_own_first:own-first-a` | rtos_own_first | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_own_first:own-first-b` | rtos_own_first | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_data_queue:data-queue-a` | rtos_data_queue | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_data_queue:data-queue-b` | rtos_data_queue | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_cyclic:cyclic-a` | rtos_cyclic | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_cyclic:cyclic-b` | rtos_cyclic | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_fixed_pool:fixed-pool-a` | rtos_fixed_pool | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_fixed_pool:fixed-pool-b` | rtos_fixed_pool | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_objects:objects-family-stage-a` | rtos_objects | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_objects:objects-family-stage-b` | rtos_objects | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_global_lock:global-lock-a` | rtos_global_lock | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_global_lock:global-lock-b` | rtos_global_lock | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_semaphore:semaphore-a` | rtos_semaphore | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_semaphore:semaphore-b` | rtos_semaphore | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_timer:timer-a` | rtos_timer | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_timer:timer-b` | rtos_timer | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_scheduler:scheduler-a` | rtos_scheduler | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_scheduler:scheduler-b` | rtos_scheduler | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_scheduler:scheduler-s2-a` | rtos_scheduler | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_scheduler:scheduler-s2-b` | rtos_scheduler | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_scheduler:s3a-a` | rtos_scheduler | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_scheduler:s3a-b` | rtos_scheduler | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_scheduler:s3b-a` | rtos_scheduler | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_scheduler:s3b-b` | rtos_scheduler | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_scheduler:s3c-a` | rtos_scheduler | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_scheduler:s3c-b` | rtos_scheduler | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_scheduler:s3d-a` | rtos_scheduler | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_scheduler:s3d-b` | rtos_scheduler | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_task_records:task-records-a-native` | rtos_task_records | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/rtos_task_records:task-records-b-native` | rtos_task_records | Frozen standalone function-reference artifact. Current linked coverage uses platform-a and platform-b. |
| `//replacement/bank:bank-modal75-engine-render-value` | menu_value | The archive is declared but no current Bazel image target publishes its ELF. |
| `//replacement/embassy_platform:module` | embassy_platform | Experimental relocatable objects, not a current linked firmware image. |
| `//replacement/lens_transport:codec` | lens_transport | Synthetic native comparison fixture, not a current production composition. |
| `//replacement/menu_widgets:value-picker-case-image` | menu_widgets | Synthetic native comparison fixture, not a current production composition. |
| `//replacement/menu_widgets:value-picker-bank-case-image` | menu_widgets | Synthetic native comparison fixture, not a current production composition. |
| `//replacement/menu_widgets:tree-list-case-image` | menu_widgets | Synthetic native comparison fixture, not a current production composition. |
| `//replacement/menu_widgets:pane-drawer-case-image` | menu_widgets | Synthetic native comparison fixture, not a current production composition. |
| `//replacement/save:save-model-storage` | save | Synthetic native comparison fixture, not a current production composition. |
| `//replacement/save:save-model-wait` | save | Synthetic native comparison fixture, not a current production composition. |
| `//replacement/save:save-model-stage` | save | Synthetic native comparison fixture, not a current production composition. |

## Owned on both sides

These rows are derived from the C-ABI bridge declarations. Each removal predicate remains independent of the ownership column.

| Unit | Family | State | Removal predicate |
| --- | --- | --- | --- |
| application_nand | `xm1_app_nand_*` exports (14 functions in `src/lib.rs`) | required | unestablished |
| application_nand | `xm1_power_resource_route_request` and the other `xm1_power_*` externs in `src/lib.rs` | required | unestablished |
| calibration_liveview | `xm1_calibration_u16`/`xm1_calibration_i16` exports | required | unestablished |
| capture | `rb_capture_*` exports in `src/lib.rs` and `rb_capture_stock.S` | required | unestablished |
| develop | `xm1_pipeline_image_*` exports | required | unestablished |
| develop | `xm1_property_get`/`xm1_property_set` externs | required | unestablished |
| develop | `xm1_save_facade` extern | required | unestablished |
| develop | `rb_capture_*` externs in `src/pipeline` and `develop/src` | required | unestablished |
| display_scanout | `xm1_scanout_*` and `xm1_power_lcd_operation_action` exports (31 families in `src/abi.rs`, panel and formatter modules) | required | unestablished |
| lens_transport | `xm1_power_current_task`, `xm1_power_interrupt_lock`, `xm1_power_interrupt_unlock`, `xm1_power_service_post`, `xm1_power_set_task_flags`, `xm1_power_resource_route_request` externs | required | unestablished |
| modal_runtime | `rb_modal_dispatch` export | required | unestablished |
| power_lifecycle | `xm1_app_nand_*`, `record_store_*`, `xm1_calibration_*`, `xm1_property_*`, `xm1_getter_owned`, `xm1_setter_owned`, `xm1_storage_initialize`, `xm1_pipeline_image_*`, `xm1_task_submit/release`, `rb_modal_dispatch`, `xm1_scanout_*` externs | required | unestablished |
| power_lifecycle | Power leaf C-ABI services: OSAL, resource routing, serial, timer, semaphore and audio service exports | required | unestablished |
| property_service | `xm1_property_get`/`xm1_property_set` exports | required | unestablished |
| record_store | `record_store_*` exports | required | unestablished |
| rtos_cyclic | `xm1_rtos_cyclic_timer_insert`/`_remove` externs | required | unestablished |
| rtos_data_queue | `xm1_rtos_scheduler_*` externs (acquire, insert, notify, prepare, release, remove, reset, save) | required | unestablished |
| rtos_data_queue | `xm1_rtos_data_queue_stock_insert_timer`/`_remove_timer` externs | required | unestablished |
| rtos_fixed_pool | `xm1_fixed_pool_stock_acquire`, `_release`, `_notify`, `_dispatch` externs | required | unestablished |
| rtos_fixed_pool | `xm1_fixed_pool_stock_wait_release`, `_wait_cancel`, `_wait_enqueue`, `_wait_enqueue_timed` externs at A `0x44568`/`0x44744`/`0x44800`/`0x448e4` (B `0xd4528`/`0xd4704`/`0xd47c0`/`0xd48a4`) | required | unestablished |
| rtos_objects | `xm1_rtos_scheduler_*` externs (acquire, insert, notify, prepare, release, remove, reset, save) | required | unestablished |
| rtos_objects | `xm1_rtos_objects_stock_insert_timer`/`_remove_timer` externs | required | unestablished |
| rtos_objects | `xm1_rtos_objects_stock_timeout` (`0x44318`/`0xd42d8`) | required | unestablished |
| rtos_platform | 49 `kind: owned` externals per domain in `carriers.json` (scheduler/timer calls into other RTOS family units, plus the event-flag wait helpers) | required | unestablished |
| rtos_scheduler | `s3a_stock_*`, `s3b_*`, `s3c_*`, `s3d_*` externs marked `kind: owned` in `carriers.json` (insert, remove, notify, switch, timer insert/remove, and `s3b_release_wait` at `0x44568`/`0xd4528` bound to the event-flag family release carrier) | required | unestablished |
| rtos_semaphore | `xm1_semaphore_stock_acquire`, `_release`, `_notify`, `_dispatch` externs | required | unestablished |
| rtos_semaphore | `xm1_semaphore_stock_wait_release`, `_wait_cancel`, `_wait_enqueue`, `_wait_enqueue_timed` externs at A `0x44568`/`0x44744`/`0x44800`/`0x448e4` (B `0xd4528`/`0xd4704`/`0xd47c0`/`0xd48a4`) | required | unestablished |
| save | `xm1_save_facade` export | required | unestablished |
| settings_storage | `record_store_append`/`record_store_register` externs | required | unestablished |
| settings_storage | `xm1_storage_output8`/`_output16`/`_status32` externs | required | unestablished |
| settings_storage | `xm1_getter_owned`, `xm1_setter_owned`, `xm1_storage_initialize` exports | required | unestablished |
| task_submission | `xm1_task_submit`/`xm1_task_release` exports | required | unestablished |
| vendor_protocol | `xm1_app_nand_*`, `xm1_power_*`, `xm1_scanout_*`, `xm1_rtos_own_first_slot_*` externs (owned units) | required | unestablished |

## abi

The bounded ARM ABI pilot. Its exports exist for the qualification harness, not for production firmware.

Classification: `linked`. [Declarations](abi/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_pilot_*` probe exports and `xm1_abi_probe` in `pilot.rs`/`pilot-entry.S` | stock-ABI export | the ABI qualification harness and `pilot.ld`, not stock firmware | no | the pilot is superseded and no harness entry remains | required | unestablished | `//replacement/abi:pilot-image` (Export, 12 symbols, 0 definite references) |
| `xm1_panic_loop` retained loop in `pilot-entry.S` | retained stock seam in assembly | stock panic behaviour used by the pilot | no | the pilot is retired | required | unestablished | `//replacement/abi:pilot-image` (RetainedEntry, 1 symbols, 0 definite references) |
| Additional linked unmangled ABI entry points | stock-ABI export | the publication or qualification consumers of the current linked image | partly | all consumers use owned typed interfaces and both sides have a qualified comparison in the same image | required | unestablished | `//replacement/abi:pilot-image` (Export, 1 symbols, 0 definite references) |

## af_result

The autofocus result pool, published at the five stock entry addresses in `af-result.ld`.

Classification: `linked`. [Declarations](af_result/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_af_select_adapter`, `xm1_af_rotate`, `xm1_af_reserve`, `xm1_af_release` and the secondary consumer in `src/abi.rs`, placed by `af-result.ld` at `0x103f190`, `0x103f1fc`, `0x103fb50`, `0x103fd38`, `0x103fd88` | stock-ABI export | stock AF callers and pointer tables that enter the five addresses | no | every AF result caller is owned or migrated | required | unestablished | `//replacement/af_result:af-result` (Export, 5 symbols, 0 definite references) |
| `xm1_af_select` entry veneer in `entry.S` | veneer or carrier at a stock address | the stock selector entry | no | the selector is called directly | required | unestablished | `//replacement/af_result:af-result` (Publication, 1 symbols, 0 definite references) |
| `xm1_af_critical_enter`, `xm1_af_critical_leave`, `xm1_af_secondary_mode` retained services | trampoline into retained stock | stock RTOS critical-section and secondary-core services | no | the RTOS platform and secondary-core state are owned | required | unestablished | `//replacement/af_result:af-result` (Import, 3 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/af_result:af-result` (Publication, 5 symbols, 0 definite references) |

## application_nand

The NAND controller and profile-map implementation. Its exports are consumed by owned units; the raw device shape is the remaining seam.

Classification: `reference-only`. [Declarations](application_nand/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_app_nand_*` exports (14 functions in `src/lib.rs`) | C-ABI bridge between two owned units | power_lifecycle and vendor_protocol | yes | both callers link the NAND crate directly and both sides compare with stock in one image | required | unestablished | reference-only: The registered mechanism remains required; this unit has no declared produced linked ELF carrying this interface. `xm1_app_nand_*` exports (14 functions in `src/lib.rs`) |
| NAND register and profile-map layout (`NAND = 0xfff50000`, profile base `0x380`, `src/physical.rs`, `src/memory.rs`) | guest-memory layout view | the stock raw-NAND clients and the boot-time map that the controller must reproduce | partly | every stock raw-NAND client uses the owned flash layer and the map is initialized by owned startup | required | unestablished | reference-only: The registered mechanism remains required; this unit has no declared produced linked ELF carrying this interface. NAND register and profile-map layout (`NAND = 0xfff50000`, profile base `0x380`, `src/physical.rs`, `src/memory.rs`) |
| `xm1_power_resource_route_request` and the other `xm1_power_*` externs in `src/lib.rs` | C-ABI bridge between two owned units | power_lifecycle leaf | yes | the leaf is linked directly and both sides compare with stock in one image | required | unestablished | reference-only: The registered mechanism remains required; this unit has no declared produced linked ELF carrying this interface. `xm1_power_resource_route_request` and the other `xm1_power_*` externs in `src/lib.rs` |

## application_pump

The application-phase pump hooks, staged at the stock prehook, normalizer and posthook addresses.

Classification: `linked`. [Declarations](application_pump/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_application_prehook`, `xm1_application_normalizer`, `xm1_application_posthook` in `entry.S` | veneer or carrier at a stock address | the stock application loop at `0xbfb1b8`, `0xbfb20c`, `0xbfb1e0` | no | the owned task1 actor calls the pump directly | required | unestablished | `//replacement/application_pump:application-pump-image` (Publication, 3 symbols, 0 definite references); `//replacement/application_pump:application-pump-image` (Export, 1 symbols, 0 definite references) |
| the same three stock hooks as `extern "C"` retained services in `src.rs` | trampoline into retained stock | the stock application loop | no | the application loop is owned | required | unestablished | `//replacement/application_pump:application-pump-image` (RetainedEntry, 3 symbols, 0 definite references); `//replacement/application_pump:application-pump-image` (Import, 3 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/application_pump:application-pump-image` (Publication, 2 symbols, 0 definite references) |

## bank

The RAM-push package crate root and its publication at stock menu hooks. It composes owned modules and declares the stock-call trampolines the package still needs.

Classification: `module`. [Declarations](bank/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 54 `RB_TRAMPOLINE` declarations in `menu/recipe_bank_arm.S` (stage open, transition, redraw, postlude, key left/right, modal query, name drawing, lifecycle helpers) | trampoline into retained stock | the stock menu/application services the package calls | no | the engine, renderer and controller units own every called service | required | unestablished | `//host/firmware:bank-elf-00c40000` (RetainedEntry, 53 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (RetainedEntry, 53 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (RetainedEntry, 53 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (RetainedEntry, 53 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (RetainedEntry, 53 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (RetainedEntry, 53 symbols, 0 definite references); `//host/firmware:bank-elf-00c40000` (Export, 7 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (Export, 7 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (Export, 7 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (Export, 7 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Export, 7 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Export, 7 symbols, 0 definite references); `//host/firmware:bank-elf-00c40000` (Import, 28 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (Import, 28 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (Import, 28 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (Import, 28 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Import, 46 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Import, 50 symbols, 0 definite references) |
| 130 `rb_*` entries in `menu/recipe_bank_allowlist.json` published at stock hook addresses | stock-ABI export | stock callers that enter the package through the allowlisted hook addresses | no | every stock caller enters an owned hook or the owned engine directly | required | unestablished | `//host/firmware:bank-elf-00c40000` (Publication, 1 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (Publication, 1 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (Publication, 1 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (Publication, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Publication, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Publication, 1 symbols, 0 definite references); `//host/firmware:bank-elf-00c40000` (Export, 75 symbols, 0 definite references); `//host/firmware:bank-elf-00c40000` (Layout, 4 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (Export, 75 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (Layout, 4 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (Export, 75 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (Layout, 4 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (Export, 75 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (Layout, 4 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Layout, 4 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Export, 75 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Layout, 4 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Export, 75 symbols, 0 definite references) |
| `bank`/`bank-modal`/`bank-lifecycle`/`bank-modal-lifecycle` scoped publication in `BUILD.bazel` | stock-ABI export | the stock bank loader and RAM-push installer | no | the owned installer publishes the package and no stock loader remains | required | unestablished | `//host/firmware:bank-elf-00c40000` (Publication, 1 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (Publication, 1 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (Publication, 1 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (Publication, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Publication, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Publication, 1 symbols, 0 definite references); `//host/firmware:bank-elf-00c40000` (Export, 16 symbols, 0 definite references); `//host/firmware:bank-elf-00c40000` (Layout, 2 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (Export, 16 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (Layout, 2 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (Export, 16 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (Layout, 2 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (Export, 16 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (Layout, 2 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Export, 16 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Layout, 2 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Export, 16 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Layout, 2 symbols, 0 definite references) |
| Definite retained references in the current linked instruction and relocation metadata | retained stock seam in assembly | retained code and guest address contracts reached by the composed image | partly | all declared retained targets have owned typed replacements and both sides are compared in the same image | required | unestablished | `//host/firmware:bank-elf-00c40000` (RetainedEntry, 0 symbols, 89 definite references); `//host/firmware:bank-elf-00c4fc88` (RetainedEntry, 0 symbols, 89 definite references); `//host/firmware:bank-modal-elf-00c40000` (RetainedEntry, 0 symbols, 90 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (RetainedEntry, 0 symbols, 90 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (RetainedEntry, 0 symbols, 123 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (RetainedEntry, 0 symbols, 167 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//host/firmware:bank-elf-00c40000` (Publication, 18 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (Publication, 18 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (Publication, 18 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (Publication, 18 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Publication, 22 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Publication, 22 symbols, 0 definite references) |

## calibration_core

A crate root that re-exports `develop/src/calibration_core` for power_lifecycle and vendor_protocol. It presents no stock shape of its own; the compatibility rows live in `develop`.

Classification: `module`. [Declarations](calibration_core/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |

## calibration_liveview

The calibration cache consumer and live-view publication. Its exports sit at the stock cache/publication entries; its retained trampolines enter stock calibration services.

Classification: `linked`. [Declarations](calibration_liveview/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 19 `xm1_calibration_*` and `xm1_liveview_*` exports in `src/abi.rs` and `src/publication.rs`, placed by `calibration-liveview.ld` | stock-ABI export | stock calibration and live-view callers | no | every calibration and live-view caller is owned or migrated | required | unestablished | `//replacement/calibration_liveview:calibration-liveview` (Export, 19 symbols, 0 definite references) |
| `retained_primary`, `retained_backup`, `retained_fixed`, `retained_copy`, `retained_wb_convert`, `retained_wb_publish`, `retained_halfword`, `retained_lock`, `retained_rebuild` in `entry.S` | trampoline into retained stock | stock calibration storage, white-balance and rebuild services | no | the settings/calibration services and white-balance path are owned | required | unestablished | `//replacement/calibration_liveview:calibration-liveview` (RetainedEntry, 9 symbols, 0 definite references) |
| fixed calibration cache at `0x41acd800` and the 504-byte defaults resource | guest-memory layout view | retained calibration consumers that read the same cache | no | every cache consumer is owned | required | unestablished | `//replacement/calibration_liveview:calibration-liveview` (Layout, 1 symbols, 0 definite references) |
| `xm1_calibration_u16`/`xm1_calibration_i16` exports | C-ABI bridge between two owned units | power_lifecycle | yes | power links the crate directly and both sides compare with stock in one image | required | unestablished | `//replacement/calibration_liveview:calibration-liveview` (Export, 2 symbols, 0 definite references) |
| Definite retained references in the current linked instruction and relocation metadata | retained stock seam in assembly | retained code and guest address contracts reached by the composed image | partly | all declared retained targets have owned typed replacements and both sides are compared in the same image | required | unestablished | `//replacement/calibration_liveview:calibration-liveview` (RetainedEntry, 0 symbols, 9 definite references) |

## callback_core

The shared admission table, shape-typed registry and kernel-slot reader. The Power services leaf and the develop unit both compile it; the leaf re-exports it with the Power interrupt lock, develop with its own, and neither keeps a private extents table or kernel-slot reader.

Classification: `module`. [Declarations](callback_core/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `CallbackRegistry` in `src/lib.rs` and the nine image text extents in `IMAGES`/`EXCLUDED`/`ADMITTED` | callback registry for guest words | stock image 4, 5 and 6 callback words and the owned registrants in the Power leaf, develop, display_scanout, application_nand and vendor_protocol | no | every stock callback word is replaced by an owned entry with a typed Rust signature | required | unestablished | `//replacement/develop:develop-candidate` (RetainedEntry, 1 symbols, 0 definite references) |
| `KernelSlot::read` and `kernel_slot` in `src/lib.rs` | kernel interface slot | the kernel interface block at 0x96bd20 read by the Power leaf and develop | no | the kernel interface is owned and entered through typed Rust functions | required | unestablished | `//replacement/develop:develop-candidate` (Layout, 1 symbols, 0 definite references) |

## capture

The develop-step completion, cancellation and scheduler-release body. It exports its boundary to develop and trampolines into retained capture services.

Classification: `linked`. [Declarations](capture/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `rb_capture_*` exports in `src/lib.rs` and `rb_capture_stock.S` | C-ABI bridge between two owned units | develop | yes | develop calls the capture crate directly and both sides compare with stock in one image | required | unestablished | `//host/firmware:bank-elf-00c40000` (Export, 11 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (Export, 11 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (Export, 11 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (Export, 11 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Export, 11 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Export, 11 symbols, 0 definite references); `//replacement/capture:capture-candidate` (Export, 11 symbols, 0 definite references); `//replacement/develop:develop-candidate` (Export, 9 symbols, 0 definite references) |
| `rb_capture_stock_advance`, `rb_capture_stock_check`, `rb_capture_stock_log`, `rb_capture_stock_release` and the wait/step-plan trampolines in `rb_capture_stock.S` | trampoline into retained stock | stock capture/develop services at the image-5 boundary | no | the capture service and its scheduler events are owned | required | unestablished | `//host/firmware:bank-elf-00c40000` (RetainedEntry, 4 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (RetainedEntry, 4 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (RetainedEntry, 4 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (RetainedEntry, 4 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (RetainedEntry, 4 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (RetainedEntry, 4 symbols, 0 definite references); `//replacement/capture:capture-candidate` (RetainedEntry, 4 symbols, 0 definite references); `//replacement/develop:develop-candidate` (RetainedEntry, 4 symbols, 0 definite references) |
| `capture.ld`, `high.ld`, `low.ld` placements and the `deps/` low/high fault variants | veneer or carrier at a stock address | stock callers of the capture step and the qualification controls | no | the capture entry is called directly | required | unestablished | `//host/firmware:bank-elf-00c40000` (Publication, 1 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (Publication, 1 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (Publication, 1 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (Publication, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Publication, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Publication, 1 symbols, 0 definite references); `//replacement/capture:capture-candidate` (Publication, 1 symbols, 0 definite references) |
| Definite retained references in the current linked instruction and relocation metadata | retained stock seam in assembly | retained code and guest address contracts reached by the composed image | partly | all declared retained targets have owned typed replacements and both sides are compared in the same image | required | unestablished | `//replacement/capture:capture-candidate` (RetainedEntry, 0 symbols, 7 definite references) |

## completion_delay

A loader-A completion-delay carrier entered by a patched `bl` and returning to the original site.

Classification: `linked`. [Declarations](completion_delay/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_completion_delay_entry`, `_end`, `_release` in `entry.S`, placed by `completion-delay.ld` | veneer or carrier at a stock address | the stock loader-A instruction at `0x40a00` that branches into the carrier | no | loader-A is owned and no patched branch remains | required | unestablished | `//replacement/completion_delay:completion-delay-image` (Publication, 1 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/completion_delay:completion-delay-image` (Publication, 2 symbols, 0 definite references) |

## critical_pair

Task103 critical-section boundaries over retained loader services.

Classification: `linked`. [Declarations](critical_pair/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_critical_enter_adapter`, `xm1_critical_exit_adapter` and `xm1_critical_enter_continue`/`xm1_critical_exit` in `entry.S`/`src.rs` | stock-ABI export | stock task103 callers | no | the task103 critical pair is called from owned code only | required | unestablished | `//replacement/critical_pair:critical-pair-image` (Export, 4 symbols, 0 definite references) |
| `xm1_critical_retained_call` in `entry.S` | trampoline into retained stock | the retained loader critical-section service | no | the loader service is owned | required | unestablished | `//replacement/critical_pair:critical-pair-image` (RetainedEntry, 1 symbols, 0 definite references); `//replacement/critical_pair:critical-pair-image` (Import, 52 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/critical_pair:critical-pair-image` (Publication, 5 symbols, 0 definite references) |

## dcf_numbering

The DCF numbering carrier ELF: entry veneers at the stock image-4 entries and import stubs.

Classification: `linked`. [Declarations](dcf_numbering/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `dcf_entry_*` in `entry.S` and `dcf-numbering.ld` | veneer or carrier at a stock address | stock callers of the ten DCF numbering entries | no | every caller enters the owned numbering directly | required | unestablished | `//replacement/dcf_numbering:dcf-numbering-image` (Publication, 10 symbols, 0 definite references) |
| Owned bodies behind the entry veneers in `dcf_numbering/src/stock.rs` | stock-ABI export | the veneers at the ten stock entries and the stock argument and return conventions of their callers | partly | every caller uses a typed owned numbering API | required | unestablished | `//replacement/dcf_numbering:dcf-numbering-image` (Export, 10 symbols, 0 definite references) |
| `imports.ld` import stubs | trampoline into retained stock | the retained media, profile, formatting and directory services | no | the owned file service, settings profile and filesystem replace them | required | unestablished | `//replacement/dcf_numbering:dcf-numbering-image` (Import, 21 symbols, 0 definite references) |

## develop

The development pipeline, including `font_core` and `calibration_core`. It is the largest unit: owned pipeline bodies call retained stock callees and the retained font/calibration data views, and 43 `_stock.S` files hold stock-shaped seams.

Classification: `linked`. [Declarations](develop/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_pipeline_image_*` exports | C-ABI bridge between two owned units | power_lifecycle | yes | power links the pipeline directly and both sides compare with stock in one image | required | unestablished | `//replacement/develop:develop-candidate` (Export, 1159 symbols, 0 definite references) |
| `xm1_property_get`/`xm1_property_set` externs | C-ABI bridge between two owned units | property_service | yes | develop links the property service directly and both sides compare with stock in one image | required | unestablished | `//replacement/develop:develop-candidate` (Export, 1 symbols, 0 definite references) |
| `xm1_save_facade` extern | C-ABI bridge between two owned units | save | yes | develop links the save facade directly and both sides compare with stock in one image | required | unestablished | `//replacement/develop:develop-candidate` (Export, 1 symbols, 0 definite references) |
| `rb_capture_*` externs in `src/pipeline` and `develop/src` | C-ABI bridge between two owned units | capture | yes | develop links the capture crate directly and both sides compare with stock in one image | required | unestablished | `//replacement/develop:develop-candidate` (RetainedEntry, 1 symbols, 0 definite references) |
| Shared callback registry and kernel-slot reader in `callback_core`, re-exported by `src/pipeline/callbacks.rs` | callback registry for guest words | stock callback words and the kernel interface block | no | every caller uses typed callbacks and the kernel interface is owned | required | unestablished | `//replacement/develop:develop-candidate` (RetainedEntry, 1 symbols, 0 definite references) |
| `xm1_kernel_console9`, `xm1_kernel_critical_enter` and `xm1_kernel_critical_exit` in `develop.ld` | trampoline into retained stock | loader-A console writer 0x41a34 and cross-core critical pair 0x7235d8/0x7236b0 through the linked property service's kernel backend | no | develop's property service uses owned console and cross-core services | required | unestablished | `//replacement/develop:develop-candidate` (Import, 55 symbols, 0 definite references) |
| 1,610 retained `extern "C"` declarations in `src/` (image conversion, blend, finish, chroma, sensor calibration, configuration, diagnostics, tracing) | trampoline into retained stock | stock develop services and device adapters | no | the develop service, sensor/calibration and device paths are owned | required | unestablished | `//replacement/develop:develop-candidate` (RetainedEntry, 1 symbols, 0 definite references) |
| 43 `*_stock.S` files (`Buffers_stock.S`, `FontOutlineVm_stock.S`, `RedEye_stock.S`, `WarpEngine_stock.S`, and the rest) | retained stock seam in assembly | stock develop bodies kept for comparison and for callers not yet owned | no | the corresponding owned body is qualified and every caller is migrated | required | unestablished | `//replacement/develop:develop-candidate` (RetainedEntry, 1 symbols, 0 definite references) |
| `font_core` asset and glyph layout (`src/font_core`, `FontFdl_stock.S`, `FontBuiltin_stock.S`, `FontScripts_stock.S`) | guest-memory layout view | retained stock font resources and text consumers | no | owned assets and every text consumer are owned | required | unestablished | `//replacement/develop:develop-candidate` (Layout, 1 symbols, 0 definite references) |
| `calibration_core` configuration/curve layout (`src/calibration_core`) | guest-memory layout view | retained calibration data and power/vendor consumers | partly | the calibration image and every consumer are owned | required | unestablished | `//replacement/develop:develop-candidate` (Layout, 1 symbols, 0 definite references) |
| `develop.ld`, `services.ld` and `caller_observer.ld` placements | veneer or carrier at a stock address | stock callers of develop stages | no | owned code is reached directly | required | unestablished | `//replacement/develop:develop-candidate` (Publication, 1 symbols, 0 definite references) |
| Additional linked unmangled ABI entry points | stock-ABI export | the publication or qualification consumers of the current linked image | partly | all consumers use owned typed interfaces and both sides have a qualified comparison in the same image | required | unestablished | `//replacement/develop:develop-candidate` (Export, 549 symbols, 0 definite references) |
| Definite retained references in the current linked instruction and relocation metadata | retained stock seam in assembly | retained code and guest address contracts reached by the composed image | partly | all declared retained targets have owned typed replacements and both sides are compared in the same image | required | unestablished | `//replacement/develop:develop-candidate` (RetainedEntry, 0 symbols, 122 definite references) |

## display_scanout

The display scanout CPU driver, including the panel component. Its exports are at stock entry addresses and its S7-S9 seams are defined in [the target design](../reports/display-render-target-design.md#25-shims) and [the scanout contract](../reports/display-render-contracts/scanout.md).

Classification: `linked`. [Declarations](display_scanout/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_scanout_*` and `xm1_power_lcd_operation_action` exports (31 families in `src/abi.rs`, panel and formatter modules) | C-ABI bridge between two owned units | power_lifecycle and vendor_protocol | yes | both callers link the scanout crate directly and both sides compare with stock in one image | required | unestablished | `//replacement/display_scanout:display-scanout` (Export, 18 symbols, 0 definite references); `//replacement/display_scanout:display-scanout-panel` (Export, 1 symbols, 0 definite references); `//replacement/display_scanout:display-scanout-panel` (Export, 45 symbols, 0 definite references); `//replacement/fixed_image:fixed-bank-image` (Export, 12 symbols, 0 definite references) |
| S7 stock publication below the owned renderer (`display_set_item_configuration`, `display_queue_secondary_callbacks`) | trampoline into retained stock | stock VGLIB front/back buffer publication | no | the owned scanout driver publishes frames | required | unestablished | `//replacement/display_scanout:display-scanout` (RetainedEntry, 1 symbols, 0 definite references); `//replacement/display_scanout:display-scanout-panel` (RetainedEntry, 1 symbols, 0 definite references); `//replacement/display_scanout:display-scanout` (Import, 5 symbols, 0 definite references); `//replacement/display_scanout:display-scanout-panel` (Import, 9 symbols, 0 definite references); `//replacement/fixed_image:fixed-bank-image` (Import, 5 symbols, 0 definite references) |
| S8 stock per-frame scheduler (`FUN_006c7318` registration) | callback registry for guest words | retained VGLIB one-shots and live-view clients | no | no retained client registers, or the owned driver serves the same request contract | required | unestablished | `//replacement/display_scanout:display-scanout` (RetainedEntry, 1 symbols, 0 definite references); `//replacement/display_scanout:display-scanout-panel` (RetainedEntry, 1 symbols, 0 definite references); `//replacement/fixed_image:fixed-bank-image` (Export, 1 symbols, 0 definite references) |
| S9 stock panel bring-up (timing, formatter and reset sequences) | retained stock seam in assembly | retained callers and ungrounded L1-L5 device laws | no | timing, formatter and reset laws are grounded, caller migration is complete and owned bring-up passes | required | unestablished | reference-only: No selected linked artifact emits the registered entry symbols xm1_scanout_bootstrap. The family remains required; production binding and caller closure are not established. |
| `display-scanout.ld` and `display-scanout-panel.ld` placements, plus the `scanout_*` critical/IRQ/yield externs | veneer or carrier at a stock address | stock display callers and the retained IRQ/queue services | no | the display entry is called directly and the IRQ/queue services are owned | required | unestablished | `//replacement/display_scanout:display-scanout` (Import, 5 symbols, 0 definite references); `//replacement/display_scanout:display-scanout-panel` (Import, 5 symbols, 0 definite references) |
| Twelve A32 branches published at the stock entries `0x6c6eb0`, `0x6c70e4`, `0x6c7318`, `0x6c926c`, `0x6c9cf4`, `0x6c9d44`, `0x6c9e7c`, `0x6c9f70`, `0x6ca3e0`, `0x6cb0c4`, `0x6cb0d8`, `0x6cb8b4` into the fixed-bank exports | veneer or carrier at a stock address | retained display callers, the interrupt dispatcher and registered callback words that name stock entry addresses | partly | every caller and registered callback word names the owned export, and the fixed bank is composed by Root's bank root | required | unestablished | `//replacement/fixed_image:fixed-bank-image` (Export, 12 symbols, 0 definite references); `//replacement/fixed_image:fixed-bank-image` (Publication, 4 symbols, 0 definite references) |

## embassy_platform

The Embassy phase-1 experiment and controls are relocatable objects consumed by experiment composition, not standalone linked firmware ELFs.

Classification: `reference-only`. [Declarations](embassy_platform/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Embassy dispatch hook, demo record and executor-stack publication | callback registry for guest words | the phase-1 experiment dispatcher and retained kernel wake path | no | the owned runtime is linked and entered through typed dispatch without the experiment hook | required | unestablished | reference-only: The demonstration and controls produce .o files rooted at embassy_on_dispatch, EMBASSY_DEMO and EMBASSY_STACKS; no standalone ELF is declared. |

## embassy_trial

A trial Embassy executor image. It presents no stock shape; the trial exists to qualify a runtime base.

Classification: `module`. [Declarations](embassy_trial/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |

## file_service

`file_service` owns the image-4 storage read and write paths: tag allocation and the handle map, the open, read, write and close posters and their ten caller entries, result-slot helpers, post, status translation, media readiness, mediamsg routing with deferral and replay, the open interceptor, the media task and all 34 request workers of the worker table. The bodies link through the bank root into the fixed image behind branch veneers at the 63 stock entries.

Classification: `linked`. [Declarations](file_service/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Branch veneers at the 63 retired image-4 entries in `file_service/src/stock_requests.rs` | veneer or carrier at a stock address | retained image-4 callers of the stock tag, map, poster, result-slot, post, translation, readiness, media-task, mediamsg and open-interceptor entries, the 34 worker-table words 0x814744..0x81484c, and the retained `bl` at 0x715c30, 0x715cd4 and 0x71a998 into the 0x2c14, 0x2c13 and 0x2c0e workers | partly | no retained branch, literal or table word enters a retired extent | required | unestablished | `//replacement/fixed_image:fixed-bank-image` (Export, 63 symbols, 0 definite references) |
| Owned bodies behind the entry veneers in `file_service/src/stock.rs`, `file_service/src/stock_requests.rs`, `file_service/src/stock_workers.rs` and the worker-group boundaries `file_service/src/stock_catalog.rs`, `stock_counts.rs`, `stock_media_io.rs`, `stock_paths.rs`, `stock_search.rs`, `stock_selection.rs` and `stock_volume.rs` | stock-ABI export | the veneers at the 63 stock entries and the stock argument and return conventions of their callers | partly | every caller uses the typed owned file-service API | required | unestablished | `//replacement/fixed_image:fixed-bank-image` (Export, 63 symbols, 0 definite references) |
| Retained kernel, media, copy, path, catalog, volume and filesystem services in `file_service/imports.ld` and `file_service/src/stock.rs` | trampoline into retained stock | semaphore wait 0x724b68 and signal 0x724c00, cache claim 0x71e960 and release 0x71e9ec, current task 0x723cb8, local record 0x7111c4, position 0x7111e0 and open 0x711200, local size 0x6aa4ec and read 0x6aa52c, device lookup 0x70ec84, strcpy 0x70e748, memcpy 0x722054, copy 0x70e6c4, fill 0x70e664, byte order 0x70e650, text difference 0x70e884, divide 0x4d978, media report 0x726c74, send 0x72503c, request receive 0x71fe2c, sleep 0x7240c8, wake 0x724144, tick read 0x7257a8; path services 0x7160b8, 0x715f0c, 0x71543c, 0x712dac, 0x716184, 0x71d1b8 and 0x71d214; extension, page and catalog services 0x713264, 0x713360, 0x713488, 0x713508, 0x7131a0, 0x713d60, 0x713db8, 0x7153a8, 0x7158a8, 0x715918, 0x715f80, 0x717a24 with visitor 0x712a4c, 0x7189b4, 0x719de0, 0x71a93c, 0x712b4c, 0x714940 and 0x713ef4; delete and attribute requests 0x71df30, 0x71e08c and 0x71e288; volume services 0x71f418, 0x71f4b8, 0x71f4d8, 0x71f4f8, 0x71f7dc, 0x71fa9c, 0x71fcbc, 0x71e100, 0x71e1bc, 0x6ed770, 0x6f52e4, 0x6f5330, 0x6f535c and 0x6fae80; media services 0x701090, 0x7012a0, 0x7012c0, 0x701e18, 0x700cd8, 0x700d20 and the pattern pass 0x715b34; filesystem open 0x701634, read 0x6feeec, write 0x6ff000, seek 0x6ff138, close 0x6ffff4, named close 0x6ffdcc, entry close 0x6ffb10, release 0x6ffc68, delete 0x6ff84c and searches 0x7004b0, 0x700768, 0x7008a4 and 0x700b10; written-file registration 0x71565c, post-close 0x71141c, rescan 0x71985c and media refresh 0x71d66c | no | owned kernel, media, copy, path and filesystem services replace every retained service the file service calls | required | unestablished | `//replacement/fixed_image:fixed-bank-image` (Import, 91 symbols, 0 definite references) |
| Stock file-service memory in `file_service/src/stock.rs`, `file_service/src/stock_requests.rs`, `file_service/src/stock_workers.rs` and the worker-group boundaries | guest-memory layout view | handle map 0x910140, tag table 0x9101c0, sequence 0x910200, result owner 0x910134 and slot 0x910138, media context 0x910204 and its device, cancellation, selection, flagged-close, directory-record, catalog, count, window and capacity fields, attach state 0x910208, local-mode record 0x8fc8a4, mediamsg session 0x910218, open set 0x910290, deferred requests 0x9102e0, media busy 0x940ac0 and media ticks 0x96bfb8 | partly | no retained code reads or writes these words | required | unestablished | `//replacement/fixed_image:fixed-bank-image` (Layout, 45 symbols, 0 definite references) |
| Computed calls to the owned request workers, caller callbacks and the catalog window visitor in `file_service/src/stock_requests.rs`, `file_service/src/stock_workers.rs` and the worker-group boundaries | trampoline into retained stock | the 34 request workers through the worker-table words in `routing::WORKERS`, which hold their stock entries and so their veneers, caller-supplied completion callbacks, the window visitor 0x712a4c handed to 0x717a24 and the open interceptor word 0x7217f0 | partly | the media task calls the owned workers directly and every completion callback is owned | required | unestablished | `//replacement/fixed_image:fixed-bank-image` (RetainedEntry, 37 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/fixed_image:fixed-bank-image` (Publication, 4 symbols, 0 definite references) |

## first_stage

The owned first-record stage, entered at the stock record slot and presenting stock's record layout, media controller, handoff words and loader entry.

Classification: `linked`. [Declarations](first_stage/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_first_stage_entry`, `xm1_first_stage_contract` and `first-stage.ld` placing the image at `0x6000` in the first record's slot | veneer or carrier at a stock address | the stock boot ROM's mode-4 handoff and the record-1 consumers that enter the slot | no | the boot ROM and every record-1 consumer are owned | required | unestablished | `//replacement/first_stage:first-stage-fixed-image` (Publication, 2 symbols, 0 definite references); `//replacement/first_stage:first-stage-image` (Publication, 2 symbols, 0 definite references); `//replacement/first_stage:first-stage-fixed-image` (Export, 6 symbols, 0 definite references); `//replacement/first_stage:first-stage-fixed-image` (Layout, 1 symbols, 0 definite references); `//replacement/first_stage:first-stage-image` (Export, 4 symbols, 0 definite references) |
| the 20-byte compressed header, the 36-byte segment entries and 12-byte sub-list header at `0x61c000`, the publication cell and the 32-byte trailer with `0xaaaa` at offset 30 | guest-memory layout view | the stock update container's record 1 and image 4 | no | owned update records replace the stock container layout | required | unestablished | `//replacement/first_stage:first-stage-fixed-image` (Layout, 1 symbols, 0 definite references); `//replacement/first_stage:first-stage-image` (Layout, 1 symbols, 0 definite references) |
| the NAND controller at `0xfff52000` with 128 KiB units and 512-byte sectors | guest-memory layout view | the stock controller and the boot ROM's mode-4 staging | no | the owned flash layer and its device model replace the stock controller | required | unestablished | `//replacement/first_stage:first-stage-fixed-image` (Layout, 1 symbols, 0 definite references); `//replacement/first_stage:first-stage-image` (Layout, 1 symbols, 0 definite references) |
| the memory DMA block at `0xfff02000` the stage fills the application bss through, with the eight-word descriptor at `0x400042c0` and completion on `0xfff02024` bit 6 | guest-memory layout view | the stock record's own `0x3844` fill and the block's device models | no | the application's bss is owned memory that no device has to clear | required | unestablished | `//replacement/first_stage:first-stage-fixed-image` (Layout, 1 symbols, 0 definite references); `//replacement/first_stage:first-stage-image` (Layout, 1 symbols, 0 definite references) |
| `xm1_first_stage_handoff`, `xm1_first_stage_handoff_vector` and `xm1_first_stage_completion_zero`: the handoff reads stock's `0x4300`/`0x4304` words and jumps, the completion export zeroes stock's completion word, and `xm1_first_stage_stage_images` writes the two handoff words | guest-memory layout view | stock loader A's entry word and the record-2 companion wait | no | loader A and the record-2 companion startup are owned | required | unestablished | `//replacement/first_stage:first-stage-fixed-image` (Layout, 4 symbols, 0 definite references); `//replacement/first_stage:first-stage-image` (Layout, 4 symbols, 0 definite references) |
| the loader-A entry at `0x40020` and the image-4 argument the stage reads at the fixed `0x61c40c`, image 4's own header word and not the record table, and jumps to | trampoline into retained stock | stock loader A and the application image the stage hands off to | no | the loaders and application are owned and entered by owned code | required | unestablished | `//replacement/first_stage:first-stage-fixed-image` (RetainedEntry, 2 symbols, 0 definite references); `//replacement/first_stage:first-stage-image` (RetainedEntry, 2 symbols, 0 definite references); `//replacement/first_stage:first-stage-fixed-image` (Import, 10 symbols, 0 definite references); `//replacement/first_stage:first-stage-image` (Import, 10 symbols, 0 definite references) |
| `decode_test_stock.S`'s `xm1_decode_test_acquire` at `0x00683830`, `xm1_decode_test_release_clock` at `0x00649ddc`, `xm1_decode_test_clear_route` at `0x006499d0` and `xm1_decode_test_signal_resource` at `0x00724c00`, called by the RAM-loaded decode test | trampoline into retained stock | the stock shared-engine reservation `hw_fff430_acquire` and the bookkeeping `hw_fff430_release` makes after its enable write, which live view and crop call | no | the decode-test experiment is retired | required | unestablished | reference-only: decode-test is a relocatable object for the RAM-loaded experiment, not part of first-stage-image or a declared produced linked ELF. The decode_test_stock.S services remain required by that experiment. |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/first_stage:first-stage-fixed-image` (Publication, 4 symbols, 0 definite references); `//replacement/first_stage:first-stage-image` (Publication, 4 symbols, 0 definite references) |

## fixed_image

The fixed-range test image preserves the retained startup phase call while observing entry and initialized storage.

Classification: `linked`. [Declarations](fixed_image/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_fixed_test_entry` in `probe-entry.S`, published at `0x69cdf8` by `ProbeMain.hs` | stock-ABI export | retained startup phase callers | no | the placement probe is replaced by the subsystem image's entries | required | unestablished | `//replacement/fixed_image:probe-image` (Export, 1 symbols, 0 definite references) |
| `xm1_fixed_test_resume` at `0x69cdfc`, bound by `ProbeMain.hs` | trampoline into retained stock | the retained startup phase body after its entry push | no | the placement probe is replaced by the subsystem image's entries | required | unestablished | `//replacement/fixed_image:probe-image` (RetainedEntry, 1 symbols, 1 definite references); `//replacement/fixed_image:probe-image` (Import, 1 symbols, 0 definite references) |
| Additional linked unmangled ABI entry points | stock-ABI export | the publication or qualification consumers of the current linked image | partly | all consumers use owned typed interfaces and both sides have a qualified comparison in the same image | required | unestablished | `//replacement/fixed_image:probe-image` (Export, 1 symbols, 0 definite references) |
| Linked publication and qualification metadata | guest-memory layout view | the publication or qualification consumers of the current linked image | partly | all consumers use owned typed interfaces and both sides have a qualified comparison in the same image | required | unestablished | `//replacement/fixed_image:probe-image` (Layout, 3 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/fixed_image:probe-image` (Publication, 4 symbols, 0 definite references) |

## general_heap

The owned general heap, with stock allocator entries, retained lock and memory-copy services, and charged arena metadata. The recipe and fixed-image variants preserve the same boundary.

Classification: `linked`. [Declarations](general_heap/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Stock malloc, free, realloc, calloc and reentrant allocator entries in entry.S and src.rs | stock-ABI export | retained stock allocation callers | no | every allocation caller uses the typed owned allocator | required | unestablished | `//replacement/general_heap:general-heap-fixed-image` (Export, 15 symbols, 0 definite references); `//replacement/general_heap:general-heap-image` (Export, 15 symbols, 0 definite references); `//replacement/general_heap:general-heap-recipe-image` (Export, 15 symbols, 0 definite references) |
| Retained cross-core heap lock and overlap-safe memory-copy helpers | trampoline into retained stock | stock lock/unlock at 0x72b164/0x72b1cc and memmove at 0x89d908 | no | owned synchronization and copying replace every retained allocator service | required | unestablished | `//replacement/general_heap:general-heap-fixed-image` (Import, 10 symbols, 0 definite references); `//replacement/general_heap:general-heap-image` (Import, 10 symbols, 0 definite references); `//replacement/general_heap:general-heap-recipe-image` (Import, 10 symbols, 0 definite references); `//replacement/general_heap:general-heap-fixed-image` (Export, 2 symbols, 0 definite references); `//replacement/general_heap:general-heap-image` (Export, 2 symbols, 0 definite references); `//replacement/general_heap:general-heap-recipe-image` (Export, 2 symbols, 0 definite references) |
| Charged arena metadata and the stock sentinel word | guest-memory layout view | retained allocator consumers and the startup arena publication | partly | owned startup and allocation callers no longer read the stock sentinel or arena metadata | required | unestablished | `//replacement/general_heap:general-heap-fixed-image` (Layout, 2 symbols, 0 definite references); `//replacement/general_heap:general-heap-image` (Layout, 2 symbols, 0 definite references); `//replacement/general_heap:general-heap-recipe-image` (Layout, 2 symbols, 0 definite references) |
| Definite retained references in the current linked instruction and relocation metadata | retained stock seam in assembly | retained code and guest address contracts reached by the composed image | partly | all declared retained targets have owned typed replacements and both sides are compared in the same image | required | unestablished | `//replacement/general_heap:general-heap-fixed-image` (RetainedEntry, 0 symbols, 1 definite references); `//replacement/general_heap:general-heap-image` (RetainedEntry, 0 symbols, 1 definite references); `//replacement/general_heap:general-heap-recipe-image` (RetainedEntry, 0 symbols, 1 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/general_heap:general-heap-fixed-image` (Publication, 1 symbols, 0 definite references); `//replacement/general_heap:general-heap-image` (Publication, 1 symbols, 0 definite references); `//replacement/general_heap:general-heap-recipe-image` (Publication, 1 symbols, 0 definite references) |

## heap_replay

The isolated allocator replay subjects for rlsf, talc and buddy. These qualification images expose the replay harness API, not a retained firmware interface; there are no stock compatibility rows.

Classification: `module`. [Declarations](heap_replay/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |

## idle_task

The shared idle task body.

Classification: `linked`. [Declarations](idle_task/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_idle_task` export in `src.rs`, placed by `idle-task.ld` | stock-ABI export | stock task records for tasks 102/103 that enter the stock idle address | no | the task records and scheduler are owned | required | unestablished | `//replacement/idle_task:idle-task-image` (Export, 1 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/idle_task:idle-task-image` (Publication, 2 symbols, 0 definite references) |

## image5_init

The image-5 synchronous initializer, calling 19 retained stock services.

Classification: `linked`. [Declarations](image5_init/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| the image-5 initializer export and `entry.S` placement in `image5-init.ld` | veneer or carrier at a stock address | the stock image-5 initializer caller | no | the image-5 caller is owned | required | unestablished | `//replacement/image5_init:image5-init-image` (Publication, 1 symbols, 0 definite references); `//replacement/image5_init:image5-init-image` (Export, 1 symbols, 0 definite references) |
| 19 `service_*` retained externs in `src.rs` and `entry.S` | trampoline into retained stock | stock image-5 constructors, task-60 adapter and settings/calibration initializers | no | those services are owned | required | unestablished | `//replacement/image5_init:image5-init-image` (RetainedEntry, 19 symbols, 0 definite references); `//replacement/image5_init:image5-init-image` (Import, 19 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/image5_init:image5-init-image` (Publication, 2 symbols, 0 definite references) |

## image5_load

The owned image-5 loader.

Classification: `linked`. [Declarations](image5_load/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_image5_load` and `xm1_image5_load_entry` in `entry.S`/`src.rs` | veneer or carrier at a stock address | the stock flashloader producer call at `0x73b4ac` | no | the loader caller is owned | required | unestablished | `//replacement/image5_load:image5-load-image` (Publication, 2 symbols, 0 definite references) |
| `xm1_image5_*` retained externs (cache, CPU fill, decoder init/finish, feed, fill request/done, marker, mode, read, segment) | trampoline into retained stock | stock image-5 decoder and cache services | no | the decoder and cache services are owned | required | unestablished | `//replacement/image5_load:image5-load-image` (RetainedEntry, 2 symbols, 0 definite references); `//replacement/image5_load:image5-load-image` (Import, 12 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/image5_load:image5-load-image` (Publication, 2 symbols, 0 definite references) |
| Five asynchronous stream entry veneers | veneer or carrier at a stock address | the image5 producer, storage callback and retained source98 IRQ dispatcher | partly | all producer and IRQ consumers call the typed owned API in the same composed image | required | unestablished | `//replacement/image5_load:image5-decoder-image` (Publication, 5 symbols, 0 definite references) |
| Native resource, clock, route, task, marker and IRQ registration boundaries | trampoline into retained stock | retained RTOS/device services and the two published owned IRQ entry veneers | partly | the retained services and IRQ dispatcher accept direct owned typed interfaces | required | unestablished | `//replacement/image5_load:image5-decoder-image` (Import, 11 symbols, 0 definite references) |
| Fixed-image body bounds and zero-once private decoder state | veneer or carrier at a stock address | the existing fixed-image first-stage loader | partly | the composed loader and decoder share a typed image/BSS lifetime contract | required | unestablished | `//replacement/image5_load:image5-decoder-image` (Publication, 5 symbols, 0 definite references) |

## interface_initializer

The core-1 interface publication after the loader-B completion wait.

Classification: `linked`. [Declarations](interface_initializer/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_interface_initializer_entry` and `xm1_interface_initializer_publish` in `entry.S`/`src.rs` | veneer or carrier at a stock address | the stock loader-B completion caller | no | the loader-B caller is owned | required | unestablished | `//replacement/critical_pair:critical-pair-image` (Publication, 2 symbols, 0 definite references); `//replacement/interface_initializer:interface-initializer-image` (Publication, 2 symbols, 0 definite references); `//replacement/interface_initializer:interface-initializer-image` (Import, 52 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/interface_initializer:interface-initializer-image` (Publication, 5 symbols, 0 definite references) |

## kernel

The kernel crate: typed handles over the image's interface slots, with the per-core slot table as its first backend.

Classification: `module`. [Declarations](kernel/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `SlotTable` in `src/slot_table.rs` and `src/slot_table/messages.rs` | kernel interface slot | the kernel bodies behind the root blocks at 0x96bd20 and 0x96bd24, the loader-A console writer 0x41a34, the error-callback word at 0x4089ea8c, the post hook at 0x4089ea94, task/queue classes and message-pool capacity words | partly | every image whose `.symbols` lists an `xm1_kernel` item also lists the kernel family entry for each slot offset in `SlotTable`'s table | required | unestablished | `//host/firmware:engine-elf-modal75-menu-engine` (Layout, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Layout, 1 symbols, 0 definite references); `//replacement/develop:develop-candidate` (Layout, 1 symbols, 0 definite references); `//replacement/property_service:property-service-image` (Layout, 1 symbols, 0 definite references); `//replacement/property_service:property-service-linked-image` (Layout, 1 symbols, 0 definite references); `//replacement/property_service:property-service-public-image` (Layout, 1 symbols, 0 definite references); `//replacement/settings_storage:settings-storage-image` (Layout, 1 symbols, 0 definite references); `//replacement/time_services:image` (Layout, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Export, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Export, 1 symbols, 0 definite references); `//replacement/develop:develop-candidate` (Export, 1 symbols, 0 definite references); `//replacement/property_service:property-service-image` (Export, 1 symbols, 0 definite references); `//replacement/property_service:property-service-linked-image` (Export, 1 symbols, 0 definite references); `//replacement/property_service:property-service-public-image` (Export, 1 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-image` (Export, 1 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (Export, 1 symbols, 0 definite references); `//replacement/settings_storage:settings-storage-image` (Export, 1 symbols, 0 definite references); `//replacement/time_services:image` (Export, 1 symbols, 0 definite references) |
| `CoreLock` in `src/slot_table.rs` | trampoline into retained stock | the stock cross-core critical section at 0x7235d8 and its release 0x7236b0 | partly | no stock caller of 0x7235d8 remains | required | unestablished | `//host/firmware:engine-elf-modal75-menu-engine` (RetainedEntry, 1 symbols, 2 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (RetainedEntry, 1 symbols, 2 definite references); `//replacement/develop:develop-candidate` (RetainedEntry, 1 symbols, 2 definite references); `//replacement/property_service:property-service-image` (RetainedEntry, 1 symbols, 2 definite references); `//replacement/property_service:property-service-linked-image` (RetainedEntry, 1 symbols, 2 definite references); `//replacement/property_service:property-service-public-image` (RetainedEntry, 1 symbols, 2 definite references); `//replacement/settings_storage:settings-storage-image` (RetainedEntry, 1 symbols, 2 definite references); `//replacement/time_services:image` (RetainedEntry, 1 symbols, 2 definite references) |
| Object construction and deletion in `src/slot_table/messages.rs` | trampoline into retained stock | queue constructor 0x724cdc and deleter 0x724f08, binary-semaphore allocator 0x7247ac and deleter 0x724974, semaphore poll 0x724ad8 | no | the kernel crate owns queue backing allocation and resource lifetime, and semaphore allocation/deletion/poll, in every linked image | required | unestablished | `//replacement/time_services:image` (RetainedEntry, 1 symbols, 5 definite references) |
| `SemaphoreWord` in `src/slot_table/messages.rs` | trampoline into retained stock | timed wait 0x724b68 and signal 0x724c00 for failed creation words | no | retained callers no longer preserve or use failed semaphore creation words | required | unestablished | `//replacement/develop:develop-candidate` (RetainedEntry, 1 symbols, 2 definite references); `//replacement/time_services:image` (RetainedEntry, 1 symbols, 2 definite references) |
| Dispatch bracket in `src/slot_table/messages.rs` | trampoline into retained stock | dispatch disable 0x723efc and enable 0x723f7c | no | the kernel crate owns the dispatch wrappers and their error reporting in every linked image | required | unestablished | `//replacement/time_services:image` (RetainedEntry, 1 symbols, 2 definite references) |
| Task construction and deletion in `src/slot_table/task.rs` | trampoline into retained stock | task constructor 0x725dd8 and deletion 0x72612c, including its storage release 0x726098 | no | the kernel crate owns task registration, stack allocation, activation, deletion and reservation release in every linked image | required | unestablished | `//replacement/time_services:image` (RetainedEntry, 1 symbols, 2 definite references) |

## kernel_install

The kernel installer and its cache maintenance.

Classification: `linked`. [Declarations](kernel_install/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_kernel_install` and `xm1_kernel_install_entry` in `entry.S`/`src.rs` | veneer or carrier at a stock address | the stock phase entry and installer callers | no | the phase entry and installer callers are owned | required | unestablished | `//replacement/kernel_install:kernel-install-image` (Publication, 2 symbols, 0 definite references) |
| `xm1_kernel_cache` retained service | trampoline into retained stock | the stock cache-maintenance helper | no | cache maintenance is owned | required | unestablished | `//replacement/kernel_install:kernel-install-image` (Import, 1 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/kernel_install:kernel-install-image` (Publication, 2 symbols, 0 definite references) |

## lens_transport

The lens transport and control actor. Its stock-ABI exports and retained task/queue/IRQ services are the current seam; power leaf calls are already owned on both sides.

Classification: `linked`. [Declarations](lens_transport/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_power_current_task`, `xm1_power_interrupt_lock`, `xm1_power_interrupt_unlock`, `xm1_power_service_post`, `xm1_power_set_task_flags`, `xm1_power_resource_route_request` externs | C-ABI bridge between two owned units | power_lifecycle leaf | yes | the leaf is linked directly and both sides compare with stock in one image | required | unestablished | reference-only: The registered mechanism remains required; this reference or qualification-only mechanism is not part of the selected linked image. `xm1_power_current_task`, `xm1_power_interrupt_lock`, `xm1_power_interrupt_unlock`, `xm1_power_service_post`, `xm1_power_set_task_flags`, `xm1_power_resource_route_request` externs |
| 14 `xm1_lens_*` exports in `src/abi.rs` and `src/control_native.rs`, placed by `control-roots.ld` and `codec.ld` | stock-ABI export | stock lens task and control callers | no | every lens caller is owned or migrated | required | unestablished | reference-only: The registered mechanism remains required; this reference or qualification-only mechanism is not part of the selected linked image. 14 `xm1_lens_*` exports in `src/abi.rs` and `src/control_native.rs`, placed by `control-roots.ld` and `codec.ld` |
| `xm1_lens_task99_entry` in `native/control-entry.S` | veneer or carrier at a stock address | the stock task99 entry | no | the task99 entry is owned | required | unestablished | reference-only: The registered mechanism remains required; this reference or qualification-only mechanism is not part of the selected linked image. `xm1_lens_task99_entry` in `native/control-entry.S` |
| retained task/queue/IRQ externs (`task_create`, `task_delete`, `activate`, `queue_create`, `queue_delete`, `send`, `send_irq`, `receive`, `delay`, `cyclic_*`, `xm1_rtos_irq_*`) | trampoline into retained stock | stock RTOS task, queue and IRQ services | no | the RTOS platform owns those services | required | unestablished | reference-only: The registered mechanism remains required; this reference or qualification-only mechanism is not part of the selected linked image. retained task/queue/IRQ externs (`task_create`, `task_delete`, `activate`, `queue_create`, `queue_delete`, `send`, `send_irq`, `receive`, `delay`, `cyclic_*`, `xm1_rtos_irq_*`) |
| `Callbacks` C-ABI struct and callback registration in `src/abi.rs` | callback registry for guest words | stock callers that pass function pointers and context words | no | callers use the typed completion sink | required | unestablished | reference-only: The registered mechanism remains required; this reference or qualification-only mechanism is not part of the selected linked image. `Callbacks` C-ABI struct and callback registration in `src/abi.rs` |
| `lens_actor_request`, `lens_actor_wake`, `lens_actor_timer`, `lens_actor_device_callback` and the fixed stock addresses in `src/actor_abi.rs`, built only with feature `native-actor` and placed by `native/actor.ld` | stock-ABI export | the native comparison harness, not stock firmware | no | the actor is compared with stock through its owned interface in a linked image | required | unestablished | `//replacement/lens_transport:actor` (Export, 4 symbols, 0 definite references) |

## loader_orchestration

The loader orchestration step.

Classification: `linked`. [Declarations](loader_orchestration/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_loader_orchestration` and `xm1_loader_orchestration_entry` in `entry.S`/`src.rs`, placed by `loader-a.ld`/`loader-b.ld` | veneer or carrier at a stock address | the stock loader continuation | no | the loader continuation is owned | required | unestablished | `//replacement/loader_orchestration:loader-a-orchestration-image` (Publication, 2 symbols, 0 definite references); `//replacement/loader_orchestration:loader-b-orchestration-image` (Publication, 2 symbols, 0 definite references) |
| 21 `xm1_loader_*` retained externs (continue, prepare, activate timers, auto-ready, clear, identity, log, peer, private initialize, publish callback, schedule) | trampoline into retained stock | stock loader and kernel startup services | no | those services are owned | required | unestablished | `//replacement/loader_orchestration:loader-a-orchestration-image` (RetainedEntry, 2 symbols, 0 definite references); `//replacement/loader_orchestration:loader-a-orchestration-image` (Import, 23 symbols, 0 definite references); `//replacement/loader_orchestration:loader-b-orchestration-image` (RetainedEntry, 2 symbols, 0 definite references); `//replacement/loader_orchestration:loader-b-orchestration-image` (Import, 10 symbols, 0 definite references); `//replacement/loader_orchestration:loader-b-orchestration-image` (Import, 23 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/loader_orchestration:loader-a-orchestration-image` (Publication, 2 symbols, 0 definite references); `//replacement/loader_orchestration:loader-b-orchestration-image` (Publication, 2 symbols, 0 definite references) |

## loader_prefix

The owned loader-B core-1 entry.

Classification: `linked`. [Declarations](loader_prefix/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_loader_prefix_entry`, `xm1_loader_prefix_exit`, `xm1_loader_prefix_install`, `xm1_loader_prefix_clear` and the fault variants in `entry.S`/`src.rs` | veneer or carrier at a stock address | the stock loader-B entry and the record-2 handoff | no | loader-B startup is owned | required | unestablished | `//replacement/critical_pair:critical-pair-image` (Publication, 4 symbols, 0 definite references); `//replacement/interface_initializer:interface-initializer-image` (Publication, 4 symbols, 0 definite references); `//replacement/loader_prefix:loader-prefix-image` (Publication, 4 symbols, 0 definite references); `//replacement/critical_pair:critical-pair-image` (Export, 1 symbols, 0 definite references); `//replacement/interface_initializer:interface-initializer-image` (Export, 1 symbols, 0 definite references); `//replacement/loader_prefix:loader-prefix-image` (Export, 1 symbols, 0 definite references) |
| `xm1_loader_prefix_clear` retained service | trampoline into retained stock | the stock workspace-clear helper | no | the clear is owned | required | unestablished | `//replacement/critical_pair:critical-pair-image` (RetainedEntry, 1 symbols, 0 definite references); `//replacement/interface_initializer:interface-initializer-image` (RetainedEntry, 1 symbols, 0 definite references); `//replacement/loader_prefix:loader-prefix-image` (RetainedEntry, 1 symbols, 0 definite references); `//replacement/loader_prefix:loader-prefix-image` (Import, 52 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/loader_prefix:loader-prefix-image` (Publication, 5 symbols, 0 definite references) |

## media_permit

The media-write permit resolver and its effects over retained frame services.

Classification: `linked`. [Declarations](media_permit/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 11 exports in `src/lib.rs` (permit resolution, cancel, effects) | stock-ABI export | stock callers of `FUN_00e77168` and the permit entry | no | every permit caller is owned or migrated | required | unestablished | `//replacement/media_permit:media-permit-candidate` (Export, 1 symbols, 0 definite references) |
| `rb_mp_stock_*` trampolines in `rb_media_permit_stock.S` (bit set, cancel, command, context, count/kind, fatal, frame kind/of-kind, map subframe, mirror read, subframe, subframe count) | trampoline into retained stock | stock frame, event and slot services | no | the frame services are owned | required | unestablished | `//replacement/develop:develop-candidate` (RetainedEntry, 12 symbols, 0 definite references); `//replacement/media_permit:media-permit-candidate` (RetainedEntry, 12 symbols, 0 definite references) |
| `media_permit.ld` and the `cancel`/`command`/`count`/`fatal2`/`low`/`side_a`/`side_b` placements | veneer or carrier at a stock address | stock permit callers | no | owned code is called directly | required | unestablished | `//replacement/media_permit:media-permit-candidate` (Publication, 1 symbols, 0 definite references) |
| `rb_mp_forbidden_body_probe` in `rb_media_permit_probe.S` | retained stock seam in assembly | the retired resolver body used as a negative control | no | the probe is removed with the qualification controls | required | unestablished | reference-only: The registered mechanism remains required; this reference or qualification-only mechanism is not part of the selected linked image. `rb_mp_forbidden_body_probe` in `rb_media_permit_probe.S` |
| Additional linked unmangled ABI entry points | stock-ABI export | the publication or qualification consumers of the current linked image | partly | all consumers use owned typed interfaces and both sides have a qualified comparison in the same image | required | unestablished | `//replacement/media_permit:media-permit-candidate` (Export, 3 symbols, 0 definite references) |
| Definite retained references in the current linked instruction and relocation metadata | retained stock seam in assembly | retained code and guest address contracts reached by the composed image | partly | all declared retained targets have owned typed replacements and both sides are compared in the same image | required | unestablished | `//replacement/media_permit:media-permit-candidate` (RetainedEntry, 0 symbols, 13 definite references) |

## menu_engine

The address-independent menu engine and its stock-state mirror. Its shims are listed in [the engine design](../reports/owned-menu-engine-design.md#shims-for-this-unit).

Classification: `module`. [Declarations](menu_engine/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `rb_menu_engine_entry` and the fallback push in `adapter/entry.S`, published at `0xbf95c8` | veneer or carrier at a stock address | the stock stage consumer and `0xbfa234` fallback | no | the owned key layer and stage consumers call the engine directly | required | unestablished | `//host/firmware:engine-elf-modal75-menu-engine` (Publication, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Publication, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Export, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Export, 1 symbols, 0 definite references) |
| `rb_menu_engine_call_*` retained services (availability, initial row, kind switch, notify, seek) | trampoline into retained stock | stock menu services | no | the tree and typed settings service answer them | required | unestablished | `//host/firmware:engine-elf-modal75-menu-engine` (RetainedEntry, 5 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (RetainedEntry, 5 symbols, 0 definite references) |
| `rb_menu_engine_guard_failure` and `__aeabi_*` in `adapter/entry.S` | retained stock seam in assembly | stock ABI helper routines and the guard path | no | the engine no longer needs the stock ABI helpers | required | unestablished | `//host/firmware:engine-elf-modal75-menu-engine` (RetainedEntry, 6 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (RetainedEntry, 6 symbols, 0 definite references) |
| block mirror at `0x430` bytes and `0x01467580/84` (`src/state.rs`) | guest-memory layout view | stock readers and writers of the menu block | no | no stock code reads or writes the block | required | unestablished | `//host/firmware:engine-elf-modal75-menu-engine` (Layout, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Layout, 1 symbols, 0 definite references) |
| `rb_ui_probe_visit` page-entry replay | callback registry for guest words | the installed bank's `rb_bank_page_entry` hook | no | the bank's page-entry work is an owned engine consumer | required | unestablished | `//host/firmware:bank-elf-00c40000` (RetainedEntry, 1 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (RetainedEntry, 1 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (RetainedEntry, 1 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (RetainedEntry, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (RetainedEntry, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (RetainedEntry, 1 symbols, 0 definite references) |

## menu_value

The IMAGE SIZE apply unit, published at the stock commit dispatcher's case word; other property arms remain retained.

Classification: `module`. [Declarations](menu_value/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `rb_menu_value_size_arm` in `adapter/entry.S`, published at the stock IMAGE SIZE case word | veneer or carrier at a stock address | the stock dispatcher's jump-table word for property `0xf6` | no | the owned dispatcher calls the transaction directly | required | unestablished | reference-only: The registered mechanism remains required; this unit has no declared produced linked ELF carrying this interface. `rb_menu_value_size_arm` in `adapter/entry.S` enters the owned apply or returns to the retained arm/shared tail. |
| `rb_menu_value_size_apply` export in `adapter/mod.rs` | stock-ABI export | the IMAGE SIZE assembly veneer | no | the owned dispatcher calls the typed transaction directly without this assembly-to-Rust ABI seam | required | unestablished | reference-only: The registered mechanism remains required; this unit has no declared produced linked ELF carrying this interface. `rb_menu_value_size_apply` in `adapter/mod.rs` is the veneer-to-Rust ABI. |
| `rb_menu_value_call_*` retained services: size cell, aspect cell, change marker, current-property marker and display update | trampoline into retained stock | stock property, cell and display services | no | the typed settings service and owned display own those services | required | unestablished | reference-only: The registered mechanism remains required; this unit has no declared produced linked ELF carrying this interface. The five `rb_menu_value_call_*` imports in `adapter/mod.rs` remain retained. |
| Current/saved choice cells and display-mode state read in `adapter/mod.rs` | guest-memory layout view | retained stock choice and snapshot state | no | owned menu/settings state replaces these cells and no retained reader requires their layout | required | unestablished | reference-only: Fixed current/saved size, aspect and third-choice cells and MODE_CELL remain read from retained state. Linked-image layout and caller closure are not established by this declaration. |

## menu_widgets

The owned tree, pane and picker widgets and their native adapters.

Classification: `linked`. [Declarations](menu_widgets/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `owned_pane_drawer`, `tree_list_entry` and the picker/pane/tree native entries in `native/*.rs`, placed by `picker.ld`, `pane_drawer.ld`, `tree.ld` | stock-ABI export | stock draw callers that enter the widget addresses | no | the owned renderer calls the widgets directly | required | unestablished | `//host/firmware:engine-elf-modal75-menu-engine-render` (RetainedEntry, 3 symbols, 0 definite references) |
| `rb_picker_call_*`, `rb_tree_call_*` and `rb_pane_call_*` retained services (draw leaves, hooks, menu-table accessors, setting reads, snapshot readers) | trampoline into retained stock | stock draw layer and menu table | no | the owned renderer, tree and settings service own those services | required | unestablished | `//host/firmware:engine-elf-modal75-menu-engine-render` (RetainedEntry, 3 symbols, 0 definite references) |
| side state `0x15e64d8` and the palette/report block kept for stock readers | guest-memory layout view | stock readers `FUN_011662a0`, `FUN_01165aa0` and the bank root drawer | no | those readers are owned | required | unestablished | `//host/firmware:engine-elf-modal75-menu-engine-render` (Layout, 1 symbols, 0 definite references) |
| installed label-id bound `0x849` in the native adapters | guest-memory layout view | the installed package's label table | no | the package's labels are declared in the tree | required | unestablished | `//host/firmware:engine-elf-modal75-menu-engine-render` (Layout, 1 symbols, 0 definite references) |

## modal_runtime

Modal dispatch and controller 75.

Classification: `module`. [Declarations](modal_runtime/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `rb_modal_entry` in `src/entry.S`, published at `0xbfb028`, with the stock+4 fallback for every controller other than 75 | veneer or carrier at a stock address | the stock dispatcher and 173 stock controllers | no | the owned dispatcher owns all 174 controllers | required | unestablished | `//host/firmware:bank-modal-elf-00c40000` (Publication, 1 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (Publication, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Publication, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Publication, 1 symbols, 0 definite references) |
| `rb_modal_dispatch` export | C-ABI bridge between two owned units | power_lifecycle | yes | power links the dispatch crate directly and both sides compare with stock in one image | required | unestablished | `//host/firmware:bank-modal-elf-00c40000` (Export, 1 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (Export, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Export, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Export, 1 symbols, 0 definite references) |
| `rb_modal_call_prehook`, `rb_modal_call` and `dispatch.S` callback glue | callback registry for guest words | the retained prehook `0xd0cc68` and stock action/transition callbacks | no | every callback is owned | required | unestablished | `//host/firmware:bank-modal-elf-00c40000` (RetainedEntry, 2 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (RetainedEntry, 2 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (RetainedEntry, 2 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (RetainedEntry, 2 symbols, 0 definite references) |
| `rb_call_modal_query`, `rb_call_redraw`, `rb_call_stage_open`, `rb_call_transition`, `rb_bank_key_left/right`, `rb_lifecycle_*` retained services | trampoline into retained stock | stock modal and bank services | no | those services are owned | required | unestablished | `//host/firmware:bank-elf-00c40000` (RetainedEntry, 4 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (RetainedEntry, 4 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (RetainedEntry, 4 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (RetainedEntry, 4 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (RetainedEntry, 4 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (RetainedEntry, 4 symbols, 0 definite references) |

## owned_menu

The owned controller-75 editor and menu-frame snapshot service, with its stock address exports and retained lifecycle services.

Classification: `module`. [Declarations](owned_menu/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `rb_ui_set`, `rb_ui_leave`, `rb_ui_editor_cancel`, `rb_ui_editor_confirm` and the nine `rb_menu_frame_*` accessors | stock-ABI export | stock controller arms and frame snapshot callers | no | every controller arm and snapshot caller is owned | required | unestablished | `//host/firmware:bank-elf-00c40000` (Export, 18 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (Export, 18 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (Export, 18 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (Export, 18 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Export, 10 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Export, 10 symbols, 0 definite references) |
| `rb_lifecycle_*`, `rb_call_*`, `rb_open_edit*` retained services in `src/lib.rs` | trampoline into retained stock | stock lifecycle, modal query, redraw, stage and transition services | no | those services are owned | required | unestablished | `//host/firmware:bank-elf-00c40000` (RetainedEntry, 53 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (RetainedEntry, 53 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (RetainedEntry, 53 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (RetainedEntry, 53 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (RetainedEntry, 53 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (RetainedEntry, 53 symbols, 0 definite references) |
| editor buffers, frame snapshots, mode/gate cells and slot tables | guest-memory layout view | stock readers of the editor and frame state | no | no stock code reads the editor or frame state | required | unestablished | `//host/firmware:engine-elf-modal75-menu-engine` (Layout, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Layout, 1 symbols, 0 definite references) |

## peer_wait_control

The loader peer-wait control carrier.

Classification: `linked`. [Declarations](peer_wait_control/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_peer_wait_control_entry`, `_poll`, `_acknowledged`, `_failure`, `_end` in `entry.S` | veneer or carrier at a stock address | the stock loader peer-wait site | no | the loader peer wait is owned | required | unestablished | `//replacement/peer_wait_control:peer-wait-control-image` (Publication, 1 symbols, 0 definite references); `//replacement/peer_wait_control:peer-wait-control-image` (Import, 1 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/peer_wait_control:peer-wait-control-image` (Publication, 4 symbols, 0 definite references) |

## plain_load

The owned plain-image loader.

Classification: `linked`. [Declarations](plain_load/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_plain_load` and `xm1_plain_load_entry` in `entry.S`/`src.rs` | veneer or carrier at a stock address | the stock plain-loader caller | no | the caller is owned | required | unestablished | `//replacement/plain_load:plain-load-image` (Publication, 2 symbols, 0 definite references) |
| `xm1_plain_delay`, `xm1_plain_extents`, `xm1_plain_give`, `xm1_plain_mode`, `xm1_plain_progress`, `xm1_plain_read`, `xm1_plain_segment`, `xm1_plain_take` retained services | trampoline into retained stock | stock plain-loader and scheduler services | no | those services are owned | required | unestablished | `//replacement/plain_load:plain-load-image` (Import, 8 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/plain_load:plain-load-image` (Publication, 2 symbols, 0 definite references) |

## playback

Emulator-only source/mode preparation, ordinary JPEG lifecycle and nominal MMIO services compiled into relocatable RAM experiment objects. Seventeen whole entries use thirty-four actual publications, qualified by conditional native and guarded installed preparation/PLAY comparisons. Develop/machine5 callers, command4 continuation, resource/kernel/timer/cache/IRQ services and shared publication lifetimes remain retained boundaries.

Classification: `module`. [Declarations](playback/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Source and mode A32 entry exports and two-word publication veneers | stock-ABI export | Queue-4 loop, retained router and machine-1 action callers | partly | all entry callers link the typed owned preparation API directly | required | unestablished | reference-only: The relocatable object is linked at the experiment's allocated runtime base; installation and conditional caller receipts bind that package. |
| Retained machine dispatch, output route, display preset and memory ABI | trampoline into retained stock | Stock dispatcher0x011e8bf8, route0x0064982c, preset0x006d1294 and memory0x0089d908 | partly | the packaged composition links the owned dispatch, output, preset and memory providers | required | unestablished | reference-only: The experiment binds these services to the retained addresses in the preparation contract. |
| Queue-4 payload lifetime and shared playback globals | guest-memory layout view | Retained message storage and machine0/1/11 consumers | partly | the message producer and all shared-state consumers use the owned typed lifetime and layout contract | required | unestablished | reference-only: Queue4 serializes entry. Source-bound payload extents stay readable and disjoint from copied destinations; the experiment remains resident over the guarded PLAY interval. |
| Channel9 acquire, restart, release and descriptor callback A32 exports | stock-ABI export | Retained Playback machine5 and overlay47 callers, owned ordinary lifecycle, static-record builder and develop job4, descriptor0x48 IRQ registry | partly | all transaction callers and IRQ registration use the typed owned JPEG API directly | required | unestablished | reference-only: The twenty-hook lifecycle and develop-consumer package has independent stock native comparisons and guarded installed preparation/PLAY receipts, with exactly forty ABI7 publication records restored on rollback. Historical eight- and seventeen-hook receipts remain separately identified. |
| Static-record construction, develop job4 consumers and whole ordinary Playback JPEG lifecycle A32 exports | stock-ABI export | Develop framework finish caller 0x00e936a8, overlay47 clear-record caller 0x01238160, machine5 and deferred notification, task-event notify/wait/retain, nested critical and timer providers, subscriber callbacks | partly | all producer and completion callers link the typed owned API directly; static-request and subscriber quiescence permits publication removal | required | unestablished | reference-only: Exact whole lifecycle and develop job4/notification/clear-record bodies retain the source/output interface. Natural cancellation owns entry0x0122026c;0x01220288 is a stock observation interior only. Owned finish/cancel release observations are immutable interior labels. Preparation construction through job4 and PLAY handbacks remain separate epochs; job4 construction runs only on task0xb. Owned job4 releases after its record notification and re-waits after unrelated event-0x10 wakes; on the cancel-all signature 0x40 it submits stock's reset/release sequence and returns Cancelled, which the retained framework caller discards. |
| Retained JPEG continuation, resource routing, lock, cache, timer and task-event services | trampoline into retained stock | Command4 endpoint reprogramming0x0065aa44, resource1e, reserved lock9, cache0x00042634, IRQ registration/release, timer0x0069b328/0x0069b7a4 and task-event notify0x00724558/wait0x007243a8/retain0x007246f0 | partly | the packaged transaction links owned service providers with qualified hardware laws | required | unestablished | reference-only: Nominal low-level MMIO services are defined in the owned transaction artifact. Native high-level callers keep explicit device-service boundaries; full-system polling and MMIO acceptance use the existing JPEG model. Command4 and status17/24 device behavior remain unqualified. |
| Shared JPEG request/configuration layout and published IRQ lifetime | guest-memory layout view | Retained machine5/overlay47 consumers, owned develop job4 and ordinary Playback lifecycle, retained interrupt release | partly | all shared-state consumers use owned layouts and IRQ quiescence permits publication removal | required | unestablished | reference-only: The runtime and callback remain resident through nominal PLAY and retained IRQ release; unload and post-consumer reuse are not qualified. |

## playback_actor

Owned queue-4 playback machine 0 in place in the plain image: republished action-table rows, A32 action exports behind the stock dispatcher and import stubs for the retained playback services.

Classification: `linked`. [Declarations](playback_actor/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| machine-0 action-table rows in `table.S` and `playback-actor.ld` | veneer or carrier at a stock address | the stock queue-4 dispatcher 0x011e8bf8 | no | the owned dispatcher calls the owned machine directly | required | unestablished | `//replacement/playback_actor:playback-actor-image` (Publication, 1 symbols, 0 definite references) |
| `imports.ld` import stubs | trampoline into retained stock | the retained playback status sources, notifications, pending-event, mode-work and device-notification services | no | the owned playback machines and notification services replace them | required | unestablished | `//replacement/playback_actor:playback-actor-image` (Import, 32 symbols, 0 definite references) |
| Owned machine-0 actions in `playback_actor/src/stock.rs` | stock-ABI export | the stock dispatcher's no-argument action call and its use of r0 as the dispatch result | partly | an owned dispatcher calls a typed owned machine API | required | unestablished | `//replacement/playback_actor:playback-actor-image` (Export, 31 symbols, 0 definite references) |

## power_lifecycle

The Power Services leaf crate and the power application crate. The leaf exports 360 record helpers at the stock ABI through `stock_export!`; the application crate calls other owned units through C-ABI exports and keeps stock record-helper residue.

Classification: `linked`. [Declarations](power_lifecycle/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_app_nand_*`, `record_store_*`, `xm1_calibration_*`, `xm1_property_*`, `xm1_getter_owned`, `xm1_setter_owned`, `xm1_storage_initialize`, `xm1_pipeline_image_*`, `xm1_task_submit/release`, `rb_modal_dispatch`, `xm1_scanout_*` externs | C-ABI bridge between two owned units | application_nand, record_store, calibration_liveview, property_service, settings_storage, develop, task_submission, modal_runtime, display_scanout | yes | every caller links the owning crate directly and both sides compare with stock in one image | required | unestablished | reference-only: The current record-helper ELF contains none of these application/client C-ABI symbols. The application arm-core library remains nonlinked here; each provider/client composition and its stock comparison are still required. |
| 360 stock-ABI record-helper exports through `stock_export!` in 45 files under `src/` (`record_native_exports.rs` defines the macro) | stock-ABI export | stock record consumers that call the helper addresses | no | every record-helper caller is owned and the helpers are linked as Rust items | required | unestablished | `//replacement/power_lifecycle:record-helpers` (Export, 278 symbols, 0 definite references) |
| 14 `.S` residue files in `src/` (`native.S`, `record_task43_*`, `capture_profile_*`, `mode_*`, `serial_write_residue.S`, and the rest) | retained stock seam in assembly | stock bodies and register/stack frames the owned code must reproduce | no | the residue is no longer needed for stock callers or ABI parity | required | unestablished | reference-only: None of the 31 declared assembly entry symbols is linked into the current record-helper ELF. The residue-bearing application and service mechanisms remain nonlinked obligations; no frozen body is rebuilt for coverage. |
| ~1,200 retained `extern "C"` declarations in `src/` (record bodies, audio, capture profile, serial, timers) | trampoline into retained stock | stock record, audio, capture and device services | no | those services are owned | required | unestablished | `//replacement/power_lifecycle:record-helpers` (Import, 3 symbols, 0 definite references); reference-only: Only the explicit three kernel imports are bound by the current record-helper ELF. The broad conditional application extern inventory is not linked coverage or an established caller closure. |
| `record-helpers.ld` placement of the helper exports | veneer or carrier at a stock address | stock callers of the record helpers | no | owned callers link directly | required | unestablished | `//replacement/power_lifecycle:record-helpers` (Publication, 1 symbols, 0 definite references) |
| `xm1_kernel_console9`, `xm1_kernel_critical_enter` and `xm1_kernel_critical_exit` in `native/record-helpers.ld` | trampoline into retained stock | loader-A console writer 0x41a34 and cross-core critical pair 0x7235d8/0x7236b0 through the linked settings core's kernel backend | no | the settings core uses owned console and cross-core services | required | unestablished | `//replacement/power_lifecycle:record-helpers` (Import, 3 symbols, 0 definite references) |
| `KernelSlot`-based root service calls in `callback_core` (re-exported by `src/callback_registry.rs`) and the audio modules | kernel interface slot | the stock kernel roots at `0x4004dae0`/`0x400ddaa0` | no | the RTOS platform owns the root services | required | unestablished | reference-only: The OSAL kernel-slot entry consumers are absent from the current record-helper ELF. A services composition retaining these entry consumers and their kernel-slot contract is required. |
| callback registries for guest words in `callback_core` (re-exported by `src/callback_registry.rs`) and the audio modules | callback registry for guest words | stock callers that pass function pointers and context words | no | callers use typed callbacks | required | unestablished | reference-only: The callback registry entry and its audio consumer entries are absent from the current record-helper ELF. Their registry and callback-extent contracts remain nonlinked obligations. |
| Additional linked unmangled ABI entry points | stock-ABI export | the publication or qualification consumers of the current linked image | partly | all consumers use owned typed interfaces and both sides have a qualified comparison in the same image | required | unestablished | `//replacement/power_lifecycle:record-helpers` (Export, 1 symbols, 0 definite references) |
| Power leaf C-ABI services: OSAL, resource routing, serial, timer, semaphore and audio service exports | C-ABI bridge between two owned units | display_scanout, lens_transport, application_nand and vendor_protocol | yes | all four consumers link the Power services crate as Rust items without the C ABI and both sides have stock comparisons in the same image | required | unestablished | reference-only: The 88 named producer service exports cover OSAL, resource routing, task flags, queues, software timers, serial, diagnostics, callback registration and audio, including the declared consumers' IRQ entry requirements. None is linked into the record-helper ELF. Their consumer-bearing service composition is still required; helper exports are not substitute evidence. |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/power_lifecycle:record-helpers` (Publication, 2 symbols, 0 definite references) |

## primary_initializer

The loader-A interface publisher.

Classification: `linked`. [Declarations](primary_initializer/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_primary_initializer_entry` and `xm1_primary_initializer_publish` in `entry.S`/`src.rs`, placed at `0x40430` by `primary-initializer.ld` | veneer or carrier at a stock address | the stock loader-A caller | no | the loader-A caller is owned | required | unestablished | `//replacement/primary_initializer:primary-initializer-image` (Publication, 2 symbols, 0 definite references); `//replacement/primary_initializer:primary-initializer-image` (Import, 1 symbols, 0 definite references) |
| the 97-slot root descriptor table written by the publisher | kernel interface slot | retained stock root-service callers | no | the RTOS platform owns the root services | required | unestablished | `//replacement/primary_initializer:primary-initializer-image` (Layout, 1 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/primary_initializer:primary-initializer-image` (Publication, 7 symbols, 0 definite references) |

## property_service

The property classifier, table generator, service and public entries. Its compatibility contract is [the property shim table](settings-storage.md#property-compatibility-shims).

Classification: `linked`. [Declarations](property_service/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| repointed table words selecting `xm1_property_get`/`xm1_property_set` | stock-ABI export | the stock entry/dispatch tables | no | owned entry and dispatch replace both stock dispatchers | required | unestablished | `//replacement/develop:develop-candidate` (Export, 1 symbols, 0 definite references); `//replacement/property_service:property-service-image` (Export, 2 symbols, 0 definite references); `//replacement/property_service:property-service-linked-image` (Export, 2 symbols, 0 definite references); `//replacement/property_service:property-service-public-image` (Export, 2 symbols, 0 definite references); `//replacement/settings_storage:settings-storage-image` (Export, 2 symbols, 0 definite references) |
| `xm1_getter_stub`/`xm1_setter_stub` stock-frame stubs and the shared exits/fallbacks | veneer or carrier at a stock address | the stock prologues, shared exits and out-of-range ids | no | owned entry and dispatch handle every id and frame | required | unestablished | `//replacement/property_service:property-service-image` (Publication, 1 symbols, 0 definite references); `//replacement/property_service:property-service-image` (Export, 1 symbols, 0 definite references) |
| `xm1_storage_store8` and `xm1_recipe_edit_get` retained services | trampoline into retained stock | the retained bank/previous-value storage and recipe getter | no | owned settings storage and the recipe getter own those paths | required | unestablished | `//replacement/property_service:property-service-image` (RetainedEntry, 1 symbols, 0 definite references); `//replacement/property_service:property-service-image` (Import, 12 symbols, 0 definite references); `//replacement/property_service:property-service-linked-image` (Import, 12 symbols, 0 definite references); `//replacement/property_service:property-service-public-image` (Import, 12 symbols, 0 definite references) |
| `xm1_property_get`/`xm1_property_set` exports | C-ABI bridge between two owned units | develop and power_lifecycle | yes | both callers link the crate directly and both sides compare with stock in one image | required | unestablished | `//replacement/develop:develop-candidate` (Export, 1 symbols, 0 definite references); `//replacement/property_service:property-service-image` (Export, 2 symbols, 0 definite references); `//replacement/property_service:property-service-linked-image` (Export, 2 symbols, 0 definite references); `//replacement/property_service:property-service-public-image` (Export, 2 symbols, 0 definite references); `//replacement/settings_storage:settings-storage-image` (Export, 2 symbols, 0 definite references) |
| base-fixed accessors, named default leaves, bank-0 bracket callees and the carrier in retired setter arms (`src/`, `entry*.S`, `property-service*.ld`, `menu/recipe_bank_ram_engine.ld`) | guest-memory layout view | retained bank/previous-value layout and default leaves, and the owned image's link script, which binds `0x6439e0`, the six default leaves and the three backup callbacks | no | owned storage carries the data and defaults | required | unestablished | `//replacement/property_service:property-service-image` (Layout, 1 symbols, 0 definite references) |
| The property service carried twice: the stock-visible carrier at `0x62c120` (`property-service-public.ld`) and the `property-service-arm` rlib the owned menu adapters link | veneer or carrier at a stock address | stock callers of `0x62c120` | no | no stock caller enters the `0x62c120` carrier | required | unestablished | `//replacement/property_service:property-service-image` (Publication, 1 symbols, 0 definite references); `//replacement/property_service:property-service-linked-image` (Export, 2 symbols, 0 definite references); `//replacement/property_service:property-service-public-image` (Export, 2 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/property_service:property-service-image` (Publication, 2 symbols, 0 definite references); `//replacement/property_service:property-service-linked-image` (Publication, 2 symbols, 0 definite references); `//replacement/property_service:property-service-public-image` (Publication, 2 symbols, 0 definite references) |

## recipe_ui

The recipe-bank UI route entry points.

Classification: `module`. [Declarations](recipe_ui/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `rb_ui_route_page`, `rb_ui_route_slot`, `rb_ui_route_item` exports | stock-ABI export | stock bank route callers | no | the owned bank route calls them directly | required | unestablished | `//host/firmware:bank-elf-00c40000` (Export, 3 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (Export, 3 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (Export, 3 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (Export, 3 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Export, 3 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Export, 3 symbols, 0 definite references) |

## record_store

The owned record store core and guest adapter. Its shim list is [the record-store design](../reports/owned-record-store-design.md#7-shims).

Classification: `module`. [Declarations](record_store/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `record_store_*` exports | C-ABI bridge between two owned units | settings_storage and power_lifecycle | yes | both clients link the store directly and both sides compare with stock in one image | required | unestablished | `//replacement/record_store_carrier:record-store-image` (Export, 9 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-image` (Export, 9 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (Export, 9 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-image` (Export, 8 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (Export, 8 symbols, 0 definite references) |
| nine entry veneers at the stock wrapper addresses and `record_store_carrier/entry.S` | veneer or carrier at a stock address | stock callers of `0x6ac2b4`-family entries | no | every client calls the owned API | required | unestablished | `//replacement/record_store_carrier:record-store-image` (Publication, 1 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-image` (Publication, 1 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (Publication, 1 symbols, 0 definite references) |
| stock statuses, ignored write results and stock program lengths | stock-ABI export | stock clients that consume stock statuses and `0x600` kind-3 programs | no | clients consume typed results and the owned NAND partial-program law is qualified | required | unestablished | `//replacement/record_store_carrier:record-store-image` (Export, 1 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-image` (Export, 1 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (Export, 1 symbols, 0 definite references) |
| staging buffers at the stock addresses, handles at `0x89f8b4 + 12i` and the client words | guest-memory layout view | stock clients that read buffers, handles and client words | no | no stock client reads them | required | unestablished | `//replacement/record_store_carrier:record-store-image` (Layout, 1 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-image` (Layout, 1 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (Layout, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Layout, 2 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Layout, 2 symbols, 0 definite references); `//replacement/develop:develop-candidate` (Layout, 2 symbols, 0 definite references); `//replacement/property_service:property-service-image` (Layout, 2 symbols, 0 definite references); `//replacement/property_service:property-service-linked-image` (Layout, 2 symbols, 0 definite references); `//replacement/property_service:property-service-public-image` (Layout, 2 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-image` (Layout, 2 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (Layout, 2 symbols, 0 definite references); `//replacement/settings_storage:settings-storage-image` (Layout, 2 symbols, 0 definite references) |
| `record_raw_*`, `record_semaphore_*` externs | trampoline into retained stock | the stock raw NAND service and semaphore `0x4b` | no | the owned `NandFlash`/`FlashMap` and RTOS semaphore are qualified | required | unestablished | `//replacement/record_store_carrier:record-store-image` (Import, 13 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-image` (Import, 13 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (Import, 13 symbols, 0 definite references) |
| stock check/prepare callbacks of the five clients | callback registry for guest words | stock area callbacks | no | each client's seal and check are owned | required | unestablished | `//replacement/record_store_carrier:record-store-image` (RetainedEntry, 1 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-image` (RetainedEntry, 1 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (RetainedEntry, 1 symbols, 0 definite references) |

## record_store_carrier

The record-store carrier ELF: entry veneers and import stubs.

Classification: `linked`. [Declarations](record_store_carrier/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `record_store_entry_*` in `entry.S` and `record-store.ld` | veneer or carrier at a stock address | stock callers of the retired record-service span | no | every client enters the owned store directly | required | unestablished | `//replacement/record_store_carrier:record-store-image` (Publication, 9 symbols, 0 definite references) |
| `imports.ld` import stubs | trampoline into retained stock | the retained raw NAND and semaphore services | no | the owned NAND and RTOS services replace them | required | unestablished | `//replacement/record_store_carrier:record-store-image` (Import, 26 symbols, 0 definite references) |

## render_adapter

The render adapter that presents owned widget drawing at stock draw addresses.

Classification: `module`. [Declarations](render_adapter/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `rb_render_picker_apply`, `rb_render_tree_apply`, `rb_render_pane_apply` in `mod.rs`, with `entry.S` and `build.sh` placements | stock-ABI export | stock draw callers that enter the adapted widget entries | no | the owned renderer calls the widgets directly | required | unestablished | `//host/firmware:engine-elf-modal75-menu-engine-render` (Export, 3 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Export, 3 symbols, 0 definite references) |
| `rb_render_call_*` retained services (draw leaves, hooks, menu-table accessors) | trampoline into retained stock | the stock draw layer | no | the owned renderer owns the draw layer | required | unestablished | `//host/firmware:engine-elf-modal75-menu-engine-render` (RetainedEntry, 54 symbols, 0 definite references) |
| `rb_render_remainder` in `entry.S` | retained stock seam in assembly | the stock remainder path after an unhandled class | no | every class is owned | required | unestablished | `//host/firmware:engine-elf-modal75-menu-engine-render` (RetainedEntry, 1 symbols, 0 definite references) |

## render_commands

The address-independent command, raster and publisher model. It has no stock entry, but it classifies stock resource-catalog ids and layout.

Classification: `module`. [Declarations](render_commands/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| stock resource-catalog id classification (`src/resources.rs`, `classify_stock_resource`, ids `0xff000000..=0xff0008c5`) | guest-memory layout view | the retained stock resource catalog and bitmaps | no | an owned asset pack covers every owned screen | required | unestablished | `//host/firmware:engine-elf-modal75-menu-engine-render` (Layout, 1 symbols, 0 definite references) |
| stock fifty-unit text window and script/glyph layout in `src/raster.rs` | guest-memory layout view | retained stock font and script resources | no | owned fonts and scripts cover every owned screen | required | unestablished | `//host/firmware:engine-elf-modal75-menu-engine-render` (Layout, 1 symbols, 0 definite references) |

## root_task

The owned root task, calling 28 retained stock services.

Classification: `linked`. [Declarations](root_task/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| the root-task export in `src.rs`, placed by `root-task.ld` | veneer or carrier at a stock address | the stock root-task entry | no | the root task is entered from owned startup | required | unestablished | `//replacement/root_task:root-task-image` (Publication, 1 symbols, 0 definite references) |
| 28 `root_*` retained externs (checked init, cleanup, config query/write, resource/worker creation, descriptor/table install, platform/runtime init, periodic callback, later submission) | trampoline into retained stock | stock kernel, resource and configuration services | no | the RTOS platform and owned configuration own those services | required | unestablished | `//replacement/root_task:root-task-image` (Import, 29 symbols, 0 definite references) |
| `xm1_app_register_callback` retained service | callback registry for guest words | the stock callback registry | no | the owned application registration owns it | required | unestablished | `//replacement/root_task:root-task-image` (Import, 1 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/root_task:root-task-image` (Publication, 2 symbols, 0 definite references) |

## rtos_cyclic

The owned cyclic-handler family.

Classification: `linked`. [Declarations](rtos_cyclic/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_rtos_cyclic_*` exports placed at `0x4be3c`-family addresses by `cyclic-a.ld`/`cyclic-b.ld` | stock-ABI export | stock callers of the cyclic entries | no | every cyclic caller is owned or migrated | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Export, 24 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Export, 24 symbols, 0 definite references); `//replacement/rtos_cyclic:app-callback` (Export, 2 symbols, 0 definite references) |
| `app-callback.ld` app-callback carrier | veneer or carrier at a stock address | the stock domain-A app caller | no | the app caller is owned | required | unestablished | `//replacement/rtos_cyclic:app-callback` (Publication, 1 symbols, 0 definite references) |
| `xm1_rtos_cyclic_timer_insert`/`_remove` externs | C-ABI bridge between two owned units | rtos_timer | yes | the cyclic crate links the timer crate directly and both sides compare with stock in one image | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Import, 49 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 49 symbols, 0 definite references) |
| Definite retained references in the current linked instruction and relocation metadata | retained stock seam in assembly | retained code and guest address contracts reached by the composed image | partly | all declared retained targets have owned typed replacements and both sides are compared in the same image | required | unestablished | `//replacement/rtos_cyclic:app-callback` (RetainedEntry, 0 symbols, 2 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/rtos_cyclic:app-callback` (Publication, 3 symbols, 0 definite references) |

## rtos_data_queue

The owned data-queue family.

Classification: `linked`. [Declarations](rtos_data_queue/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_rtos_data_queue_*` exports placed by `dq-a.ld`/`dq-b.ld` at the stock entries | stock-ABI export | stock queue callers | no | every queue caller is owned or migrated | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Export, 33 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Export, 33 symbols, 0 definite references) |
| `xm1_rtos_scheduler_*` externs (acquire, insert, notify, prepare, release, remove, reset, save) | C-ABI bridge between two owned units | rtos_scheduler | yes | the queue crate links the scheduler crate directly and both sides compare with stock in one image | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Import, 49 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 49 symbols, 0 definite references) |
| `xm1_rtos_data_queue_stock_insert_timer`/`_remove_timer` externs | C-ABI bridge between two owned units | rtos_timer | yes | the queue crate links the timer crate directly and both sides compare with stock in one image | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Import, 1 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 1 symbols, 0 definite references) |

## rtos_exception

The owned exception entries, vectors and ack/EOI wrappers.

Classification: `linked`. [Declarations](rtos_exception/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_exception_*` vector slots, dispatcher, ack/EOI wrappers and data in `stage/carrier.S`, placed by `exception-a.ld`/`exception-b.ld` at the stock vector addresses | veneer or carrier at a stock address | the stock vector table and exception callers | no | the vector table and every exception entry are owned | required | unestablished | `//replacement/rtos_exception:exception-a` (Publication, 18 symbols, 0 definite references); `//replacement/rtos_exception:exception-a` (Import, 8 symbols, 0 definite references); `//replacement/rtos_exception:exception-b` (Publication, 18 symbols, 0 definite references); `//replacement/rtos_exception:exception-b` (Import, 8 symbols, 0 definite references) |
| `xm1_exception_stock_identity`, `_log`, `_release`, `_try` retained services | trampoline into retained stock | stock exception identity, logging and release services | no | those services are owned | required | unestablished | `//replacement/rtos_exception:exception-a` (Import, 8 symbols, 0 definite references); `//replacement/rtos_exception:exception-b` (Import, 1 symbols, 0 definite references); `//replacement/rtos_exception:exception-b` (Import, 8 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/rtos_exception:exception-a` (Publication, 5 symbols, 0 definite references); `//replacement/rtos_exception:exception-b` (Publication, 5 symbols, 0 definite references) |

## rtos_fixed_pool

The owned fixed-pool family.

Classification: `linked`. [Declarations](rtos_fixed_pool/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_rtos_fixed_pool_*` exports in `stage/entries.S`, placed by `fp-a.ld`/`fp-b.ld` | stock-ABI export | stock pool callers | no | every pool caller is owned or migrated | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Export, 21 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Export, 21 symbols, 0 definite references) |
| `xm1_fixed_pool_stock_acquire`, `_release`, `_notify`, `_dispatch` externs | C-ABI bridge between two owned units | rtos_scheduler | yes | the pool crate links the scheduler crate directly and both sides compare with stock in one image | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Import, 1 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 1 symbols, 0 definite references) |
| `xm1_fixed_pool_stock_wait_release`, `_wait_cancel`, `_wait_enqueue`, `_wait_enqueue_timed` externs at A `0x44568`/`0x44744`/`0x44800`/`0x448e4` (B `0xd4528`/`0xd4704`/`0xd47c0`/`0xd48a4`) | C-ABI bridge between two owned units | rtos_objects event-flag carriers | yes | the pool crate links the object crate directly and both sides compare with stock in one image | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Import, 1 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 1 symbols, 0 definite references) |
| `xm1_fixed_pool_stock_mask`, `_restore`, `_save_irq`, `_restore_irq`, `_uidivmod` externs | trampoline into retained stock | retained mask/restore and divide helpers | no | the RTOS platform and owned division own those helpers | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Import, 1 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 1 symbols, 0 definite references) |

## rtos_global_lock

The owned global lock.

Classification: `linked`. [Declarations](rtos_global_lock/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_rtos_global_lock_try`, `_release`, `_observe` and `helper_try`, `helper_release`, `helper_observe` exports placed at `0x412cc`, `0x412e8`, `0x41304`, `0x427d4`, `0x427fc`, `0x42828` | stock-ABI export | stock lock callers | no | every lock caller is owned or migrated | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Export, 12 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Export, 12 symbols, 0 definite references) |

## rtos_irq_control

The owned IRQ control and handler registration.

Classification: `linked`. [Declarations](rtos_irq_control/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_rtos_irq_*` exports placed at `0x41108`, `0x41124`, `0x4d8fc`-family addresses by `irq-a.ld`/`irq-b.ld` | stock-ABI export | stock IRQ callers | no | every IRQ caller is owned or migrated | required | unestablished | `//replacement/rtos_irq_control:irq-a` (Export, 6 symbols, 0 definite references); `//replacement/rtos_irq_control:irq-b` (Export, 6 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/rtos_irq_control:irq-a` (Publication, 1 symbols, 0 definite references); `//replacement/rtos_irq_control:irq-b` (Publication, 1 symbols, 0 definite references) |

## rtos_objects

The owned flag and mailbox family.

Classification: `linked`. [Declarations](rtos_objects/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_rtos_objects_*` exports placed by `family-a.ld`/`family-b.ld` at the stock entries | stock-ABI export | stock flag/mailbox callers | no | every object caller is owned or migrated | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Export, 26 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Export, 26 symbols, 0 definite references) |
| Thirteen event-flag carrier entries per domain in the aggregate `carriers.json` and `veneer-a.S`/`veneer-b.S`: eight flag services and the `family_release`, `helper_release` (`0x44744`/`0xd4704`), `helper_block`, `helper_block_timed` and `helper_check` bodies | veneer or carrier at a stock address | stock flag callers, and retained stock mailbox send, receive and timed receive that branch into the helper extents | no | every flag caller is owned or migrated | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Publication, 26 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Publication, 26 symbols, 0 definite references) |
| `xm1_rtos_scheduler_*` externs (acquire, insert, notify, prepare, release, remove, reset, save) | C-ABI bridge between two owned units | rtos_scheduler | yes | the object crate links the scheduler crate directly and both sides compare with stock in one image | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Import, 49 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 49 symbols, 0 definite references) |
| `xm1_rtos_objects_stock_insert_timer`/`_remove_timer` externs | C-ABI bridge between two owned units | rtos_timer | yes | the object crate links the timer crate directly and both sides compare with stock in one image | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Import, 1 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 1 symbols, 0 definite references) |
| `xm1_rtos_objects_stock_timeout` (`0x44318`/`0xd42d8`) | C-ABI bridge between two owned units | the owned scheduler timeout body behind the aggregate veneer | yes | the object crate names the owned timeout callback directly | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Import, 1 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 1 symbols, 0 definite references) |

## rtos_own_first

The 17 own-first root slots.

Classification: `linked`. [Declarations](rtos_own_first/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_rtos_own_first_slot_*` exports placed by `own-first-a.ld`/`own-first-b.ld` at the stock root-slot addresses | stock-ABI export | stock root callers that enter the 17 slots | no | every root-slot caller is owned or migrated | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Export, 35 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Export, 35 symbols, 0 definite references) |
| root descriptor and task-record views used by the slots (`src/lib.rs`) | kernel interface slot | the stock kernel root descriptors and task records | no | the RTOS platform owns the root services | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Layout, 34 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Layout, 34 symbols, 0 definite references) |

## rtos_platform

The platform aggregate: 126 veneer entries per domain and 60 externals per domain declared in `carriers.json`, with `veneer-a.S`/`veneer-b.S` generated from it.

Classification: `linked`. [Declarations](rtos_platform/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 49 `kind: owned` externals per domain in `carriers.json` (scheduler/timer calls into other RTOS family units, plus the event-flag wait helpers) | C-ABI bridge between two owned units | rtos_scheduler, rtos_timer, rtos_objects and the family units | yes | the aggregate links the family crates directly and both sides compare with stock in one image | required | unestablished | `//replacement/rtos_platform:platform-a` (Import, 49 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 49 symbols, 0 definite references) |
| 11 `kind: retained` externals per domain in `carriers.json` (stock IRQ raise/clear, mask/restore, divide helpers) | trampoline into retained stock | stock IRQ, mask/restore and divide helpers | no | the RTOS platform owns those helpers | required | unestablished | `//replacement/rtos_platform:platform-a` (Import, 60 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 60 symbols, 0 definite references) |
| 126 veneer entries per domain in `veneer-a.S`/`veneer-b.S`, placed by `platform-a.ld`/`platform-b.ld` | veneer or carrier at a stock address | stock callers of the replaced root slots | no | every root-slot caller is owned or migrated | required | unestablished | `//replacement/rtos_platform:platform-a` (Publication, 252 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Publication, 252 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/rtos_platform:platform-a` (Publication, 7 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Publication, 7 symbols, 0 definite references) |

## rtos_scheduler

The owned scheduler groups S1, S2, S3a-S3d.

Classification: `linked`. [Declarations](rtos_scheduler/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| S1/S2/S3 stock-ABI exports placed at the stock entry addresses by the `s3*-a.ld`/`s3*-b.ld` and `scheduler-*.ld` scripts | stock-ABI export | stock scheduler, task and exception callers | no | every scheduler caller is owned or migrated | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Export, 105 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Export, 105 symbols, 0 definite references) |
| `s3a_stock_*`, `s3b_*`, `s3c_*`, `s3d_*` externs marked `kind: owned` in `carriers.json` (insert, remove, notify, switch, timer insert/remove, and `s3b_release_wait` at `0x44568`/`0xd4528` bound to the event-flag family release carrier) | C-ABI bridge between two owned units | the scheduler's own S1/timer groups, rtos_objects and the RTOS family units | yes | the groups link as Rust items and both sides compare with stock in one image | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Import, 23 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 23 symbols, 0 definite references); `//replacement/rtos_platform:platform-a` (Export, 8 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Export, 8 symbols, 0 definite references) |
| `xm1_rtos_scheduler_stock_raise_irq` and `xm1_rtos_scheduler_stock_raise_irq`-family retained externs | trampoline into retained stock | the stock IRQ raise helper | no | the RTOS platform owns IRQ raising | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Import, 1 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 1 symbols, 0 definite references) |

## rtos_semaphore

The owned semaphore family.

Classification: `linked`. [Declarations](rtos_semaphore/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_rtos_semaphore_*` exports placed by `sem-a.ld`/`sem-b.ld` at the stock entries | stock-ABI export | stock semaphore callers | no | every semaphore caller is owned or migrated | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Export, 19 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Export, 19 symbols, 0 definite references) |
| `xm1_semaphore_stock_acquire`, `_release`, `_notify`, `_dispatch` externs | C-ABI bridge between two owned units | rtos_scheduler | yes | the semaphore crate links the scheduler crate directly and both sides compare with stock in one image | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Import, 1 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 1 symbols, 0 definite references) |
| `xm1_semaphore_stock_wait_release`, `_wait_cancel`, `_wait_enqueue`, `_wait_enqueue_timed` externs at A `0x44568`/`0x44744`/`0x44800`/`0x448e4` (B `0xd4528`/`0xd4704`/`0xd47c0`/`0xd48a4`) | C-ABI bridge between two owned units | rtos_objects event-flag carriers | yes | the semaphore crate links the object crate directly and both sides compare with stock in one image | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Import, 1 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 1 symbols, 0 definite references) |
| `xm1_semaphore_stock_mask`, `_restore` externs | trampoline into retained stock | retained mask/restore helpers | no | the RTOS platform owns those helpers | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Import, 1 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 1 symbols, 0 definite references) |

## rtos_task_records

The owned task-record create/delete entries.

Classification: `linked`. [Declarations](rtos_task_records/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_rtos_task_create`/`xm1_rtos_task_delete` exports placed by `task-records-a-native.ld`/`task-records-b-native.ld` | stock-ABI export | stock task-creation callers | no | every task caller is owned or migrated | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Export, 6 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Export, 6 symbols, 0 definite references) |
| `xm1_task_mask_save`/`xm1_task_mask_restore` externs | trampoline into retained stock | the stock mask helpers | no | the RTOS platform owns mask handling | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Import, 2 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Import, 2 symbols, 0 definite references) |

## rtos_timer

The owned timer heap.

Classification: `linked`. [Declarations](rtos_timer/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_rtos_timer_initialize`, `_insert`, `_remove`, `_tick` exports placed by `timer-a.ld`/`timer-b.ld` at `0x44a50`-family addresses | stock-ABI export | stock timer callers | no | every timer caller is owned or migrated | required | unestablished | reference-only: Standalone family qualification images are frozen reference artifacts; the current linked boundary is the platform aggregate, not a rebuilt standalone family image.; `//replacement/rtos_platform:platform-a` (Export, 12 symbols, 0 definite references); `//replacement/rtos_platform:platform-b` (Export, 12 symbols, 0 definite references) |

## saclac

The task 43 engine: the owned dispatcher and machine 0's router over stock's 37 machine tables, entered at the two stock extents they retire. No machine is owned yet, so every action is a stock action word.

Classification: `linked`. [Declarations](saclac/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| the two image-5 extents `[0xefca6c,0xefcb88)` and `[0xefcb88,0xefd2fc)`, 1,164 owned bytes of the 2,192-byte pair, written by `stage-saclac` through record-5 staging, with the entry veneers `xm1_saclac_dispatch_entry` and `xm1_saclac_router_entry` from `entry.S` placed at `0xefca6c` and `0xefcb88` by the linker script generated from `Replacement.Saclac.Artifact.extents` and the remaining bytes the trap word `0xe7f000f0` | veneer or carrier at a stock address | the stock actor at `0xf01bc8`, the 36 stock row functions that call the dispatcher, and the nested row calls from actions | no | every machine owned | required | unestablished | `//replacement/saclac:saclac-image` (Publication, 2 symbols, 0 definite references); `//replacement/saclac:saclac-image` (Export, 2 symbols, 0 definite references) |
| `StockTables`, `StockPorts` and the guest addresses in `src/stock.rs`, with the four per-machine count arrays `SACLAC_CELL_COUNTS`, `SACLAC_ACTION_COUNTS`, `SACLAC_SECOND_COUNTS` and `SACLAC_ROW_COUNTS` and machine 0's row map `SACLAC_ROW_MAP` exported for the layout check | guest-memory layout view | the cell, action and second tables at `0xf12f70`, `0xf133ec` and `0xf12074`, the state counts at `0xf1158c`, and the two halfword arrays | no | every machine owned | required | unestablished | `//replacement/saclac:saclac-image` (Layout, 5 symbols, 0 definite references) |
| the trace entry `0xf46c38` called through the literal word at `0xefcce4`, declared in `Replacement.Saclac.Artifact.retainedExternals` | trampoline into retained stock | the stock trace body at `0xf46c38` | no | the trace recorder is owned | required | unestablished | `//replacement/saclac:saclac-image` (RetainedEntry, 1 symbols, 1 definite references) |
| stock action words and second-table words entered through `StockCodeWord` with r2 = the machine's table base word and r3 = the entered word, the values stock's dispatcher holds at `0xefcb20` and `0xefcb58` | trampoline into retained stock | the 813 distinct action words the cells reference, all image-5 code addresses, of which 701 are declared function starts and 112 are entries the decomp does not declare; no cell selects a second-table word; no entry reads r2, and 224 save r3 in a store-multiple prologue, the only r3 read the walk finds | no | every machine owned | required | unestablished | `//replacement/saclac:saclac-image` (RetainedEntry, 1 symbols, 0 definite references) |

## saclac_interface



Classification: `module`. [Declarations](saclac_interface/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Command, parameter-block and callback descriptors in `src/stock.rs` | guest-memory layout view | retained queue-43 command producers, their 46 shared parameter blocks and 106 completion callbacks | no | queue-43 producers and completion callbacks are owned and no retained consumer reads the parameter blocks | required | unestablished | `//replacement/saclac:saclac-image` (Layout, 1 symbols, 0 definite references) |

## save

The save facade and its retained image-5 services.

Classification: `linked`. [Declarations](save/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_save_facade`, `xm1_save_completion`, `xm1_save_release_completion`, `svc_*` exports in `src/lib.rs`/`save_entry.S` | stock-ABI export | stock save callers | no | every save caller is owned or migrated | required | unestablished | `//replacement/develop:develop-candidate` (Export, 3 symbols, 0 definite references); `//replacement/save:save-owned` (Export, 3 symbols, 0 definite references) |
| `xm1_save_facade` export | C-ABI bridge between two owned units | develop | yes | develop links the facade directly and both sides compare with stock in one image | required | unestablished | `//replacement/develop:develop-candidate` (Export, 1 symbols, 0 definite references); `//replacement/save:save-owned` (Export, 1 symbols, 0 definite references) |
| `rb_save_*` retained services in `save_stock.S` (current task, extent copy/name/preset, prepare metadata, register release, cached state, service byte/present/profile, signal, stage, storage direct/kind-three, wait) | trampoline into retained stock | stock image-5 save and storage services | no | those services are owned | required | unestablished | `//replacement/develop:develop-candidate` (RetainedEntry, 15 symbols, 0 definite references); `//replacement/save:save-owned` (RetainedEntry, 15 symbols, 0 definite references) |
| `save.ld`, `stage.ld`, `storage.ld`, `wait.ld` placements and the `xm1_save_*_probe` entries | veneer or carrier at a stock address | stock save callers and the qualification probes | no | owned code is called directly | required | unestablished | `//replacement/save:save-owned` (Publication, 1 symbols, 0 definite references) |
| Additional linked unmangled ABI entry points | stock-ABI export | the publication or qualification consumers of the current linked image | partly | all consumers use owned typed interfaces and both sides have a qualified comparison in the same image | required | unestablished | `//replacement/save:save-owned` (Export, 2 symbols, 0 definite references) |
| Definite retained references in the current linked instruction and relocation metadata | retained stock seam in assembly | retained code and guest address contracts reached by the composed image | partly | all declared retained targets have owned typed replacements and both sides are compared in the same image | required | unestablished | `//replacement/save:save-owned` (RetainedEntry, 0 symbols, 15 definite references) |

## secondary_stage

The owned record-2 startup.

Classification: `linked`. [Declarations](secondary_stage/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_secondary_entry`, `xm1_secondary_handoff`, the handoff vector and the log read/store in `entry.S`/`src.rs` | veneer or carrier at a stock address | the stock record-2 vector and handoff | no | the record-2 vector and handoff are owned | required | unestablished | `//replacement/critical_pair:critical-pair-image` (Publication, 2 symbols, 0 definite references); `//replacement/interface_initializer:interface-initializer-image` (Publication, 2 symbols, 0 definite references); `//replacement/loader_prefix:loader-prefix-image` (Publication, 2 symbols, 0 definite references); `//replacement/secondary_stage:secondary-stage-image` (Publication, 2 symbols, 0 definite references); `//replacement/critical_pair:critical-pair-image` (Export, 4 symbols, 0 definite references); `//replacement/interface_initializer:interface-initializer-image` (Export, 4 symbols, 0 definite references); `//replacement/loader_prefix:loader-prefix-image` (Export, 4 symbols, 0 definite references); `//replacement/secondary_stage:secondary-stage-image` (Export, 4 symbols, 0 definite references) |
| `xm1_retained_companion_*`, `xm1_retained_config_record`, `xm1_retained_conversion`, `xm1_retained_delay`, `xm1_retained_peripheral_*`, `xm1_retained_segment_lookup`, `xm1_retained_structure_init`, `xm1_secondary_log_timestamp` | trampoline into retained stock | retained companion, configuration and peripheral services | no | those services are owned | required | unestablished | `//replacement/critical_pair:critical-pair-image` (RetainedEntry, 1 symbols, 0 definite references); `//replacement/critical_pair:critical-pair-image` (Import, 9 symbols, 0 definite references); `//replacement/interface_initializer:interface-initializer-image` (RetainedEntry, 1 symbols, 0 definite references); `//replacement/interface_initializer:interface-initializer-image` (Import, 9 symbols, 0 definite references); `//replacement/loader_prefix:loader-prefix-image` (RetainedEntry, 1 symbols, 0 definite references); `//replacement/loader_prefix:loader-prefix-image` (Import, 9 symbols, 0 definite references); `//replacement/secondary_stage:secondary-stage-image` (RetainedEntry, 1 symbols, 0 definite references); `//replacement/secondary_stage:secondary-stage-image` (Import, 42 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/secondary_stage:secondary-stage-image` (Publication, 5 symbols, 0 definite references) |

## service

The retained row-availability probe binding.

Classification: `module`. [Declarations](service/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `rb_service_field_row_state` export | stock-ABI export | stock menu callers of the row probe | no | the menu engine declares availability itself | required | unestablished | `//host/firmware:bank-elf-00c40000` (Export, 1 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (Export, 1 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (Export, 1 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (Export, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Export, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Export, 1 symbols, 0 definite references) |
| `rb_call_stock_probe` in `rb_service_stock.S` | trampoline into retained stock | the stock probe `FUN_00d52684` | no | the probe is owned | required | unestablished | `//host/firmware:bank-elf-00c40000` (RetainedEntry, 1 symbols, 0 definite references); `//host/firmware:bank-elf-00c4fc88` (RetainedEntry, 1 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c40000` (RetainedEntry, 1 symbols, 0 definite references); `//host/firmware:bank-modal-elf-00c4fc88` (RetainedEntry, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (RetainedEntry, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (RetainedEntry, 1 symbols, 0 definite references) |

## settings_storage

The owned settings storage, bank projections, custom record lifecycle and header mirror.

Classification: `linked`. [Declarations](settings_storage/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `record_store_append`/`record_store_register` externs | C-ABI bridge between two owned units | record_store | yes | settings links the store directly and both sides compare with stock in one image | required | unestablished | `//replacement/settings_storage:custom-settings-image` (Export, 2 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (Export, 2 symbols, 0 definite references) |
| `xm1_storage_output8`/`_output16`/`_status32` externs | C-ABI bridge between two owned units | property_service | yes | settings links the property service directly and both sides compare with stock in one image | required | unestablished | `//replacement/settings_storage:settings-storage-image` (RetainedEntry, 1 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Export, 3 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Export, 3 symbols, 0 definite references); `//replacement/property_service:property-service-image` (Export, 3 symbols, 0 definite references); `//replacement/property_service:property-service-linked-image` (Export, 3 symbols, 0 definite references); `//replacement/property_service:property-service-public-image` (Export, 3 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-image` (Export, 3 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (Export, 3 symbols, 0 definite references); `//replacement/settings_storage:settings-storage-image` (Export, 3 symbols, 0 definite references) |
| `xm1_getter_owned`, `xm1_setter_owned`, `xm1_storage_initialize` exports | C-ABI bridge between two owned units | power_lifecycle | yes | power links the settings crate directly and both sides compare with stock in one image | required | unestablished | `//replacement/settings_storage:custom-settings-image` (Export, 1 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (Export, 1 symbols, 0 definite references); `//replacement/settings_storage:settings-storage-image` (Export, 26 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine` (Export, 6 symbols, 0 definite references); `//host/firmware:engine-elf-modal75-menu-engine-render` (Export, 6 symbols, 0 definite references); `//replacement/property_service:property-service-image` (Export, 6 symbols, 0 definite references); `//replacement/property_service:property-service-linked-image` (Export, 6 symbols, 0 definite references); `//replacement/property_service:property-service-public-image` (Export, 6 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-image` (Export, 17 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (Export, 17 symbols, 0 definite references) |
| `settings_bank_*` and `settings_custom_*` entries in `bank_entry.S`/`custom_entry.S`, placed by `settings-storage.ld`/`custom-settings.ld` | veneer or carrier at a stock address | stock bank/custom callers | no | every bank/custom caller is owned or migrated | required | unestablished | `//replacement/settings_storage:custom-settings-image` (Publication, 1 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (Publication, 1 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-image` (Export, 30 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (Export, 30 symbols, 0 definite references) |
| 25 retained `xm1_stock_*`, `xm1_bank_*`, `xm1_storage_*` and `settings_custom_*` externs | trampoline into retained stock | stock bank, header, storage and custom-record services | no | owned storage replaces those services | required | unestablished | `//replacement/settings_storage:settings-storage-image` (RetainedEntry, 1 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-image` (Import, 44 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (Import, 45 symbols, 0 definite references); `//replacement/settings_storage:settings-storage-image` (Import, 16 symbols, 0 definite references) |
| header mirror, previous-value snapshot `0x89ec00`, backup memory `0x409c281c` and the 16-byte shadow `0x409c29b8` | guest-memory layout view | retained stock readers and the bank-0 backup contract | no | no stock code reads the mirror or backup | required | unestablished | `//replacement/settings_storage:settings-storage-image` (Layout, 1 symbols, 0 definite references) |
| Definite retained references in the current linked instruction and relocation metadata | retained stock seam in assembly | retained code and guest address contracts reached by the composed image | partly | all declared retained targets have owned typed replacements and both sides are compared in the same image | required | unestablished | `//replacement/settings_storage:custom-settings-image` (RetainedEntry, 0 symbols, 1 definite references); `//replacement/settings_storage:settings-storage-image` (RetainedEntry, 0 symbols, 13 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/settings_storage:custom-settings-image` (Publication, 1 symbols, 0 definite references); `//replacement/settings_storage:custom-settings-published-image` (Publication, 8 symbols, 0 definite references) |

## startup_dispatch

The phase dispatcher.

Classification: `linked`. [Declarations](startup_dispatch/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_startup_dispatch` and `xm1_startup_dispatch_entry` in `entry.S`/`src.rs`, placed by `startup-dispatch.ld` | veneer or carrier at a stock address | the stock phase entry at `0x69cdf8` | no | the phase entry is owned | required | unestablished | `//replacement/startup_dispatch:startup-dispatch-image` (Publication, 2 symbols, 0 definite references) |
| `xm1_startup_clear`, `xm1_startup_install`, `xm1_startup_mode`, `xm1_startup_submit` retained services | trampoline into retained stock | stock startup clear/install/mode/submit services | no | those services are owned | required | unestablished | `//replacement/startup_dispatch:startup-dispatch-image` (Import, 4 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/startup_dispatch:startup-dispatch-image` (Publication, 2 symbols, 0 definite references) |

## task1_actor

The owned task1 UI actor and its pump.

Classification: `linked`. [Declarations](task1_actor/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| the actor export and `task1-actor.ld` placement | veneer or carrier at a stock address | the stock task1 entry at `0xc22f60` | no | the task1 entry is owned | required | unestablished | `//replacement/task1_actor:task1-actor-image` (Publication, 1 symbols, 0 definite references); `//replacement/task1_actor:task1-actor-image` (Export, 1 symbols, 0 definite references) |
| 29 retained `extern "C"` declarations (queue receive/service, pending predicates, controller setup, pump, registration, diagnostics) | trampoline into retained stock | stock image-4/image-5 application and queue services | no | those services are owned | required | unestablished | `//replacement/task1_actor:task1-actor-image` (Import, 30 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/task1_actor:task1-actor-image` (Publication, 19 symbols, 0 definite references) |

## task_constructors

The owned task constructors.

Classification: `linked`. [Declarations](task_constructors/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 13 `xm1_ctor_*` entries in `entry.S`, placed by `task-constructors.ld` at the stock constructor addresses | veneer or carrier at a stock address | the stock constructor caller list | no | the constructor list is owned | required | unestablished | `//replacement/task_constructors:task-constructors-image` (Publication, 13 symbols, 0 definite references); `//replacement/task_constructors:task-constructors-image` (Export, 13 symbols, 0 definite references) |
| 21 `xm1_sac_*`, `xm1_create_queue`, `xm1_record_*`, `xm1_submit_task`, `xm1_usb_clear`, `xm1_diag_initialize` retained services | trampoline into retained stock | stock subsystem constructors | no | those subsystems are owned | required | unestablished | `//replacement/task_constructors:task-constructors-image` (Import, 33 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/task_constructors:task-constructors-image` (Publication, 39 symbols, 0 definite references) |

## task_storage

The owned task-storage transition.

Classification: `linked`. [Declarations](task_storage/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_task_storage_entry` and `xm1_task_storage_transition` in `entry.S`/`src.rs`, placed by `task-storage.ld` | veneer or carrier at a stock address | the stock task-storage caller | no | the caller is owned | required | unestablished | `//replacement/task_storage:task-storage-image` (Publication, 2 symbols, 0 definite references); `//replacement/task_storage:task-storage-image` (Import, 2 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/task_storage:task-storage-image` (Publication, 2 symbols, 0 definite references) |

## task_submission

The owned task submission and release wrappers.

Classification: `linked`. [Declarations](task_submission/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_task_submit_entry`, `xm1_task_release_entry` in `entry.S` and `xm1_task_submit`/`xm1_task_release` in `src.rs` | veneer or carrier at a stock address | the stock submit/release callers | no | every caller is owned or migrated | required | unestablished | `//replacement/task_submission:task-submission-image` (Publication, 4 symbols, 0 definite references); `//replacement/task_submission:task-submission-image` (Layout, 1 symbols, 0 definite references) |
| `xm1_task_submit`/`xm1_task_release` exports | C-ABI bridge between two owned units | power_lifecycle | yes | power links the submission crate directly and both sides compare with stock in one image | required | unestablished | `//replacement/task_submission:task-submission-image` (Export, 2 symbols, 0 definite references) |
| `xm1_task_cache`, `xm1_task_critical_enter/exit`, `xm1_task_error_callback_dispatch`, `xm1_task_fill`, `xm1_task_log`, `xm1_task_reservation`, `xm1_task_try_lock`, `xm1_task_unlock` retained services | trampoline into retained stock | stock kernel and cache services | no | the RTOS platform owns those services | required | unestablished | `//replacement/task_submission:task-submission-image` (Import, 15 symbols, 0 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/task_submission:task-submission-image` (Publication, 4 symbols, 0 definite references) |

## time_services

`time_services` owns tasks 6 and 9, software timers, timed delays and interrupt registration. Its interface entries and their owned symbols are declared in `time_services/stock-interface.json`; `time_services:artifact` derives publications from those rows.

Classification: `linked`. [Declarations](time_services/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Published time entries in `time_services/src/stock.rs` | stock-ABI export | retained callers and callback words entering the entries in `stock-interface.json` | partly | no retained branch, table word or executed computed reference enters the published stock entries | required | unestablished | `//replacement/time_services:image` (Export, 22 symbols, 0 definite references); `//replacement/time_services:image` (Export, 7 symbols, 0 definite references) |
| Handle allocation in `time_services/src/stock.rs` | trampoline into retained stock | allocator 0x65722c, resolver 0x657384 and release 0x65744c | no | the peripherals owner supplies typed handle allocation to the linked image | required | unestablished | `//replacement/time_services:image` (RetainedEntry, 1 symbols, 3 definite references); `//replacement/time_services:image` (Import, 3 symbols, 0 definite references) |
| Device timer programming in `time_services/src/stock.rs` | trampoline into retained stock | acquire 0x655b28, configure 0x655ccc, stop 0x655fe8 and release 0x655c24 | no | device timer programming is owned in the linked image | required | unestablished | `//replacement/time_services:image` (RetainedEntry, 1 symbols, 4 definite references) |
| Interrupt backend in `time_services/src/stock.rs` | trampoline into retained stock | interrupt configure 0x723870 and detach 0x7239a0 | no | the kernel crate exposes the owned interrupt backend in the linked image | required | unestablished | `//replacement/time_services:image` (RetainedEntry, 1 symbols, 2 definite references) |
| Diagnostic and callback words in `time_services/src/stock.rs` | trampoline into retained stock | diagnostic 0x726c74 and callbacks supplied by retained consumers | partly | every supplying consumer and the diagnostic service are owned in the linked image | required | unestablished | `//replacement/time_services:image` (RetainedEntry, 1 symbols, 1 definite references) |
| Stock memory in `time_services/src/stock.rs` | guest-memory layout view | interrupt handles and sources 0x940e9c, delay and timer descriptors 0x95dc08 and 0x96bd40, tick count 0x96e97c, timer handle table 0x95aa9c, task arguments 0x89f76c | partly | no retained code reads these words and the retained allocator no longer holds the handle tables | required | unestablished | `//replacement/time_services:image` (Layout, 1 symbols, 0 definite references); `//replacement/time_services:image` (Layout, 1 symbols, 0 definite references) |
| Definite retained references in the current linked instruction and relocation metadata | retained stock seam in assembly | retained code and guest address contracts reached by the composed image | partly | all declared retained targets have owned typed replacements and both sides are compared in the same image | required | unestablished | `//replacement/time_services:image` (RetainedEntry, 0 symbols, 3 definite references) |
| Global untyped assembly entries and linker publication labels | veneer or carrier at a stock address | retained entry, compiler ABI and image-layout consumers | partly | no consumer requires these published ABI or layout labels and both sides have stock comparisons in the same image | required | unestablished | `//replacement/time_services:image` (Publication, 4 symbols, 0 definite references) |

## validator

The area-3 record validator replacement and its stock-ABI adapter.

Classification: `linked`. [Declarations](validator/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `area3_validate` export in `validator.rs`/`faulty.rs`, placed by `validator.ld` | stock-ABI export | the stock area-3 check callback address | no | the record store calls the Rust validator directly | required | unestablished | `//replacement/validator:area3-arm` (Export, 1 symbols, 0 definite references) |
| `adapter.rs` host C-ABI shim | stock-ABI export | the native qualification adapter | no | the adapter is no longer needed for qualification | required | unestablished | reference-only: The registered mechanism remains required; this reference or qualification-only mechanism is not part of the selected linked image. `adapter.rs` host C-ABI shim |

## vendor_protocol

The vendor, wireless, filesystem and device-protocol unit. Most of its externals are owned units; the remaining seam is the stock USB/wireless and filesystem surface.

Classification: `linked`. [Declarations](vendor_protocol/shims.json).

| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xm1_app_nand_*`, `xm1_power_*`, `xm1_scanout_*`, `xm1_rtos_own_first_slot_*` externs (owned units) | C-ABI bridge between two owned units | application_nand, power_lifecycle, display_scanout, rtos_own_first | yes | each caller links the owning crate directly and both sides compare with stock in one image | required | unestablished | reference-only: The registered mechanism belongs to the component/runtime composition, not the bounded session-native ELF. No produced production ELF establishes this boundary. `xm1_app_nand_*`, `xm1_power_*`, `xm1_scanout_*`, `xm1_rtos_own_first_slot_*` externs (owned units) |
| `vendor_session_initialize`, `vendor_session_control`, `vendor_session_context_size`, `vendor_session_context_alignment`, `xm1_vendor_usb_task`, `xm1_vendor_usb_interrupt` exports | stock-ABI export | stock USB/session callers | no | every USB/session caller is owned or migrated | required | unestablished | reference-only: USB task and interrupt exports remain component declarations without an existing produced production ELF.; `//replacement/vendor_protocol:session-native` (RetainedEntry, 1 symbols, 0 definite references) |
| `xm1_wireless_driver_task`, `xm1_wireless_supplicant_task` retained services | trampoline into retained stock | stock wireless driver and supplicant tasks | no | the wireless subsystem is owned | required | unestablished | reference-only: The registered mechanism belongs to the component/runtime composition, not the bounded session-native ELF. No produced production ELF establishes this boundary. `xm1_wireless_driver_task`, `xm1_wireless_supplicant_task` retained services |
| filesystem and media-task ABI views (`filesystem_*`, `media_parser`, `metadata_*`) | guest-memory layout view | retained stock filesystem, media and metadata services | no | the owned file service and filesystem are qualified | required | unestablished | reference-only: The registered mechanism belongs to the component/runtime composition, not the bounded session-native ELF. No produced production ELF establishes this boundary. filesystem and media-task ABI views (`filesystem_*`, `media_parser`, `metadata_*`) |

