# USB-toggle RAM push: bounded cardless session

This is a script, not a run or an authorization. It follows [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs) and the [Classic Chrome RAM session pattern](classic-chrome-patch.md#84-camera-session-script-first-classic-chrome-ram-push). The operation is H2: publish the reviewed Q-attach stub and hook in RAM, change only RAM `0xf7` through two Q attaches, then remove all power. No flash, card insertion, persistent configuration operation, `ram-unload` or `ram-free` is part of this session. The card slot is broken and remains empty.

The [mechanism design](usb-personality-toggle.md#14-ram-push-variant) and [independent reviewer brief](review/usb-toggle-ram-push.md) own the bytes, publication contract and evidence limits. The payload allocation is `0xd20` bytes, ABI 6, with 22 setup publications followed by one activation publication. The stub stays at `0x00bf4454`; the hook is `0x00ecf900`. The allocation contains the shared installer, not a new address for the stub.

## 1. Before the session

| Code | Action | Must show |
| --- | --- | --- |
| M1 | On the session commit, run `bazel test //... //tests:research-contracts` and the toggle review gates under the checked-in devenv shell | Passing suites; independent mechanism review approved; review-byte and verdict gates passing for `transport/ram-push-toggle-review.json` |
| M2 | Read the exact-payload emulator receipts named in the reviewer brief | Successful allocation, authenticated relink, upload and installation; every unavailable detach and continuation explicitly recorded as hardware-only, not claimed as a complete controller cycle |
| M3 | Confirm this body's backups and ordinary RC0 baseline | X-M1 firmware 1.50 with RC0, seven film rows, stored `0xf7 = 01`, no other RAM payload resident; no recovery experiment or flash |
| M4 | Create a new private session-notes directory and reserve fresh allocation, link and push output names | No output directory or consumed allocation state reused |
| P1 | Owner: ordinary power-on with USB disconnected and no key held; leave the card slot empty | Normal startup, live view and menus, no setup prompt; record ordinary M/C settings and the seven film rows without changing them |
| P2 | Owner: use a charged battery or secured mains-backed dummy supply, keep the camera stable | Power uninterrupted until the scripted ending |
| P3 | Owner: plain USB attach, then leave it attached until A1 | Exactly one `04cb:ff80`; no Q press during this attach |
| M5 | Under this session's go, `read-config` with the existing private gate-byte plan | X-M1, RAM `0xf7 = 01`; record `0x80` and `0x81`, expected `22` and `ff`; window 0 equal to the body's reference outside those three bytes |
| M6 | Under this session's go, one `read-ram` with the four-window plan in section 5 | All 144 extent bytes stock, hook `0xebdf3ffd`, callback slot `0x00dc7874`, service mode 3 and table pointer `0x015c22a0` |

Every missing check or unexpected value stops before publication. A switch cycle alone does not prove an earlier payload has gone; start only after the ordinary baseline and stock-word read agree.

## 2. One-command-at-a-time load

These are conditional commands for the later authorized camera session. The author of this script does not run them while preparing it. Run from the repository root; each output directory must be new. Each live command must have its expected final line before proceeding.

```sh
ALLOC_OUT="$PWD/private-data/sessions/xm1-usb-toggle-ram-alloc-1"
LINK_OUT="$PWD/private-data/analysis/usb-toggle/ram-push/operator-linked-1"
PUSH_OUT="$PWD/private-data/sessions/xm1-usb-toggle-ram-push-1"

devenv shell -- bazel run //transport:camera -- ram-alloc 0xd20 "$ALLOC_OUT"
devenv shell -- bazel run //transport:camera -- ram-relink "$ALLOC_OUT/ram-alloc-state.json" "$LINK_OUT"
```

| Code | Action | Must show |
| --- | --- | --- |
| L0 | Owner confirms the ordinary shooting screen, no menu, no card activity; pause other USB clients and model requests | Idle condition before L1 and L3 |
| L1 | `ram-alloc` above | `ram-alloc: retained`; state size 3360, X-M1, nonzero `config_f7`, CPU0 `0x4003`, nonzero eight-byte-aligned returned base, table pointer `0x015c22a0`, stock slot `0x00dc7874` |
| L2 | Host-only `ram-relink` above; record SHA-256 of `runtime.bin` and `ram-push-manifest.json` | 23 publications, 22 setup then hook activation; reviewed source identities and publication contract; correct entries for this returned base |
| L2a | Independent actual-base review of the resulting runtime, manifest and allocation bounds | Approval names these exact two SHA-256 values; an emulator/reference base is not the camera's address |
| L3 | Owner repeats the L0 idle check, then the model runs the command below once | `ram-push: installed`; ABI 6, INSTALL status 0, validated 23, setup 22, activation 1, peer status 0 and both CPU0 waits below `0x80000`; callback slot restored; all publications read back |

```sh
devenv shell -- bazel run //transport:camera -- ram-push "$ALLOC_OUT/ram-alloc-state.json" "$LINK_OUT/ram-push-manifest.json" "$PUSH_OUT"
```

Read the saved raw mailbox reply and installed state offline. Require agreement with the accepted allocation, runtime and manifest. Do not send any extra camera query during allocation or installation. An allocation, relink or push failure goes directly to section 4, without free, unload, retry or a second allocation.

## 3. Two Q attaches

Start external phone video before A1. Each cable act uses the camera end once; the host end stays attached. Descriptor listings use only `devenv shell -- lsusb -d 04cb:`. Do not run PTP clients, `gphoto2`, probes, whole-area commits or any command that writes the RAM gate. The broken slot prevents file-transfer and capture/save/playback acceptance.

| Code | Act | Must show |
| --- | --- | --- |
| A1 | Owner: unplug after L3, wait five seconds | The detach after an enumerated `ff80` session is hardware-only coverage. Record the display, open and close MENU once, require live view and no reboot or freeze |
| A2 | At the shooting screen, owner presses and holds Q, plugs USB once, holds about two seconds, releases Q; close an open Q panel with DISP/BACK | `04cb:02b6`, no `ff80`; record whether Q opened a panel, when it was pressed relative to the plug and any USB-screen flicker. RAM `0xf7 = 0` is inferred from the personality, not directly read |
| A3 | While PTP is selected | No `read-ram`, `ram-alloc`, `ram-push` or other service request. The gate is closed, so the host cannot verify RAM again here |
| A4 | Owner: unplug, wait five seconds | Record the display; MENU opens and closes, live view runs, no reboot or freeze. This detach after PTP start is also outside the retained controller model |
| A5 | At the shooting screen, owner holds Q across one attach as in A2, then releases it and closes any Q panel | `04cb:ff80`, no `02b6` |
| A6 | Model: one `read-config` of the gate-byte plan, then one `read-ram` of the four-window plan | RAM `0xf7 = 01`; `0x80`/`0x81` and the rest of config window 0 unchanged from M5; stub/hook equal the accepted manifest's new words; callback slot stock; service mode/table correct |
| A7 | Optional, only if included in the go: owner unplugs, checks the A1 display conditions, waits five seconds and attaches without touching Q | `04cb:ff80`; plain attach retains the current RAM personality. No additional service read is needed |

Any descriptor mismatch stops the sequence, including an unexpected `ff80` at A2. Do not repeat an attach to chase contact timing or read a still-visible service device. Contact bounce can deliver more than one attach while Q is held; record the timing and screen, then analyze offline. No on-body claim is accepted until A1 through A6 and the ending pass.

## 4. Ending and stop rules

The normal ending and every stopped path use full power removal. There is no `ram-unload` requirement and no cleanup command over USB.

1. Stop all host requests and menu use. Photograph the last display and retain journals and video.
2. Owner switches off. If shutdown does not complete within 60 seconds, disconnect USB and remove the battery or cut the dummy supply. Do not wait for or retry a host command after an uncertain target write.
3. Owner disconnects USB and removes battery/dummy supply for at least ten seconds, even after an apparently normal shutdown. A switch-only cycle is insufficient. This invalidates the allocation, installed state and actual-base manifest for future trials.
4. Reinsert power, boot normally with USB disconnected, require normal startup/live view/menus, no setup prompt, seven RC0 film rows and unchanged recorded M/C settings. Do not change settings to conceal a difference.
5. Under the session go's post-reboot read authorization, make one plain attach. Require `04cb:ff80` from stored `0xf7 = 1`. Run one `read-config` of the gate-byte plan and one `read-ram` of section 5. Require `0xf7 = 01`, config window 0 unchanged from M5, every publication at its old word, the full 144-byte extent stock and callback slot `0x00dc7874`.
6. Stop if any reboot/read condition differs. Do not reuse saved states, reflash, restore a dump or repeat the experiment. Hardware work resumes only after reporting the finding and a new go.

An unexpected reply/status/disconnect, command without its final line, short read, failed check, freeze, error dialog, unexpected reboot or changed setting ends the sequence immediately. Intentional A1/A4/A7 cable changes occur only between completed commands. If a command remains without a final line for ten minutes, treat it as uncertain and use the ending above. Do not issue a different command to determine whether the first one finished.

## 5. Private post-reboot read plan

The prepared file is `private-data/read-plans/usb-toggle-ram-push-post-reboot.json`, mode 0600. It is also the pre-push and after-restoration read plan. Its bytes are reproducible from this JSON, saved under `umask 077` and with `chmod 0600` before use:

```json
{"windows":[{"index":0,"address":"0x00bf4454","length":144,"purpose":"USB toggle extent: require all stock bytes after full power removal"},{"index":1,"address":"0x00ecf900","length":4,"purpose":"Handler A hook: require stock BL 0xebdf3ffd after full power removal"},{"index":2,"address":"0x015c22ac","length":4,"purpose":"RAM push callback slot: require stock 0x00dc7874"},{"index":3,"address":"0x0146a0c0","length":16,"purpose":"Service mode 3 and callback table pointer 0x015c22a0"}]}
```

```sh
devenv shell -- bazel run //transport:camera -- read-config "$PWD/private-data/read-plans/gate-byte.json"
devenv shell -- bazel run //transport:camera -- read-ram "$PWD/private-data/read-plans/usb-toggle-ram-push-post-reboot.json"
```

Decode words little-endian. M6 and the final read compare window 0 byte-for-byte with `[0x00bf4454,0x00bf44e4)` of the pinned stock image 5 (extent SHA-256 `a862a878075d05fd011efb724fdd7503fb406bddac8c1a00f27998486efd4ec0`); window 1 must be `fd 3f df eb`. A6 instead requires the first 88 bytes equal the reviewed stub (`35a7e9597f06bdc1dda250d991df4a468d2401e5ae3c24c22e37403318746d62`), its remaining 56 bytes stock, and window 1 `d3 92 f4 eb`. Window 2 is `74 78 dc 00`; window 3 starts with mode 3 and pointer `a0 22 5c 01` at offset 4. No read is attempted while PTP is selected.

## 6. What the go names

The owner's fresh H2 go names this session commit, the approved toggle source-identity and publication-contract pins in `transport/ram-push-toggle-review.json`, the exercised runtime and manifest digests in the [reviewer brief](review/usb-toggle-ram-push.md), `ram-alloc 0xd20`, host-only relink at the actual returned base, independent actual-base approval before the single `ram-push`, A1 through A6, whether optional A7 is included, and the battery-removal/reboot/read ending. It authorizes the listed descriptor checks and exact read plans across the intentional cable detachments; the standing read grant alone lapses on every unplug.

The exercised FujiMod allocation was `0x00a7e8c8`: runtime SHA-256 `b6b86ec6a0409cf4419c2d7c5a0b4ecb3ce301a59b2d652533cf92a9e59257ad`, manifest SHA-256 `0a37e6aa0a511608f267701730455a87bb2a4adafd2bfa0f58e18d9255024bb1`, publication contract `9689704f21b660df2806a52a481cda1909182cac593007e094d569d69764334c`. Its push succeeded; the next configured-`ff80` detach was undelivered, so the body cycle is not emulator-proven. The actual camera allocation need not match this address.

The go acknowledges unavailable SD recovery, no card-backed capture/playback acceptance, loss of all RAM state on battery removal, the model's undeliverable detach edges, the unobserved UI effect of held Q and the unobserved key-driver read from usbcont on hardware. It authorizes no persistent write, firmware update, gate-writing probe, unload/free or retry. Actual-base approval cannot be substituted with the reference digests.
