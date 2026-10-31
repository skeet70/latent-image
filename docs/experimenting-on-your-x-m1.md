# Experimenting on your own X-M1

> **Status.** Every underlying camera operation this guide uses (`read-config`, `commit-f7-enable`, `commit-f7-restore`, `ram-alloc`, `ram-relink` and `ram-push`) has run on the maintainer's X-M1. The `debug-flag` and `ram-session` commands that combine them have not yet run on a camera: the maintainer's body is away for a hardware repair.

This guide is for X-M1 owners who want to set the debug flag on their own camera and RAM-push a payload they are building or changing. Both run on your camera at your own risk. A RAM push is volatile: removing the battery returns the camera to its installed firmware. The debug flag is persistent: it stays set until you clear it. Nothing here repairs a camera that no longer starts; read [the recovery kit](../recovery/README.md) before you begin.

The project ships the sources, builders and the parameter files in [`extensions/ram-inputs/`](../extensions/ram-inputs/); you combine them with Fujifilm's official firmware, which the project does not redistribute. The payloads here are development candidates, not the published [firmware extensions](../README.md#firmware-extensions).

Run the commands below from your checkout inside `devenv shell`, which supplies `bazel` and the pinned toolchains. Outputs go under the ignored `private-data/`. Keep them out of issues and pull requests.

## Requirements

