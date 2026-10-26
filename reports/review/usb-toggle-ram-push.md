# Independent review: USB-toggle RAM publication

## Decision requested

Review the new H2 RAM-push mechanism before any live run. The stub's 88 bytes and fixed address have earlier independent approval, but copying them with the shared installer and activating handler A while usbcont may be running is new mechanism. Approval must name the source-identity and publication-contract pins in `transport/ram-push-toggle-review.json`, the count-specialized installer bytes at the reference bases and the [bounded session script](../usb-toggle-ram-push-session-script.md). It does not authorize a camera command or accept an unknown actual-base runtime.

The requested hardware experiment is one push on an ordinary RC0 boot with stored `0xf7 = 1`, detach, Q attach to PTP, detach, Q attach to `ff80`, optional plain-attach retention, full battery removal, ordinary reboot and a stock-word read. The broken SD slot stays empty. There is no flash, persistent settings operation, `ram-unload`, `ram-free` or retry.

## Mechanism and exact-byte contract

- Stub: `host/firmware/usb-toggle-stub.S`, linked at `0x00bf4454`, 88 bytes, SHA-256 `35a7e9597f06bdc1dda250d991df4a468d2401e5ae3c24c22e37403318746d62`. The same four stock callees and return ABI as the [reviewed flash toggle](../usb-personality-toggle.md#31-stub).
- Hook: at `0x00ecf900`, old `0xebdf3ffd` (`bl 0x0069f8fc`), new `0xebf492d3` (`bl 0x00bf4454`). No stub relocation or change of continuation `0x00ecf904`.
- Publications: 22 aligned setup words covering `[0x00bf4454,0x00bf44ac)`, then the hook as the single activation word. Contract SHA-256 `9689704f21b660df2806a52a481cda1909182cac593007e094d569d69764334c`.
- Old image 5: SHA-256 `3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a`; original 144-byte extent SHA-256 `a862a878075d05fd011efb724fdd7503fb406bddac8c1a00f27998486efd4ec0`. The final 56 extent bytes are untouched.
- Allocation: 3360 bytes (`0xd20`), ABI 6; shared installer starts at `align32(base)`, occupies `0xd00` bytes, with install/rollback/status offsets `0x600/0x83c/0x868`, table offset `0xa88`, 368 publication-table bytes, rendezvous `+0xc00`, mailbox `+0xc80`.
- Shared source: `menu/ram_push_installer.c` and `menu/ram_push_abi.h` are unchanged. Their count-specialized object code differs from existing payloads. Its layout is validated at build time, not assumed from the bank's entry offsets.

The pinned review JSON is authoritative for source identities, builder identity, toolchain identity and review verdict. The builder validates the linked stub hash and target calls, old hook/extent, shared installer call closure, runtime section/layout bounds, table order and allocation overlap. `ram-relink` and `ram-push` authenticate both supplied files against the pinned relinker at the actual retained base. The byte gate hashes the generated stub artifact as well as workspace files.

The publication-contract digest binds addresses, order, phases and old words. New words are bound by the authenticated builder output and stub/source/toolchain pins, not that digest alone. Live `ram-unload` refuses this payload before USB: a task suspended inside the fixed-address stub cannot resume safely after in-place restoration of its old extent. Battery removal is the ending.

## Read set

1. [RAM design, section 14](../usb-personality-toggle.md#14-ram-push-variant), [original design sections 2.3, 3.1 to 3.5, 5, 6.2, 8.3, 8.5 and 9.3](../usb-personality-toggle.md).
2. [Publication and authentication contract](../../transport/README.md#ram-push-review-record), `transport/FujiAF/RamPush.hs`, `RamPushReview.hs`, camera dispatch and generated review-pin plumbing.
3. `host/firmware/BuildUsbToggleRam.hs`, `UsbToggleRam.hs`, `usb-toggle-ram.ld`, `build-usb-toggle-ram.sh`, the new builder/source-digest targets, and `BuildUsbToggleRamTests.hs`.
4. The unchanged `menu/ram_push_installer.c`, `menu/ram_push_abi.h`, `host/firmware/usb-toggle-stub.S` and `.ld`; the new `menu/BUILD.bazel` export group.
5. `transport/ram-push-toggle-review.json`, new review/repin/build-check/camera-pins targets and behavioral test additions.
6. `emulation/UsbToggleEmu.hs --fujimod --ram-push`, its receipt on the exact payload, `emulation/fujimod-xm1/src/toggle.rs`'s List command, and native cases supplied with the installed stub bytes.
7. [Camera session script](../usb-toggle-ram-push-session-script.md), including mode-0600 private read plan, no-service-request PTP interval, stop rules and the battery-removal/reboot/read ending.

## Questions

1. Does each manifested setup word exactly reproduce the reviewed linked stub, with the old value from the pinned image 5, followed only by the activation hook? Do the publication contract and authenticated builder output together bind order, phase, addresses and old/new words?
2. Does the builder pin every input/source/tool capable of changing mechanism bytes, reject substituted stub/image/tool/runtime/manifest/publication bytes, and keep the complete installer inside the allocation for every accepted eight-byte-aligned base?
3. Does the count-specialized shared installer preserve the CPU1 park/ack and IRQ-masked CPU0 window, validate every old word before any store, publish activation last and finish cache maintenance before releasing either core?
4. Is section 14.2's suspended-handler argument sound? Before the hook, an existing frame will execute the complete new stub; inside the old getter it returns to the unchanged continuation; after the old getter it consumes its existing `r0`. No pre-existing frame can refer into the unreferenced stub extent. Does any cached instruction or peer state escape the shared maintenance contract?
5. Are the hook's caller-saved-register, stack and flags requirements unchanged, and do all stores remain RAM-only? Is the read of key-driver row `+0x15` from usbcont's context covered only by the earlier static/emulated evidence, without claiming hardware observation?
6. Is the session's verification honest while RAM `0xf7 = 0`? PTP descriptors are the only host observation, RAM 0 is inferred, and no service read or second allocation is attempted until Q restores `ff80`.
7. Does the exact-payload receipt distinguish successful installation from the controller disconnect boundary? Are detach after an enumerated `ff80` session, detach after PTP start, held-Q UI behavior and usbcont's key read still named as hardware-only where not actually delivered?
8. Do the stop rules avoid retry/unload/free after an uncertain write and require full power removal followed by a fresh stock-word/config read, without reusing allocation or installed-state files?

## Host-only reproduction

Use a dedicated worktree/output base with `--jobs=4 --local_resources=memory=6144` and `XM1_EMULATOR_SLOTS=8`. These commands build and emulate only:

```sh
devenv shell -- bazel test //host/firmware:usb-toggle-ram-tests //transport:ram-push-tests //transport:ram-push-review-record-tests //transport:ram-push-toggle-review-verdicts //transport:ram-push-toggle-review-bytes //transport:ram-push-toggle-camera-pins
devenv shell -- bazel run //transport:ram-push-toggle-reviewed-build-check
devenv shell -- bazel run //emulation:usb-toggle-emu -- --fujimod --ram-push --receipt-output private-data/analysis/usb-toggle/ram-push/REVIEW_SESSION
devenv shell -- bazel run //emulation:usb-toggle-emu -- --fujimod --runs e7,e5u --dats rc0-toggle --receipt-output private-data/analysis/usb-toggle/ram-push/REVIEW_TRANSITIONS
devenv shell -- bazel test //... //tests:research-contracts
```

The session command exits nonzero if a planned detach is undelivered; inspect its `result`, `boundary`, completed `steps` and `checks`. A known boundary is not a passing complete session, and a different refusal is not automatically that boundary. Existing flash-toggle transition runs use identical installed stub/hook bytes but do not prove that a pushed session detached successfully. Exact-payload byte identity and the real upload receipt must both be inspected.

After review approval, record the reviewer's identity, reviewed commit, verdict and scope in the review JSON and re-pin the approved subjects. The final live go still needs independent actual-base review after allocation and the owner's authorization. No camera or USB command is part of this review.

## Exercised artifacts and outcomes

| Allocation base | Runtime SHA-256 | Manifest SHA-256 | Evidence |
| --- | --- | --- | --- |
| `0x00c20000` | `9d3c4f4d0a6322c41943fb0d16b14d3a5cf71461f182812e6b6dc41744e468b1` | `340980738ab9aa3a34bcd070e311bbea60323fc24cf4019955d21949f2f93cb3` | Authenticated reference build |
| `0x00c21f88` | `ee5cb53d297cb2cd8544faceec3e52f76dd5a07f0f9b79428092293ae8962895` | `879650c6fdd346ab82a72b47de24bc7bd881e392ce3a43c74315742d21d52037` | Authenticated reference build |
| `0x00a7e8c8` | `b6b86ec6a0409cf4419c2d7c5a0b4ecb3ce301a59b2d652533cf92a9e59257ad` | `0a37e6aa0a511608f267701730455a87bb2a4adafd2bfa0f58e18d9255024bb1` | Exact FujiMod push, `session-4` |

All three builds use builder identity `e20f5ac79bc760ef436f973099d8fbe7a0993c39c4f0aac8d92dcc105c8406ac` and the source/toolchain pins in the review JSON. A different actual allocation base requires its own independent artifact approval.

- `private-data/analysis/usb-toggle/ram-push/session-4/ram-push-rc0/receipt.json`: allocation, authenticated relink and `RamPushInstalled`; readback matched the exact stub/hook, 23 publications and unchanged RAM `0xf7 = 1`. The following enumerated-`ff80` detach is blocked at section 8.3, driver state `0x0a`, status `0x11`, one failed edge-delivery check. Both planned Q attaches are unrun in this continuous session.
- `private-data/analysis/usb-toggle/ram-push/native-exact-2`: slot-held native cases use `session-4`'s `installed-stub.bin`; 364/364 pass, both toggle directions covered, 80/80 plain-edge pairs identical to stock. These are native behavior cases, not physical disconnect evidence.
- `private-data/analysis/usb-toggle/ram-push/transitions-1`: FujiMod `e5u-ff80`, `e5u-q-ptp`, `e7-plain` and `e7-q` pass on the byte-identical flash-toggle composition. The cycle stops at PTP detach (state `0x08`, status `0x01`), one failed edge-delivery check. This is component evidence, not a complete RAM-pushed cycle.
- `//transport:ram-push-toggle-reviewed-build-check`: both reference builds accepted; substituted compiler, manifest, source identities, runtime, publication contract and entry/state inputs refused before USB where required.
- Final `//...`, `//tests:research-contracts` and explicit `//transport:ram-push-toggle-review-verdicts`: 102/102 tests passed, including the byte and live-refusal gates.

Hardware-only acceptance is both configured detach edges, the complete pushed cycle, the held-Q UI effect, the key-driver read from usbcont, and the battery-removal/reboot/stock-read result. No body command was run.

## Independent decision

`ToggleMechanismReview` and `ToggleSessionReview`, both `anthropic/claude-opus-5-5`, independently approved their complementary slices on 2026-10-26. The reviewed working-tree source scope is committed as `9b10f289ee5e1e5cfb5d80f08190602eb05ca8e2`. The mechanism reviewer identified the unsafe possibility of in-place live unload with a suspended stub frame; the CLI now refuses this payload before output creation or USB, and the reviewer confirmed closure. No findings remain.

The session reviewer inspected the exact push receipt and private read-plan mode/content. The mechanism reviewer inspected the source, layout, call closure, pins and publication argument; the host test/emulator outcomes in the artifact table are author-executed evidence, not an independently rerun experiment. Review records are in `transport/ram-push-toggle-review.json`, with retained reviewer responses at `private-data/analysis/usb-toggle/ram-push/reviews.json` and the linked reviewer sessions. Approval is for the bounded temporary mechanism and script, not a physical-body result or an unknown actual-base artifact.
