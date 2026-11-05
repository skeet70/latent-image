# Display/render: stock map, target design and first increment

Ledger: `E-DISPLAY-RENDER-TARGET-DESIGN-150`. Project: the display/render service of [#127](https://github.com/skeet70/latent-image/issues/127) ("own render actors/commands, buffer lifetimes and acknowledged complete frame publication over a grounded scanout/device backend"), organized per [spec/11 ownership stages](../spec/11-incremental-firmware-replacement.md#ownership-stages). Host only: no emulator or camera run was made for this report.

Sources: the render-service manifest ([draft](render-service-manifest-draft.md), `E-RENDER-SERVICE-MANIFEST-150`), the x70view decompilation at commit `a72c95a3572562fc05f359b4f35915f3a0243b19` (branch `decomp/xm1-coverage`; evidence rules in [xm1-decomp.md](xm1-decomp.md)), the image4 and plain-image listings (`private-data/analysis/x-m1-1.50/lz/id4.dis`, `decode/img6.dis`), the [device-fidelity audit](emulator-device-fidelity-audit.md), the emulator's render gate and comparator, and three RAM dumps already held: body Run-3 (`E-BULK-READ-RUN3-150`) and the FujiMod and retained calibration dumps (`E-RAM-CALIBRATION-150`).

Status words used for each stock fact: **exact** (exact C at the pinned commit, `source-inspected`), **bytes** (NONMATCHING C or a scout reading confirmed against the listing at the cited addresses), **draft** (NONMATCHING C not confirmed in bytes; a reading aid only), **dump** (read host-only from a held RAM dump). Function names are the decomp's donor-derived names and carry no evidential weight. **[INFERENCE]** marks what the evidence does not show directly.

## 1. Stock subsystem

### 1.1 Actors and entry points

| Unit | Entry | Role | Status |
| --- | --- | --- | --- |
| UILIB task 33 | `ov44_event_task` `0x1109130`, step `ov44_receive_event` `0x11090a4` | Receives queue-33 requests, filters, dispatches | exact |
| UILIB filter | `ov44_event_is_stale` `0x1109018` | Drops a stale family-`0x4000` request (manifest rule) | NONMATCHING; rule from the manifest's byte reading |
| UILIB dispatcher | `ov44_dispatch_event` `0x1108a2c` | Splits family and index; `0x4000` codes 1..`0x175` go to `ov44_handle_draw_event` `0x110871c` | draft (routing); handler call bytes below |
| Screen handler call | `ov44_handle_draw_event` `0x110871c` | `blx` through the 8-byte `{handler, flags}` row at `0x110f9f8 + (code-1)*8` | bytes `11089c8-11089d8` |
| Completion events | `ov44_post_event_notifications` `0x1108350` | Maps a finished `0x4000`/`0x5000` code to event records posted through `dispatch_device_notification` `0x72b680` | draft; `0x72b680` exact |
| Main UI sender | `FUN_00d38d28`, `FUN_0105e7ac` -> `FUN_00db68dc` -> `0xdb673c`/`0xdb6804` -> `0xdb6464` (outer code `0x3100`) | Maps UI events to render commands and sends to queue 33 | both entry gates exact; `0xdb68dc` and below NONMATCHING (manifest byte reading) |
| UILIB kick | `overlay_notify_resource_draw_completion` `0x112e380` | Family-`0x8000` request, 2590 call sites | exact |
| Graphic task 40 | `graphic_task` `0x6d2700`, constructor `graphic_create_task` `0x6d2230` | Layout commands `0x3800..0x3806`, relays `0x3804` as a VGLIB render | task draft, constructor exact |
| VGLIB task 73 | `vglib_task` `0x6d7540`, constructor `vglib_create_task` `0x6d7f88`; render step `FUN_006d7920` | `0x5900` open, `0x5901` close, `0x5902` render frame | task draft, constructor exact, render step bytes (below) |
| VGLIB client | `vglib_schedule_animation` `0x6dcb60(slot, callback)` | Posts a slot callback and a `0x5902` request | draft (manifest byte reading) |
| Vector-engine service | tasks 71/72: `graphics_service_payload_messages` `0x892184`, `graphics_poll_completions` `0x892110`, `graphics_start_event_tasks` `0x892334`; ring `0x892eb8`, IRQ handler `0x8924c0` | OpenVG job submission and completion | 0x892110 and 0x892334 exact; the rest draft |
| OpenVG submission | `graphics_send_resource_signal` `0x892054` | Sends command `0x5700` to task `0x47` (71), called from `vgc_submit_*` `0x8551c0..0x855620` and `0x870a80` | exact; call sites bytes |
| Display driver | `display_save_context` `0x6c926c`, commit `FUN_006ca3e0`, engine wrappers `FUN_006cb0c4` / `FUN_006cb0d8` | Shadow-to-register commit per frame interrupt | wrappers exact; commit and save NONMATCHING, offsets bytes |
| Display scheduler | `FUN_006c7318` with ISRs `FUN_006c70e4` / `FUN_006c6eb0` | Persistent and one-shot per-frame callbacks | draft; source choice bytes |
| Row probe | `ui_setting_availability` `0xd52684` | Row state for the menu engine | exact (24,632 bytes) |

The availability probe is in the render manifest but belongs to the settings and menu tree side: the menu engine already holds it as the shim "Stock availability `0xd52684`" in [owned-menu-engine-design.md](owned-menu-engine-design.md#shims-for-this-unit). It is not part of this project's target.

### 1.2 Draw command flow for a menu screen

1. The menu engine updates the block `0x009571d0..0x00957600` and the 44-byte snapshot areas (menu-engine design). Draw code reads snapshots through `get_current_menu_snapshot` `0x0061cd14` (exact) or `ui_menu_copy_saved_report` `0x0061ce1c` (exact). `ui_update_selected_menu_snapshot` `0xd4c8d4` (draft) caches menu state for UI events `0x816`/`0x837`; it is not draw code.
2. A UI event reaches `FUN_00db68dc` through the exact gates `0xd38d28`/`0x105e7ac`, which drop it when `0x944580+0xbe5c` is nonzero or `+0xfcf8 == 1`, and which drop codes up to `0x9ff` while `+0xbed4 == 1`. `0xdb68dc` maps it to a `0x4xxx` or `0x5xxx` request on queue 33 (manifest). The path from a stage event to the specific redraw request is not traced here.
3. UILIB dispatches a `0x4000 | code` request to the screen table. Menu screen codes whose handler words are confirmed in the plain-image table and whose handler-to-draw `bl` sites are confirmed in bytes include `0x3a`, `0x3c`, `0x57`, `0xe8`, `0x100..0x102`, `0x108..0x10a`, `0x10c`, `0x10d`, `0x10f`, `0x128..0x12b`, `0x12d`, `0x12f`, `0x133..0x136`, `0x140`, `0x141` and `0x143`; screen `0x12e` (`FUN_011241f8`, exact) draws a tree through `FUN_0116bff4(0x4a)`. The handler-to-name mapping of menu items is unread.
4. Handlers call small exact wrappers in `tu_0115609c.c` (`FUN_01158188`, `overlay_render_menu_item_variant_with_primary_state` `0x11581e0`, `FUN_01158274`), which reach `FUN_0116768c` (draft; its snapshot read at `1167818` and tree call at `1167a5c` are bytes) and then `overlay_draw_menu_tree` `0x1164e1c` (list with page tabs, 3,204 bytes) or `overlay_draw_choice_list` `0x1163f2c` (picker, 3,824 bytes). Both are NONMATCHING drafts.
5. `overlay_draw_menu_tree` takes the node as its first argument (passed to `menu_tree_summarize_node` `0xbf8e64`) and reads these snapshot words, each confirmed in bytes: `+0x00` list kind (1 flat, 2 tree), `+0x0c` a word compared with `0x27` and `0x228`, `+0x10` selected row, `+0x14` previous row, `+0x1c` cursor index, `+0x20` visible count, `+0x24` page, `+0x28` scroll top (read confirmed; its use in the value loop is draft). The kind is forced to 1 only for node `0x25` with `+0x10 == 0` and `+0x0c == 0x27`, and for node `0x38` with `+0x10 == 11` and `+0x0c == 0x228` (bytes `1164f18-1164f60`). Rows per page come from `0xbf8e64` and rows from `menu_tree_describe_child` `0xbf8d20` (both NONMATCHING), not from the snapshot.
6. Primitives (draft call sequence): text and icons through `overlay_draw_resource_item` (exact), which sets the clip (`FUN_01177de4`, exact) and calls `overlay_draw_pixel_resource` or `overlay_draw_aligned_pixel_resource` (both exact), which reach `xm1_draw_resource` (3,524 bytes, NONMATCHING draft); rectangles as solid resource `0xff000838` through `overlay_fill_pixel_rectangle`, `overlay_fill_box_border`, `overlay_fill_grid_rectangle` (exact) and `overlay_fill_grid_cell_box` (NONMATCHING) on a 20x24 cell grid; the selected row by text-format brackets (`ov48b_parse_text_format`, draft), palette 0x69 against 0x6a/0x6b and a cell box of colour 0x7c. Resource ids: 0..`0x83d` catalog resources or text, `0xff000000+` bitmaps, `0xfd000000+` custom text, `0xfc000000` inline text. Objects come from `ov48b_allocate_object` (exact; 128 per bank, a limit message when full) and are linked by `ov48b_append_object` (exact).
7. After the handler, `ov44_handle_draw_event` calls `overlay_complete_display_update` `0x1112170` (exact) with `ov44_post_deferred_event` `0x1108638` (exact) as its argument when `ov44_deferred_event_pending` (exact) is nonzero (bytes `11089f0-1108a04`), and calls `0x1108638` directly, without a flush, when it is zero (bytes `1108a0c`). The flush `FUN_01178378` (exact) runs, in order: `FUN_0117cb74` (staged plane descriptor `0x95a064` against applied `0x95a03c`, draft), `ov48b_advance_animation`, `FUN_0117bedc` (draw-command cache replay, draft), `FUN_0117c460` (draw events `0x957aa0+0x1c` against shadow `+0x12dc`, a graphic call per changed event, draft), `ov48b_queue_completion` and `ov48b_take_pending_result`, then clears `+0x2bf8`.
8. A changed event on planes 0/1 sets `display_pending` (`+0x2c0c`), on planes 2+ `commands_dirty` (`+0x2c08`) (draft, `FUN_0117ce14`/`FUN_0117cf14`). `ov48b_queue_completion` (exact) spins `dispatch_delay_ticks(3)` while `batch_busy` (`+0x2bfc`) is 1; with `commands_dirty` clear it calls the callback at once; otherwise it sets `batch_busy`, stores the callback at `+0x2c00`, calls `vglib_schedule_animation(3, ov48b_dispatch_completion)` and clears `commands_dirty`. `ov48b_take_pending_result` (exact) turns `display_pending` into `display_selected_callback_event3(0)`.
9. VGLIB `0x5902` (`FUN_006d7920`) renders the draw list through OpenVG into the back surface; OpenVG submissions post `0x5700` to task 71 (exact `graphics_send_resource_signal`), which runs jobs on the vector engine `0xfff78000` (IRQ source 79, table row `{0x3c, 0xc, 1}` at `0x7c94d8`, bytes). That task 71 queues these jobs into the ring `0x892eb8` is draft.
10. Buffer swap and hand-off (section 1.4), commit on the frame interrupt, one-shot `vglib_post_display_event` (exact) sets flag `0xfefe` on task 73 through `dispatch_send_notification` (exact), VGLIB's `dispatch_wait_current_all(0xfefe, ..., 100)` (exact) returns, the six slot callbacks run, and `ov48b_dispatch_completion` (exact) clears `batch_busy`, runs the enabled callback and the stored callback.

### 1.3 State

| State | Address | Meaning | Status |
| --- | --- | --- | --- |
| Display record | `0x957aa0`, `0x2c10` bytes | Draw events live `+0x1c` and shadow `+0x12dc` (0x20 bytes each), `batch_busy +0x2bfc`, `batch_callback +0x2c00`, `commands_dirty +0x2c08`, `display_pending +0x2c0c`, `+0x2bf0` output-size selector, `+0x2bf8` flush flag | `+0x2bfc/+0x2c00/+0x2c08/+0x2c0c` use exact; layout of the event arrays draft |
| Draw event | 0x20 bytes | identifier, left, top, right, bottom, stroke, colour[3], flags, index; `ov48b_draw_event_changed` compares exactly these | exact (`draw_cache.c:15`) |
| Command mappings | `ov48b_command_mappings`, 10 x 8 bytes | Reset by `ov48b_reset_draw_mappings` | exact |
| Resource objects | `xm1_resource_objects[bank]`, 128 per bank | Free when allocation state 2 and handle 0 | exact |
| Plane descriptor | staged `0x95a064` (`+0x25c4`), applied `0x95a03c` (`+0x259c`) | A change builds a `0xfc000000` object of 1920 x 1080 or 720 x 480 | `FUN_0117cac8` exact, the rest draft |
| VGLIB state | `0x8e2098` | Surfaces `+0x90` (`0x8e2128`), images `+0xc0` (`0x8e2158`), slot callbacks `0x8e21b0..0x8e21c8`, front index byte `+0x160` (`0x8e21f8`) | bytes |
| Engine records | `0x9553cc + engine*0x58` | Callback lists `first[6] +0x24`, `second[6] +0x3c`, `processing +0xc`, interrupt handle `+0x14` | `display_queue_secondary_callbacks` exact; offsets bytes |
| Shadow image | `0x96b9bc + engine*0x1a4` | Register image the commit copies; layer address at `+0x64 + group*0x4c + item*16` | bytes |
| Engine selection | `0x955af8` | Engine 1 when `+0x28 == 0` and `+0x10 == 1` | bytes (`6d03cc-6d03f8`, `6cfd90-6cfdbc`) |

### 1.4 Frame buffers and the front-buffer rotation

- Each VGLIB stage has two buffers. `render_next_buffer` `0x6d700c` returns the other index (bytes `6d7014-6d7030`). `FUN_006d7920` draws into that buffer and hands its image's `+0x1c` address to `FUN_006d76ec` (calls at `6d7b38`, `6d7c40`, `6d7d2c`; modes 8 and 10 make two calls, `6d7c20`/`6d7c40` and `6d7d0c`/`6d7d2c`), which writes the shadow through `display_set_item_configuration` `0x6c9cf4` on the selected engine, group 1, item 0: `bfi` of the address's low 30 bits into shadow `+0xb0`, keeping its top two bits (bytes `6c9cfc-6c9d30`). Then it increments the front byte `0x8e21f8` and wraps it to 0 above 1 (bytes `6d7d30-6d7d58`). Error exits before the increment (`bne 0x6d7df0`) leave the front byte unchanged.
- The commit writes shadow `+0xb0 + 16i` to register `+0xb4 + 4i` of `0xfff6b000` or `0xfff6e000` (bytes `6cacac-6cacb8`).
- dump: in body Run-3 and both calibration dumps `0x955af8+0x10 = 1` and `+0x28 = 0`, so engine 1 (`0xfff6e000`) is the panel engine on the camera and in both models. The two stage-0 image objects carry buffer addresses `0x4228c800` and `0x423de000` in all three dumps, `0x151800` = 720 x 480 x 4 bytes apart, and the engine-1 shadow's group-1 size word `+0xac` is `0x02cf01df` (720 x 480). The body's front byte is 1 and its shadow `+0xb0` is `0x023de000`; both models' front byte is 0 and their shadow `+0xb0` is `0x0228c800`. In all three the shadow holds the buffer the front byte names, as the rotation law requires.
- dump: the body buffer at physical `0x023de000` is a camera-rendered OSD frame of the shooting screen (mode M, 1/60, F4.5, ISO 200, a CARD ERROR banner), extracted as `private-data/scratch/RenderDesign/rd-body-front.bin`. The FujiMod and retained buffers show a different screen state (mode S, no banner), so no pixel law is compared here. The channel order of the PNG renderings is assumed RGBA.
- Pair checks, read host-only from the 56 INC3 render comparisons (`private-data/scratch/ControllerStrategy/menu-engine/pair-runs-3/*-render.json` and their scanout metadata). In all 154 captures that are pixel-equal but unequal, PNG and source-plane bytes are equal and the native scanout state differs in two registers only: 150 in `0xfff6e02c` alone, 2 in `0xfff6e02c` and `0xfff6e0b4`, 2 in `0xfff6e0b4` alone. `0xfff6e02c` is the emulator's main-plane source (691,200 bytes, 720 x 480 x 2); it takes the values `0x3386000`, `0x342ec00`, `0x34d7800` and `0x3580400`, `0xa8c00` apart, a ring of at least four buffers whose writer is not identified here. `0xfff6e0b4` is the OSD address of this section; in the four frame-seek captures that differ in it, the roles hold `0x0228c800` and `0x023de000`. The "front-buffer address" of `E-MENU-ENGINE-INC3-GUEST-PAIRS-150` is therefore mostly the main-plane ring position, and in four captures the OSD buffer index. [INFERENCE] The OSD difference is a different count of completed `0x5902` frames, mod 2, between the roles; no receipt counted them.

### 1.5 Device interactions

| Block | Offsets | Access | Code | Status |
| --- | --- | --- | --- | --- |
| Scanout `0xfff6b000` / `0xfff6e000` | group 0 layers `0x44/0x54/0x64/0x74 + 4i`, group 1 layers `0x94/0xa4/0xb4/0xc4 + 4i`, `0x10c..0x118`, `0x194`, `0x198` | W | `FUN_006ca3e0` | bytes |
| same | `0x00..0x40`, `0x84..0x90`, `0xd4..0xe4`, window pairs `0x11c+8k`/`0x120+8k`, `0x16c..0x190`, `0x1bc`, `0x9d0`, `0x9d4`; `0x18` only on engine 1 | W | `FUN_006ca3e0` | constants bytes, mapping draft |
| same | same offsets | R | `display_save_context` `0x6c926c` | draft |
| same | `+0x00` bit 16 reset handshake | R/W | `display_reset_register_bank` `0x6c9160` | draft |
| Timing `0xfff6a000` / `0xfff6d000` | `0x00`, `0x04`, `0x08..0x20`, `0x2c`, `0x34`, `0x38`; `0x24`/`0x28` on `0xfff6d000` only | W | `display_engine_flush_configuration` `0x6c7b9c`, `FUN_006c8790` | draft |
| same | `+0x30` bit 1 | R | ISRs `0x6c70e4` (`0xfff6a030`) / `0x6c6eb0` (`0xfff6d030`), selecting the next source and phase | bytes `6c70f0-6c7100` |
| same | `+0x00` bit 16 clear, then spin on `+0x30` bit 1 | R/W | `display_wait_input_ready` `0x6c8710` | draft |
| Formatter `0xfff69000` / `0xfff6c000` | `+0x00` bit 28 set, spin until clear | R/W | `FUN_006ce2dc` | bytes `6ce2f4-6ce35c` |
| same | `0x00`, `0x04`, `0x20`, `0x34`, `0xc8`, runs from `0x38` | W | `FUN_006cc1f0` | draft |
| Vector engine `0xfff78000` | job programming, status `0x34` | R/W | tasks 71/72 | draft (manifest) |
| Interrupt sources | 9, 10 -> line `0x37`; 11, 12 -> line `0x34`; 14 -> line `0x38`; 79 -> line `0x3c` | table `0x7c9124 + src*12` read by `driver_configure_boot_handle` `0x64e918` (exact) | rows bytes |

- The engine-0 commit `FUN_006cb0c4` registers as a persistent scheduler request through `FUN_006c7318` (bytes `6c991c-6c9944`). The engine-1 commit `FUN_006cb0d8` registers directly on source 14, line `0x38`, through `driver_configure_boot_handle` (bytes `6c96c4-6c96ec`). The manifest had both on the scheduler's sources 9 to 12.
- The scheduler uses sources 9/10 (line `0x37`, ISR `0x6c6eb0`) when `FUN_006c8850` (exact, `0x9429f4+0x58`) is nonzero. dump: that word is 1 on the body and both models.
- `FUN_006ca3e0` runs and clears both callback lists on every call, not only when a commit is pending (bytes `6caea0-6caeec`), correcting the manifest's reading.
- No memory-DMA (`0xfff02000`) caller was found in VGLIB, the ov44/ov48b draw layer or the display driver; its users found are ISP output routes. This is a negative search result, not a proof.

### 1.6 Exactness

By decomp file placement (a donor-derived grouping), exact and NONMATCHING definitions with their stock bytes:

| Placement | Exact | Bytes | NONMATCHING | Bytes |
| --- | ---: | ---: | ---: | ---: |
| `app/drivers/display` | 161 | 12,012 | 166 | 49,888 |
| `app/ui/render` | 147 | 15,864 | 72 | 30,596 |
| `app/ui/menus` | 61 | 9,716 | 177 | 142,252 |
| `app/native_overlay44` | 489 | 49,140 | 296 | 185,508 |
| `app/graphics/openvg` | 441 | 60,904 | 188 | 95,916 |
| `app/graphics/backend` | 217 | 10,188 | 44 | 15,928 |

The completion and lifecycle edges of the path are exact: `ov48b_queue_completion`, `ov48b_dispatch_completion`, `ov48b_take_pending_result`, `FUN_01178378`, `overlay_complete_display_update`, `ov48b_draw_event_changed`, the object pool, `overlay_draw_resource_item` and the fill functions other than `overlay_fill_grid_cell_box`, `display_queue_completion`, `display_queue_secondary_callbacks`, `FUN_006cfc68`, `vglib_post_display_event`, `dispatch_send_notification`, `dispatch_wait_current_all`, `graphics_send_resource_signal`, the menu snapshot readers and the probe. The large bodies are not: the menu list and picker draw functions, `xm1_draw_resource`, the draw-cache replay, VGLIB's render step, `display_set_item_configuration`, the commit `FUN_006ca3e0` (3,300 bytes), `ov44_post_event_notifications`, and the scheduler and ISRs are NONMATCHING, so their facts above rest on the cited byte reads.

## 2. Target design

The design we would build from scratch: the owned menu (#128) and later owned screens describe what to show as typed views; one widget layer lowers views into a typed draw list; one owned renderer rasterizes draw lists into an owned OSD surface; one owned frame publisher owns buffer lifetimes and completion; one owned scanout driver owns the display engine. Live view and playback image planes stay producers outside this project that hand frames to the scanout driver through the same publication interface.

### 2.1 Draw-command layer (`replacement/render_commands`, `no_std`)

- `DrawList { surface: SurfaceId, commands: [DrawCommand] }` with a fixed capacity, built per frame. `SurfaceId` is a closed enum (`Osd`, later `Hdmi`).
- `DrawCommand` is a closed enum: `Fill { rect: PanelRect, colour: Colour }`, `Border { rect: PanelRect, stroke: StrokeWidth, colour: Colour }`, `Text { origin: PanelPoint, text: TextRef, font: FontId, style: TextStyle }`, `Image { origin: PanelPoint, image: ImageId }`, `Clip(PanelRect)`. Selection is a `TextStyle`/colour choice of the widget, not a draw-time mode.
- `PanelRect` and `PanelPoint` are validated against the surface geometry at construction. `Colour` is a palette entry (`PaletteIndex`) plus `Alpha`; `TextRef` is `Catalog(CatalogTextId)`, `Custom(CustomTextId)` or `Inline(TextRun)`; `ImageId` names a bitmap. Stock's numeric resource id ranges are decoded into these at the adapter boundary only.
- The command set is the union of what stock's draw layer emits for the screens in scope, derived from recorded draw events and resource objects (increment R1), not designed ahead of that evidence. #128 lists "the render layer's command set" as not yet designed; this layer is where it lands.
- Producers: the owned menu engine does not draw. It exposes its state; #128's widgets (list with page tabs, value picker and the rest) lower a typed view into a `DrawList`. For the list: `ListView { rows: [RowView { label: TextRef, value: Option<TextRef>, state: RowState }], cursor: RowIndex, scroll_top: RowIndex, page: PageIndex, pages: PageCount }`; selection exists only as `cursor`. Damage is the difference between two draw lists, as stock's draw cache does over draw events.

### 2.2 Owned renderer

- A CPU rasterizer over the draw list into a 32-bit straight-alpha surface of the surface's geometry (720 x 480 for the panel, the geometry the dumps show). It runs identically on host and guest, so its output is a host-testable function of the draw list and the asset pack.
- It does not use the vector engine for UI. Design choice: the vector engine's rasterization law has no hardware evidence (section 3), and a host-testable renderer removes it from every UI qualification. Full-surface redraw writes 1.38 MB per frame; damage-limited redraw bounds the per-frame cost. Throughput on the camera is not measured.
- Fonts, glyphs and bitmaps come from an asset pack with declared provenance (publication policy).

### 2.3 Owned frame publisher

- Two buffers per surface with a typestate per buffer: `Free -> Drawing -> Queued -> Scanning -> Free`. A buffer returns to `Free` only on the scanout driver's acknowledgment that a newer buffer is scanning.
- `publish(surface, frame) -> FrameId` and a completion event `FramePresented(FrameId)`. "Drained" is a typed predicate: no buffer in `Drawing` or `Queued`. The publisher keeps its buffer states in one typed record the harness reads, which replaces the emulator's render gate (section 3.1) for owned surfaces.
- Buffer identity is not observable behaviour: pair checks compare presented content and the presented frame sequence, and treat the buffer index as an internal detail.

### 2.4 Owned scanout driver

- Owns one display engine's register blocks (scanout, timing, formatter) through a typed register image (`EngineImage { groups: [LayerGroup; 2] }`, layers with origin, size, address, enable, alpha), committed at the engine's frame interrupt.
- Exposes layer publication for the frame publisher and for image-plane producers, and the per-frame callback service retained clients need (section 2.5, S8).
- Panel bring-up (timing configuration, formatter configuration, reset handshake) is part of the target and comes last, once its laws are grounded.

### 2.5 Shims

| Shim | What it does | Removal condition |
| --- | --- | --- |
| S1 Typed command adapter onto the stock draw layer (#128's "stock rendering behind a typed command layer") | Plays an owned `DrawList` through stock's drawing functions inside the UILIB draw event, in place of `overlay_draw_menu_tree`/`overlay_draw_choice_list` for owned classes. Its direct callees are `overlay_draw_resource_item`, `overlay_fill_pixel_rectangle`, `overlay_fill_box_border`, `overlay_fill_grid_rectangle` (exact) and `overlay_fill_grid_cell_box` for the selected-row box (NONMATCHING); below them `xm1_draw_resource` is NONMATCHING. Where a callee is NONMATCHING the adapter's contract rests on bytes. Stock's object pool, draw cache, VGLIB and vector engine render the result | The owned renderer draws the OSD surface for every owned screen |
| S2 UILIB queue and screen table as the draw trigger | Owned drawing is reached from a queue-33 `0x4000` code through its screen-table slot | Owned controllers and the owned event loop (#128, #154) request frames from the publisher directly and no stock sender posts menu codes |
| S3 Stock completion events | The adapter leaves `ov44_post_event_notifications` to post the finished-screen events | Every consumer of those events is owned |
| S4 Display record as shared state | Owned code keeps `batch_busy`, `batch_callback`, `commands_dirty`, `display_pending` and the draw-event arrays consistent because retained stock code reads them | No retained code reads `0x957aa0` |
| S5 Stock resource catalog as asset source | Text and images resolve through stock catalog ids (0..`0x83d`) and bitmaps | An owned asset pack covers every owned screen |
| S6 Shared OSD surface | Stock VGLIB keeps drawing screens not yet owned; owned and stock producers take the surface in turn, handed over at screen transitions | Every screen on the surface is owned |
| S7 Stock publication below the owned renderer | The owned renderer writes the VGLIB back buffer and publishes through `display_set_item_configuration` and `display_queue_secondary_callbacks` with VGLIB's front byte kept in step | The owned scanout driver publishes |
| S8 Stock per-frame scheduler | Retained clients (VGLIB one-shots, live view) keep registering through `FUN_006c7318` | No retained client registers, or the owned driver serves the same request contract |
| S9 Stock panel bring-up | `display_engine_flush_configuration`, formatter configuration and the reset handshake stay stock | Timing, formatter and reset laws are grounded (section 3) and owned bring-up passes |

The menu engine's row "Stock redraw and render path: Redraw requests after the engine; removed when the render layer owns menu drawing" covers S1 and S2 seen from the engine. Its removal condition becomes: S1 and S2 are both removed.

## 3. Device laws for render qualification

A stock/owned pair on one model cancels any law both roles share. Each law matters only for the increment that changes which code exercises it. Hardware status is from the [fidelity audit](emulator-device-fidelity-audit.md) unless a dump read is cited.

| Law | Firmware dependency | Model (FujiMod `3d90035d`) | Hardware status | Needed by |
| --- | --- | --- | --- | --- |
| L1 Frame interrupt delivery and phase order | Commit on line `0x38` (engine 1), scheduler on `0x37` (sources 9/10), one-shot completion at phase 0 | Three phase interrupts per frame on `0x37/0x37/0x38` for bank 1, phases at p/2, 3p/4, p | none (audit 2.4); interrupt controller delivery none (2.2) | R3 |
| L2 Timing status `+0x30` bit 1 polarity | ISRs choose the next source and phase from it; `display_wait_input_ready` spins on it | Set at phase 0, cleared at phase 1, reset value 2 | none; the audit names it the display analogue of the timer direction | R3 |
| L3 Frame period | Frames per key press, VGLIB's 100-tick wait, drain time | `frame_ticks` 400,000 (about 30 Hz at the declared 12 MHz) | none; the 30 fps install video cannot resolve it | R2 timing, R3 |
| L4 Scanout latch and reset handshake | Register writes take effect at the commit; `+0x00` bit 16 handshake | Register file, 2-tick reset completion | none | R3 |
| L5 Formatter strobe | `FUN_006ce2dc` spins until bit 28 self-clears | Latch after 2 ticks, refuses writes while pending | none | R3 |
| L6 Composition and pixel format | Group-1 layer is 32-bit, 720 x 480; backdrop and rectangle overlay | `mode1-abgr8888-straight`, YCbCr packing, backdrop and rectangle profiles declared unmeasured | none. The dumps show firmware-written geometry and addresses agreeing with the model, which is firmware state, not a device law | R2 (PNG only), R3 |
| L7 Vector-engine rasterization and completion | Every stock UI pixel; job completion wakes task 72 through IRQ line `0x3c` | `vector-gpu` node, `bytes_per_tick` 16, IRQ `0x3c`, primitives declared unmeasured | none. The model's line agrees with the firmware's static table; the Run-3 dump holds one camera-rendered frame that a later comparison can use | R2 |
| L8 Kernel timer | VGLIB 100-tick wait, `dispatch_delay_ticks(3)` spin | Up-count after the patch | agreement (audit 2.1); instructions per tick (U14) declared | all timing |
| L9 Memory DMA | Not on the render or scanout path found here | `max(2, bytes/32)` ticks | none (2.3) | none for this project |

### 3.1 The (16,472) render-drain stall

- Facts: in `E-PROPERTY-SERVICE-NATURAL-150` the render after OK on the live-view overlay (16,472) did not drain within 90,000,000 instructions, so the next key was not pressed (`prior-render-pending`). Crawl-4 stopped 7 presses with no display plane at (16,472) and (73,71) ([recipe-bank-ui-ownership.md](recipe-bank-ui-ownership.md#behavioural-census)); that stop is the bridge's separate 60,000,000-instruction plane wait. No record explains either.
- The gate is the emulator's (`emulation/fujimod-xm1/src/render.rs:135-210`): every key binding released, UI, graphic, VGLIB and UILIB tasks all in queue wait, seven cells zero (`ui_pending`, `ui_busy`, `display_suppression` and the four display-record cells), UILIB initialized, the vector engine idle with all jobs acknowledged, and the packet tokens consumed. [INFERENCE] A producer that renders continuously would hold it open.
- [INFERENCE] Candidates: a live-view producer that keeps rendering (no steady state exists to drain to), a task outside queue wait, `display_pending` or `display_suppression` held by the live-view path, or a vector-engine job timing that never lets the counters meet. L3 and L7 are the device laws that could change the outcome; nothing shows either is wrong.
- Settling evidence, emulated and cheap: one run of the natural-3 route to crawl-trial-01469 that records the gate's fields at budget exhaustion. Until then, routes through (16,472) stay out of render qualification, as they are out of the property pairs.
- Target consequence: the owned frame publisher's drain predicate is defined for owned surfaces only and never waits for image-plane producers.

## 4. First increment (R1) and its qualification

R1 makes S1 real for the menu list and picker screens of the owned menu engine's classes, with the command set derived from stock. It changes no device-facing code, so no new device law is needed: both roles share VGLIB, the vector engine and scanout.

1. Host: the `render_commands` crate (types of section 2.1, decode of stock resource ids into `TextRef`/`ImageId`/`Colour`), and a decoder from recorded stock draw state (resource object lists per bank and draw events `0x957aa0+0x1c`) into `DrawList`.
2. Observation: one emulated run per existing menu-oracle and picker configuration that reads the object banks and draw-event arrays at the return of the flush call in `ov44_handle_draw_event` (bytes `11089f0-1108a04`) and, on the unflushed branch, at its call to `0x1108638` (`1108a0c`), giving the stock draw state per draw event. The command set is fixed from these, and any object the decoder cannot type fails the run.
3. Host oracle: the owned list and picker widgets lower the recorded engine state and tree to a `DrawList` equal to the decoded stock list, per case. Negative controls (selected row unstyled, page tabs off by one, scroll top ignored, a hidden row drawn, a disabled row styled as offered) each fail at least one case. The bytes the adapter produces through stock's functions are checked in the guest (step 6), not modelled on the host.
4. Guest: the adapter replaces the calls to `overlay_draw_menu_tree` and `overlay_draw_choice_list` for owned classes and plays the owned list through the functions S1 names; unowned classes fall back to the stock call before any draw.
5. Residency: the run's checker derives each draw's class from the traced arguments (node, snapshot) and the menu tables, independently of the adapter, and fails the run on any entry to `0x1164e1c` or `0x1163f2c` for an owned class while the adapter is resident and on any owned-class draw that does not pass the adapter's entry. One guest negative control, an owned build whose lowering drops the selected-row style, must fail both the draw-state comparison and render-compare.
6. Pairs: the menu-engine guest pairs (`E-MENU-ENGINE-GUEST-PAIRS-150`, `E-MENU-ENGINE-INC3-GUEST-PAIRS-150`) rerun with the owned adapter. Draw events are paired per segment in order of their UILIB codes. Pass: per pair of draw events, the object-bank and draw-event bytes equal, with pointers into the pool compared as pool indices and only the clock exemption of step 7; an unpaired draw event fails unless its draw list differs from the preceding one only in the clock text run; render-compare equal on PNG, source-plane bytes, pixels, native frame state and native scanout state under step 7; the engine checks of those records unchanged.
7. Normalizations, each recorded in the comparison receipt; any difference outside them fails. Captures are taken in the drained state, after the `0xfefe` completion, so the commit has run.
    - Main plane: `0xfff6e02c`, `native_frame_state.main.source`, `native_frame_state.main.source_cpu_alias` and the main plane's `native_source_address` may differ when the main-plane bytes are equal. That plane belongs to a producer outside this project.
    - OSD: `0xfff6e0b4`, `native_frame_state.graphics[0].source`, `native_scanout_state.instances[1].graphics[1].channels[0].source` and the graphics plane's `native_source_address` may differ only between the two stage-0 buffer addresses, and only when, in each role, `0xfff6e0b4` equals the low 30 bits of the `+0x1c` word of the image the front byte `0x8e21f8` names, and the roles' front bytes differ. Each role records its front byte at segment start and counts increments with a trace site on the store at `6d7d44`, so the receipt shows the parity that explains the difference.
    - Clock: on the DATE/TIME screen, the clock text run is identified in the draw list by its resource id. When the roles' runs differ, that text object is excluded from the draw-state comparison and its rectangle from pixel equality.
8. Routes through (16,472) and picker 246 stay excluded while their records are open.

R2 (owned renderer behind S7 for the same screens) needs L7 grounded before pixel equality with stock means anything beyond the model. The held Run-3 frame allows a host-only first check: drive the emulator to the same shooting-screen state (mode M, 1/60, F4.5, ISO 200, card error), capture the plane, and compare per glyph and per region with `rd-body-front.bin`. A camera `read-ram` of `0x0228c800` and `0x023de000` after a known menu screen (inside the range `read-ram` admits) gives menu-screen samples. R3 (owned scanout) needs L1 to L5, which only camera reads of the timing status sequence and the scanout registers can settle.

## 5. What the decomp changed

- Before it, the manifest bound 472 UILIB screen handlers to their slots without reading them, named the OpenVG/EGL library only by inference and refused the commit `0x6ca3e0` on saved-stack taint. With it, the menu screen codes, their draw functions and the snapshot fields the list draw reads are located and byte-confirmed, the OpenVG submission into task 71 is exact C, and the commit's register map is confirmed in bytes.
- The buffer rotation, the frame-done chain and the interrupt-source table were unknown; they are now confirmed in bytes, and the dumps show the rotation's state on the camera.
- The completion and lifecycle edges S4 depends on (batch flags, completion callbacks, the object pool, the draw-event change test) are exact C. S3 (`ov44_post_event_notifications`) and S7 (`display_set_item_configuration`) rest on NONMATCHING functions and byte reads.
- The large draw, render and commit bodies are NONMATCHING, so the command set still has to be derived from observation (R1 step 2). Device laws gain nothing from the decomp: it shows what the firmware reads and waits for, not what the hardware returns.
- Knowing the buffer addresses from source made the held Run-3 dump yield a camera-rendered OSD frame, and made the pair receipts' "front-buffer address" decomposable into the main-plane ring and the OSD buffer index.

## 6. Limitations

- Host only: no claim here was executed. The frame-parity explanation of the four OSD-address differences is an inference.
- The stage-event-to-redraw request path and the menu item to screen-code mapping are not traced. Where the draw list of an unflushed draw event (`ov44_deferred_event_pending` zero) is submitted is not traced.
- Only the INC3 render receipts were read for section 1.4; the receipts behind `E-MENU-ENGINE-GUEST-PAIRS-150` were not.
- The body frame and the model frames show different screen states; no pixel law is compared, and the PNG channel order is assumed.
- Facts marked draft are reading aids only.
- Renderer throughput on the camera is not measured.

## Reproduction

```sh
devenv shell -- bazel run //build-support:xm1-decomp -- lookup 0x6ca3e0 --rev a72c95a3572562fc05f359b4f35915f3a0243b19
devenv shell -- bazel run //build-support:xm1-decomp -- show 0x01177c9c --rev a72c95a3572562fc05f359b4f35915f3a0243b19
od -An -tx1 -j $((0x8e21f8)) -N1 private-data/sessions/20261025T050355Z-read-bulk/ram-00000000-05aa0000.bin
od -An -tx4 -j $((0x96b9bc+0x1a4+0xa0)) -N 0x20 private-data/sessions/20261025T050355Z-read-bulk/ram-00000000-05aa0000.bin
dd if=private-data/sessions/20261025T050355Z-read-bulk/ram-00000000-05aa0000.bin bs=2048 skip=$((0x023de000/2048)) count=$((0x151800/2048)) of=rd-body-front.bin
magick -size 720x480 -depth 8 rgba:rd-body-front.bin -channel A -evaluate set 100% +channel -background black -flatten rd-body-front.png
```