- Linux x86_64 with Nix and devenv, as in [Try it on your host](../README.md#try-it-on-your-host).
- An X-M1 on firmware 1.50. The USB toggle payload works on stock 1.50 or on [RC0](../README.md#firmware-extensions); the Classic Chrome and recipe-bank payloads need RC0 installed.
- The official 1.50 `FWUP0002.DAT` from [Fujifilm](https://www.fujifilm-x.com/global/support/download/firmware/cameras/x-m1/), SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`, saved as `private-data/firmware/x-m1-1.50/FWUP0002.DAT`.
- A USB micro-B cable, and a battery or dummy-battery supply you can remove with the camera on.

## 1. Prepare the relink inputs

The camera relinks a payload at the address it allocates, from decoded firmware images at fixed paths under `private-data/`. Start with the stock decodes, which every payload kind uses:

```sh
D=private-data/firmware/x-m1-1.50/FWUP0002.DAT
R=private-data/analysis/x-m1-1.50
mkdir -p "$R/lz" "$R/update/prefix" "$R/filmsim" "$R/recipes/rc-complete-20261013"
bazel run //recovery:inspect-firmware -- extract "$PWD/$D" "$PWD/$R/stock-components"
bazel build //lz:fflzd //host/firmware:apply-patch-set //host/firmware:rebuild-update
bazel-bin/lz/fflzd "$R/stock-components/segment-5.bin" 0 "$R/lz/id5-00bf1a00.bin"
bazel-bin/lz/fflzd "$R/stock-components/segment-4.bin" 0 "$R/lz/id4-0061c400.bin"
bazel-bin/host/firmware/apply-patch-set recovery/rc0-patch-set.json "$R/lz/id4-0061c400.bin" "$R/update/prefix/id4-patched.bin"
bazel-bin/host/firmware/rebuild-update extract-plain "$D" "$R/lz/plain-01100000.bin"
bazel-bin/host/firmware/rebuild-update extract-image2 "$D" "$R/filmsim/id2-00130000.bin"
```

The USB toggle needs nothing more. Classic Chrome and its icon variant add the fitted parameter file, the Classic Chrome images built from it, and the RC0 DAT:

```sh
P=private-data/analysis/classic-chrome
C=private-data/analysis/classic-chrome-patch/candidate-build-1
mkdir -p "$P" private-data/analysis/classic-chrome-patch
cp extensions/ram-inputs/classic-chrome-candidate.json "$P/candidate.json"
bazel run //host/firmware:build-classic-chrome -- "$P/candidate.json" "$C"
bazel-bin/host/firmware/apply-patch-set "$C/cc-image4.json" "$R/lz/id4-0061c400.bin" "$C/patched-id4.bin"
bazel-bin/host/firmware/apply-patch-set "$C/cc-image5.json" "$R/lz/id5-00bf1a00.bin" "$C/patched-id5.bin"
bazel-bin/host/firmware/apply-patch-set "$C/cc-plain.json" "$R/lz/plain-01100000.bin" "$C/patched-plain.bin"
bazel run //extensions:build-extension -- extensions/rc0.json "$D" private-data/analysis/classic-chrome-patch/rc0-regression
```

The recipe bank also needs the bank spec, the D3 recipe definition and the D3 images the recipe package builds from them:

```sh
cp extensions/ram-inputs/recipe-bank-spec.json "$R/recipes/bank-spec.json"
cp extensions/ram-inputs/recipe-d3-compiled.json "$R/recipes/d3-compiled.json"
bazel run //host/firmware:build-recipe-package -- "$R/recipes/rc-complete-20261013/package-field-marker"
```

Every step checks its inputs against pinned hashes and stops on a mismatch. Built from the official DAT, the outputs match the bytes the maintainer's reviewed payloads were built from.

| Payload kind | Allocation | Installed firmware it needs |
| --- | --- | --- |
| `usb-toggle` | `0xd20` | Stock 1.50 or RC0 |
| `classic-chrome` | `0x1560` | RC0 |
| `classic-chrome-icon` | `0x21fc` | RC0 |
| `recipe-bank` | `0x1dc98` | RC0 |

## 2. Set the debug flag

Adjustment byte `0xf7` is the debug flag. With it set, every USB attach in an ordinary boot starts the `04cb:ff80` service device instead of PTP, so ordinary USB file transfer is unavailable. Each such attach also fills about 41 MB of display buffers with `0xff`, and any program on the host can write camera RAM. The camera stores the flag through a whole-area commit that also stores `0x80 = 0x22` and `0x81 = 0xff`. [The transport README](../transport/README.md#operations-and-their-bytes) gives every byte.

1. Enter jig mode: with the camera off and USB attached, hold the shutter fully down and the Up key, switch on, and keep both held until the screen is blank and the green LED is lit. `lsusb -d 04cb:` should list `04cb:ff80`.
2. `bazel run //transport:camera -- debug-flag backup` reads the whole adjustment area, your body's calibration, into `private-data/sessions/<UTC>-debug-flag-backup/windows/`. It keeps window `0x000000` as `private-data/debug-flag/reference-000000.bin`; a later backup stops if window `0x000000` differs from it outside `0x80`, `0x81` and `0xf7`. Copy the session directory to a second location before going on.
3. `bazel run //transport:camera -- debug-flag enable` ends with `commit-f7-enable: committed`. Any other result line names what it found and what to do.
4. End the session by removing the battery or cutting the dummy-battery supply, then unplug USB. Don't use the POWER switch: ending a jig session that way resets date, time, language and the C slot.
5. Boot normally and attach USB. `lsusb -d 04cb:` should list `04cb:ff80`, and `bazel run //transport:camera -- debug-flag status` prints `0xf7=01`.

To clear the flag, run `debug-flag disable` in a jig boot and end the same way. `0x80` stays `0x22`. By static analysis, installing the official 1.50 update from the camera's update menu also returns `0xf7`, `0x80` and `0x81` to `0x00`; no body has confirmed it.

## 3. Change the payload

The relinkers are `host/firmware/build-*-ram.sh` with their builders in `host/firmware/`. Payload code lives in `menu/`, `host/firmware/` and `replacement/`; the parameter files are the ones in `extensions/ram-inputs/` you copied in step 1, so edit the copies under `private-data/`. Several builders also check the hashes of their own inputs, so a change can need a builder change too. The RAM installer (`menu/ram_push_installer.c`) and its rendezvous are shared by every kind, and a fault there can hang the camera.

Run the change in the emulator before the camera. [The emulator README](../emulation/README.md) describes the RAM-push end-to-end harness and its `--unreviewed-relink` mode, which pushes a build that differs from the pins below.

## 4. Pin your change, or waive pinning

The camera binary compiles the source and input digests of each payload kind from `transport/ram-push*-review.json`. Before any USB transfer, `ram-session` relinks the payload and refuses when the build differs from them. After a change, do one of the following.

**Pin it.** Commit your change, then record the new digests:

```sh
bazel run //transport:ram-push-toggle-repin -- unreviewed "$(git rev-parse HEAD)" --changed
```

`--changed` re-pins every pin and the publication contract whose digest differs from the relink, marked unreviewed at that commit, and prints each one. The other kinds use `//transport:ram-push-cc-repin`, `//transport:ram-push-cc-icon-repin` and `//transport:ram-push-repin` (the recipe bank). When the relink names a file the record has no entry for, or the reverse, `--changed` names it and stops; add or remove that pin entry first ([Re-pin](../transport/README.md#re-pin)). From then on the camera pushes exactly the bytes you pinned: an uncommitted edit, a different input image or a toolchain change is refused until you pin again. This project's own pre-go gate, `//transport:ram-push-review-verdicts`, lists unreviewed pins.

**Waive it.** Add `--waive-pins` to `ram-session` (step 5). The record stays as it is, and the camera pushes whatever the relink produces. These checks still run: each builder's own input hashes, the [bank ELF allowlist](../transport/README.md#bank-elf-allowlist) for the recipe bank, the installer entry offsets against the compiled layout, the manifest's base and size against the allocation, and the installer's check that every address it writes still holds its expected stock word. What you give up:

- Nothing ties the pushed bytes to a commit. An uncommitted edit, a stale or different input image, or a toolchain change is pushed as built, and the session's files are the only record of what ran.
- The publication contract, the set of addresses and words the installer writes, is not compared with anything. A change there reaches the camera with only the stock-word check in front of it.
- `ram-unload` authenticates against the pins and refuses a waived push whose bytes differ from them. Remove such a payload with the battery.

Either way, a payload runs inside the camera's own firmware. A fault can hang the camera until you remove the battery, and a payload that calls stock code can change settings, which an orderly switch-off then stores.

## 5. Push it

Boot normally with the debug flag set, attach USB, and leave the camera idle at the shooting screen with no menu open and no card activity. Then run:

```sh
bazel run //transport:camera -- ram-session usb-toggle
```

It relinks the payload at its reference base and checks the build, refusing before any USB transfer on a mismatch. It then allocates camera RAM, relinks at the allocated address and installs, writing `alloc/`, `link/` and `push/` under `private-data/sessions/<UTC>-ram-session-usb-toggle/`. Success ends with `ram-push: installed`.

A result line containing `uncertain` means: send nothing further and remove the battery. If the relink fails after the allocation, the block stays allocated and uninstalled; `ram-free` with the printed state file frees it, and removing the battery also clears it.

## 6. Remove it

Remove the battery for at least ten seconds, then start again. A switch-off alone has left a payload's menu entries in place, and it stores any settings the payload changed. `ram-unload` exists for the other kinds but is not needed, and the USB toggle refuses it.
