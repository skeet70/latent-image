# Build and test your own X-M1 feature

> **Status.** Every camera operation underneath this guide (`read-config`, `commit-f7-enable`, `ram-alloc`, `ram-relink`, `ram-push`, `read-ram`) has run on the maintainer's X-M1, and the shared RAM installer experiments use has installed payloads there. The `debug-flag` and `ram-session` commands that combine them, and experiments themselves, have only run in the emulator so far: the maintainer's body is away for a hardware repair.

You write a feature as an **experiment**: a small Rust or C program in `experiments/<name>/`, plus a `hooks.json` naming the stock firmware functions it should run in front of. The tools link it into 32 KiB of camera RAM and redirect those functions through it. You try it in the emulator, then push it to your camera over USB. Nothing is written to the camera's flash, and taking the battery out removes it.

You combine the project's code with Fujifilm's official firmware, which you download yourself. Run every command from your checkout inside `devenv shell`. Everything you generate goes under the ignored `private-data/`; keep it out of issues and pull requests.

## 1. Set up once

You need Linux x86_64 with [Nix and devenv](../README.md#try-it-on-your-host), an X-M1 on firmware 1.50, a USB micro-B cable, and a battery or dummy-battery supply you can pull with the camera on.

Download the official 1.50 `FWUP0002.DAT` from [Fujifilm](https://www.fujifilm-x.com/global/support/download/firmware/cameras/x-m1/) (SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`) to `private-data/firmware/x-m1-1.50/FWUP0002.DAT`, then decode the three firmware images experiments hook into:

```sh
D=private-data/firmware/x-m1-1.50/FWUP0002.DAT
R=private-data/analysis/x-m1-1.50
mkdir -p "$R/lz"
bazel run //recovery:inspect-firmware -- extract "$PWD/$D" "$PWD/$R/stock-components"
bazel build //lz:fflzd //host/firmware:rebuild-update
bazel-bin/lz/fflzd "$R/stock-components/segment-4.bin" 0 "$R/lz/id4-0061c400.bin"
bazel-bin/lz/fflzd "$R/stock-components/segment-5.bin" 0 "$R/lz/id5-00bf1a00.bin"
bazel-bin/host/firmware/rebuild-update extract-plain "$D" "$R/lz/plain-01100000.bin"
```

These are image 4 at `0x0061c400`, image 5 at `0x00bf1a00` and the plain image at `0x01100000`, the addresses they run at. The build checks their hashes every time.

EMULATOR_SETUP

## 2. Write an experiment

Start from the template:

```sh
cp -r experiments/hello experiments/my-feature
```

[`experiments/hello`](../experiments/hello/) hooks `0x00bfb028`, the stock dispatcher that every MENU open and key press goes through, and counts the calls in a static named `HELLO`. An experiment is:

- **`hooks.json`**: up to 8 hooks, each a stock function's entry address and the handler to run first.

  ```json
  { "hooks": [ { "address": "0x00bfb028", "handler": "hello_on_dispatch" } ] }
  ```

- **`lib.rs`** (a `no_std` crate root) and/or **`*.c` and `*.S` files**. C includes [`experiments/sdk/experiment.h`](../experiments/sdk/experiment.h).

A handler receives the hooked function's registers at entry, `r0`-`r3` being its first four arguments. Whatever you write back is what the stock function sees; it always runs after your handler returns.

```rust
#[no_mangle]
pub extern "C" fn my_handler(regs: &mut Regs) { /* regs.r[0] is the first argument */ }
```

```c
#include "experiment.h"
void my_handler(struct experiment_regs *regs) { /* regs->r[0] is the first argument */ }
```

To call stock code, cast its address to a function pointer, for example `let f: extern "C" fn(u32) -> u32 = unsafe { core::mem::transmute(0x00d52684usize) };`. The [reports](../reports/) name the functions the project has mapped; [the AF map](../reports/autofocus-static-map.md), [input map](../reports/input-map-150.md) and [menu ownership](../reports/recipe-bank-ui-ownership.md) are good starting points. To read the code yourself, disassemble an image:

```sh
llvm-objcopy -I binary -O elf32-littlearm --set-section-flags .data=code,alloc,load "$R/lz/id5-00bf1a00.bin" private-data/id5.elf
llvm-objdump -D --triple=armv7-none-eabi --adjust-vma=0x00bf1a00 private-data/id5.elf > private-data/id5.dis
```

Limits:

- Code and data together fit in 20 KiB. There is no heap and no standard library.
- A handler runs on the caller's stack, on whichever core called the function, with interrupts as the caller had them. Keep it short and don't block.
- A hooked function's first two instructions are moved into your experiment, so they must not use the PC or branch. The build checks this and names the instruction if it fails; hook a different function or one a few instructions in.
- To read data over USB later, store it through the non-cacheable alias (address `| 0x40000000`), as `hello` does; `EXPERIMENT_UNCACHED` does it in C.

## 3. Try it in the emulator

EMULATOR_RUN

## 4. Put it on your camera

### Once: set the debug flag

The camera takes RAM pushes over its USB service mode, which adjustment byte `0xf7`, the debug flag, turns on. With it set, every USB attach in a normal boot starts the service device `04cb:ff80` instead of the usual file-transfer mode. Setting it rewrites the camera's adjustment area, so take a backup first:

1. Enter jig mode: camera off, USB attached, hold the shutter fully down and the Up key, switch on, and keep both held until the screen is blank and the green LED is lit. `lsusb -d 04cb:` lists `04cb:ff80`.
2. `bazel run //transport:camera -- debug-flag backup` saves the whole adjustment area, your body's calibration, under `private-data/sessions/`. Copy that directory somewhere safe.
3. `bazel run //transport:camera -- debug-flag enable` ends with `commit-f7-enable: committed`.
4. Pull the battery (not the POWER switch, which resets date, time and language after a jig session), then unplug USB.

`debug-flag status` shows the flag; `debug-flag disable`, in a jig boot, clears it again.

### Each time: push

Boot normally, attach USB and leave the camera at the shooting screen with no menu open. Then:

```sh
bazel run //transport:camera -- ram-session experiment my-feature
```

It builds your experiment, refuses before touching the camera if a check fails, then allocates the RAM, links your code there and installs the hooks. Success ends with `ram-push: installed`. The output directory's `link/experiment.json` lists where each of your symbols landed.

### Read what it recorded

Point `read-ram` at a symbol's address from `experiment.json` with `0x40000000` added (so `0x00a8f0c8` becomes `0x40a8f0c8`), written as 8 lowercase hex digits, for up to 248 bytes:

```sh
(umask 077 && printf '{"windows":[{"index":0,"address":"0x40a8f0c8","length":24,"purpose":"HELLO"}]}\n' > "$PWD/private-data/read-plans/my-feature.json")
bazel run //transport:camera -- read-ram "$PWD/private-data/read-plans/my-feature.json"
```

The bytes land in that session's `ram/` directory. `--repeat N --interval-ms M` watches them change.

### Remove it

Pull the battery for ten seconds. A switch-off alone may leave the experiment running, and it saves any settings the experiment changed.

## What the tools check, and what they don't

Before anything reaches the camera, the build checks that each hooked function's first two words are what Fujifilm's 1.50 firmware holds there and can be moved, that your code fits, and that nothing else is written. On the camera, the installer checks every word it replaces against the same values and refuses if any differs, so an experiment can't install over a different firmware version or another payload.

Nothing checks what your code does. Things that can go wrong:

- **A crash or hang.** Pull the battery; the camera starts on its own firmware.
- **Settings.** Stock functions you call can change camera settings, and a normal switch-off stores them. Settings can be reset from the camera's menus.
- **Persistent storage.** Calling stock code that writes flash, the adjustment area or firmware can make changes that survive a battery pull, up to a camera that no longer starts. Don't call storage, update or calibration functions unless you know exactly what they write; [the recovery kit](../recovery/README.md) covers what the project can repair.
- **Push timing.** A function that is running at the instant its hook goes in can crash. Push with the camera idle, as above.

## The project's own payloads

The recipe bank, Classic Chrome and the USB toggle use the same route as experiments, with review records the camera checks: `ram-session recipe-bank|classic-chrome|classic-chrome-icon|usb-toggle`. Their inputs and the pin and waiver options are in [the transport README](../transport/README.md#guided-commands) and [the extensions README](../extensions/README.md#ram-payload-inputs).
