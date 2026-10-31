# Experimenting on your own X-M1

This guide is for X-M1 owners who want to set the debug flag on their own camera and RAM-push a payload they are changing. Both run on your camera at your own risk. A RAM push is volatile: removing the battery returns the camera to its installed firmware. The debug flag is persistent: it stays set until you clear it or install a firmware update. Nothing here repairs a camera that no longer starts; read [the recovery kit](../recovery/README.md) before you begin.

Every command below runs from your checkout as `devenv shell -- bazel run //TARGET -- ARGS`, shortened here to `bazel run`. Outputs go under the ignored `private-data/`. Keep them out of issues and pull requests.

## Requirements

- Linux x86_64 with Nix and devenv, as in [Try it on your host](../README.md#try-it-on-your-host).
- An X-M1 on firmware 1.50. The USB toggle payload works on stock 1.50 or on [RC0](../README.md#firmware-extensions); the Classic Chrome and recipe-bank payloads need RC0 installed.
- The official 1.50 `FWUP0002.DAT` from [Fujifilm](https://www.fujifilm-x.com/global/support/download/firmware/cameras/x-m1/), SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`, saved as `private-data/firmware/x-m1-1.50/FWUP0002.DAT`.
- A USB micro-B cable, and a battery or dummy-battery supply you can remove with the camera on.

## 1. Prepare the relink inputs

The camera relinks a payload at the address it allocates, from decoded firmware images at fixed paths. These commands produce the USB toggle's input:

```sh
D=private-data/firmware/x-m1-1.50/FWUP0002.DAT
R=private-data/analysis/x-m1-1.50
mkdir -p "$R/lz"
bazel run //recovery:inspect-firmware -- extract "$PWD/$D" "$PWD/$R/stock-components"
bazel build //lz:fflzd
bazel-bin/lz/fflzd "$R/stock-components/segment-5.bin" 0 "$R/lz/id5-00bf1a00.bin"
```

`id5-00bf1a00.bin` must hash to `3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a`. The Classic Chrome payloads also need the stock and RC0 image-4, plain and image-2 files:

```sh
mkdir -p "$R/update/prefix" "$R/filmsim"
bazel build //host/firmware:apply-patch-set //host/firmware:rebuild-update
bazel-bin/lz/fflzd "$R/stock-components/segment-4.bin" 0 "$R/lz/id4-0061c400.bin"
bazel-bin/host/firmware/apply-patch-set recovery/rc0-patch-set.json "$R/lz/id4-0061c400.bin" "$R/update/prefix/id4-patched.bin"
bazel-bin/host/firmware/rebuild-update extract-plain "$D" "$R/lz/plain-01100000.bin"
bazel-bin/host/firmware/rebuild-update extract-image2 "$D" "$R/filmsim/id2-00130000.bin"
```

| Payload kind | Allocation | Inputs reproducible from the official DAT |
| --- | --- | --- |
| `usb-toggle` | `0xd20` | Yes, the first block above |
| `classic-chrome` | `0x1560` | No: it also needs the fitted parameter file `private-data/analysis/classic-chrome/candidate.json`, made from X-T1 firmware and calibration captures ([model report](../reports/classic-chrome-model.md)) |
| `classic-chrome-icon` | `0x21fc` | No: the Classic Chrome inputs, plus the RC0 DAT from `//extensions:build-extension` |
| `recipe-bank` | `0x1dc98` | No: it also needs a bank spec and the D3 recipe definition, which have no public producer |

## 2. Set the debug flag

Adjustment byte `0xf7` is the debug flag. With it set, every USB attach in an ordinary boot starts the `04cb:ff80` service device instead of PTP, so ordinary USB file transfer is unavailable. Each such attach also fills about 41 MB of display buffers with `0xff`, and any program on the host can write camera RAM. The camera stores the flag through a whole-area commit that also stores `0x80 = 0x22` and `0x81 = 0xff`. [The transport README](../transport/README.md#operations-and-their-bytes) gives every byte.

1. Enter jig mode: with the camera off and USB attached, hold the shutter fully down and the Up key, switch on, and keep both held until the screen is blank and the green LED is lit. `lsusb -d 04cb:` should list `04cb:ff80`.
2. `bazel run //transport:camera -- debug-flag backup` reads the whole adjustment area, your body's calibration, into `private-data/sessions/<UTC>-debug-flag-backup/windows/`. It keeps window `0x000000` as `private-data/debug-flag/reference-000000.bin`; a later backup stops if window `0x000000` differs from it outside `0x80`, `0x81` and `0xf7`. Copy the session directory to a second location before going on.
3. `bazel run //transport:camera -- debug-flag enable` ends with `commit-f7-enable: committed`. Any other result line names what it found and what to do.
4. End the session by removing the battery or cutting the dummy-battery supply, then unplug USB. Don't use the POWER switch: ending a jig session that way resets date, time, language and the C slot.
5. Boot normally and attach USB. `lsusb -d 04cb:` should list `04cb:ff80`, and `bazel run //transport:camera -- debug-flag status` prints `0xf7=01`.

To clear the flag, run `debug-flag disable` in a jig boot and end the same way. `0x80` stays `0x22`. By static analysis, installing the official 1.50 update from the camera's update menu also returns `0xf7`, `0x80` and `0x81` to `0x00`; no body has confirmed it.

## 3. Change the payload

The relinkers are `host/firmware/build-*-ram.sh` with their builders in `host/firmware/`. Payload code lives in `menu/` and `host/firmware/`. Several builders also check the hashes of their own inputs, so a change can need a builder change too. The RAM installer (`menu/ram_push_installer.c`) and its rendezvous are shared by every kind, and a fault there can hang the camera.

Run the change in the emulator before the camera. [The emulator README](../emulation/README.md) describes the RAM-push end-to-end harness and its `--unreviewed-relink` mode, which pushes a build that differs from the pins below.

## 4. Re-pin your change

The camera binary compiles the source and input digests of each payload from `transport/ram-push*-review.json` and refuses a relink that differs from them. Commit your change, then record the new digests:

```sh
bazel run //transport:ram-push-toggle-repin -- unreviewed "$(git rev-parse HEAD)" --changed
```

`--changed` re-pins every pin and the publication contract whose digest differs from the relink, marked unreviewed at that commit, and prints each one. The other kinds use `//transport:ram-push-cc-repin`, `//transport:ram-push-cc-icon-repin` and `//transport:ram-push-repin` (the recipe bank). A line saying the relink's manifest names a file the record lacks means that file needs a pin entry first ([Re-pin](../transport/README.md#re-pin)). The camera accepts unreviewed pins; this project's own pre-go gate `//transport:ram-push-review-verdicts` lists them.

## 5. Push it

Boot normally with the debug flag set, attach USB, and leave the camera idle at the shooting screen with no menu open and no card activity. Then run:

```sh
bazel run //transport:camera -- ram-session usb-toggle
```

It relinks the payload at its reference base and refuses before any USB transfer when the tree differs from the compiled pins. It then allocates camera RAM, relinks at the allocated address and installs, writing `alloc/`, `link/` and `push/` under `private-data/sessions/<UTC>-ram-session-usb-toggle/`. Success ends with `ram-push: installed`.

A result line containing `uncertain` means: send nothing further and remove the battery. If the relink fails after the allocation, the block stays allocated and uninstalled; `ram-free` with the printed state file frees it, and removing the battery also clears it.

## 6. Remove it

Switch off, remove the battery for at least ten seconds, and start again. A switch-off alone has left a payload's menu entries in place. `ram-unload` exists for the other kinds but is not needed, and the USB toggle refuses it.
