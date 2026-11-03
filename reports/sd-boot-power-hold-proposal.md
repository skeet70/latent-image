# Camera-run proposal: power-hold blink card

Not run. This is a written proposal under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs); it needs the owner's go naming the operation and the image digest below. The analysis it tests is [section 2.62 of the card-entry report](sd-boot-card-entry.md#262-the-fffw-unbricking-note-and-the-power-hold).

## What it decides

tiredboffin's note says the boot ROM's copy of `ffsys.bin +0x10` bit 0 into `0xfff88084` powers off an EXR body during a slow card load, and that setting the bit fixes it. On this body the hypothesis that the note explains the on-off flick is open (E-SDBOOT-NOTE-FLICK-TEST-150). With the bit set the ROM never releases `0xfff88084` on the mode-1 success path, so:

- a cold start that blinks makes the release the cause of cold-start failure, and the flick becomes a power-hold effect;
- cold starts that stay dark refute that, leaving reset-time selection and card bring-up;
- with the payload running, one move of the switch to OFF shows whether bit 0 holds supply against the switch, which binds the power-hold meaning on X-M1.

## Operation and bytes

| Item | Value |
| --- | --- |
| Body | XM1-body-01, 1.50 with RC0, healthy |
| Class | H2: ROM SD boot of a project RAM payload; no internal-media, configuration or card write exists in the ROM path or the payload |
| Image | `private-data/analysis/x-m1-1.50/bootrom/power-hold-blink-startup-card/diagnostic-sd.img`, 17,826,304 bytes, SHA-256 `aec1d2b3a6b1521d10cb751a32732a8f23bc48ca4eb8152d9e353cac6886d27d` |
| `ffsys.bin` | SHA-256 `c393cd3cabdedb7680a92855539d4a9bc9b7ec18dcb09b9b73ec4e6ad5c6c19e`; stock file with `+0x10` = `0x301` |
| `ffboot.bin` | SHA-256 `30b8f5c0b53323a78cd2bdf971b2849b7f409b40132af73f328c9ca1e880de03`, the startup-restored blink payload run on this body before |
| Difference from the run image `4f1c632c…` | two bytes, image offsets `0x10` and `0x124210` (raw-route and FAT copies of `+0x10`), `0x00` to `0x01` |
| Build | `devenv shell -- bazel run //host/firmware:build-boot-card -- --power-hold build private-data/firmware/x-m1-1.50/FWUP0002.DAT bazel-bin/host/firmware/diagnostic-blink-startup.bin NEW_PRIVATE_DIRECTORY` |
| Verify | `devenv shell -- bazel run //host/firmware:build-boot-card -- --power-hold verify private-data/firmware/x-m1-1.50/FWUP0002.DAT bazel-bin/host/firmware/diagnostic-blink-startup.bin private-data/analysis/x-m1-1.50/bootrom/power-hold-blink-startup-card/diagnostic-sd.img` |
| Independent review | required before the run because the change alters a ROM boot control; status in the review record below |

The value is not guessed (spec 07 section 3.3): 1 is the value the ROM itself writes at `0x800000ec` on every reset, and the payload writes the same 1 within its first instructions.

## Preparation

Write the whole image to the expendable 7.2 GB SDHC card with the guarded write and readback block of [card-entry section 2.8](sd-boot-card-entry.md#28-spare-card-preparation-and-conditional-camera-procedure), `EXPECTED` set to the digest above. Stop unless it prints PASS. Remove and reinsert the card in the reader and repeat the 17,826,304-byte comparison. Fully charged battery; USB disconnected; external video of the rear LED with audio, running throughout.

## Procedure

**A. Cold starts, at most five.** For each: battery out for at least 30 seconds, battery in, switch OFF, card inserted. Hold Fn plus the inward sub-command-dial press, move the switch ON once, keep both held 10 seconds, observe 60 seconds. If the payload cadence (three short red intervals and one long, repeating) appears, go to B. If dark at 60 seconds, switch OFF, remove the battery and start the next cold start. Do not toggle in A.

**B. Switch test, once, only while the cadence runs.** Release the buttons. After one full cadence cycle, move the switch OFF once and observe 20 seconds without touching anything. Record whether the cadence continues. Then remove the battery, whatever happened. The payload writes `0xfff88084` = 1 itself, so B binds the bit's meaning on either card; only A tests the card change.

**C. Toggled control, only if all A starts stayed dark.** From switch OFF with the battery in, hold Fn plus the inward dial press and toggle at about 0.75 second per transition, at most 40 transitions, ending ON; stop at the first LED. If the cadence runs, do B. If no LED by the 40th transition, switch OFF and remove the battery.

**Return.** Normal card in, ordinary start without held buttons, one shot and playback, check date/time, language and the C slot.

## Outcomes

| Observation | Reading |
| --- | --- |
| Any A start shows the cadence | the release at `0x80000330` is the cold-start failure; the note explains the flick on this body |
| All A starts dark, C enters | release refuted as the cold-start cause; the flick is a selection or bring-up effect (issues #60, #63) |
| All A starts dark, C does not enter | no reading about the hold; the card may differ in some other way, stop for host analysis |
| B: cadence continues with the switch OFF | `0xfff88084` bit 0 holds supply against the switch on X-M1 (power-hold meaning hardware-observed) |
| B: cadence stops within 20 s | the switch removes supply regardless of bit 0; the note's mechanism cannot act while the switch is ON |

## Stops and risks

Stop on any unexpected display, sound, heat or smell: remove the battery. A blinking body that ignores the switch is an expected outcome of B, not a fault; battery removal ends it. With bit 0 held, a ROM hang in an apply wait may also keep the body powered after switch OFF; battery removal ends that too. The ROM path issues no write command to the card or internal media and the payload touches only `0xfff88084` and the LED GPIO, so no persistent state changes. The body's 1.50 adjustment backup exists in two locations (E-ADJ-BACKUP-GAP-READ-150).

## Review record

Independent review of commit `6003a71` and image `aec1d2b3…`, 2026-10-24: **APPROVE WITH LIMITATIONS**, no blocking finding. The reviewer reproduced the two-byte difference, censused every ROM store to `0xfff88084` (including the mode-7 teardown caller and reader `0x80000b04`), confirmed `+0x10` has no reader beyond bit 0, bit 1 and bits 10:8, confirmed the apply always returns 0 so its faults are hangs after `0x80000330`, and re-ran `verify` on the existing startup-restored, after-apply, before-memory-wait and halt-control images plus `--power-hold verify` and both negative controls. Residual unknowns: what bit 0 controls on X-M1, whether supply follows bit 0, the switch or a companion controller, whether the apply waits complete on cold starts, which ROM route entries use, and the effects of the unchanged MBR-overlap register words. The reviewer classed the value as not guessed under spec 07 section 3.3. The review is not an owner go.

## Running it on another X-M1

The fast-toggle requirement has only been seen on XM1-body-01 (E-SDBOOT-ENTRY-TIMING-150). An X-M1 with a working SD slot can test both the requirement and the hold bit with two cards that differ only in `+0x10` bit 0. Nothing here installs firmware: the ROM path issues no write to the card or internal memory, and the 512-byte card program sets `0xfff88084` = 1 and blinks the red LED. Whether to run it on your camera is your decision; read [Stops and risks](#stops-and-risks) first.

### Build the two cards

You need Linux x86_64 with Nix and devenv ([host setup](../README.md#try-it-on-your-host)), an expendable SD card of at least 18 MB and a host card reader. Download the official X-M1 1.50 firmware from [Fujifilm's X-M1 page](https://www.fujifilm-x.com/global/support/download/firmware/cameras/x-m1/) into `private-data/` in your checkout. The builder refuses any other file; its SHA-256 is `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`. From the repository root:

```sh
mkdir -p private-data/scratch
devenv shell -- bazel build //host/firmware:diagnostic-blink-startup
devenv shell -- bazel run //host/firmware:build-boot-card -- build private-data/FWUP0002.DAT bazel-bin/host/firmware/diagnostic-blink-startup.bin private-data/scratch/baseline-card
devenv shell -- bazel run //host/firmware:build-boot-card -- --power-hold build private-data/FWUP0002.DAT bazel-bin/host/firmware/diagnostic-blink-startup.bin private-data/scratch/power-hold-card
```

Each command writes `diagnostic-sd.img` (17,826,304 bytes) into its directory and prints its SHA-256:

| Card | `diagnostic-sd.img` SHA-256 |
| --- | --- |
| Baseline: stock `ffsys.bin`, bit 0 clear, the image run on XM1-body-01 | `4f1c632cd9ea6247899e2e97c2ed4ee5d3a1b179ea33b28908d4b974090ddbb0` |
| Power hold: bit 0 set | `aec1d2b3a6b1521d10cb751a32732a8f23bc48ca4eb8152d9e353cac6886d27d` |

Stop if either digest differs. Write each image raw to the card with the guarded write-and-readback block in [card-entry section 2.8](sd-boot-card-entry.md#28-spare-card-preparation-and-conditional-camera-procedure), setting `IMG` to the image path and `EXPECTED` to its digest. Stop unless it prints `PASS`. Don't mount, format or copy files onto the card afterward: formatting loses the raw layout.

### Procedure

Fully charged battery, USB disconnected. Film the rear LED with sound throughout if you can. The card program's pattern is three short red intervals then one long one, repeating.

1. **Baseline card, at most five cold starts.** For each: battery out for at least 30 seconds, battery in, switch OFF, card in. Hold Fn and press the rear sub-command dial inward, move the switch ON once, keep both held for 10 seconds, then watch for 60 seconds. Don't toggle. Note dark, the LED pattern or a normal boot. Remove the battery after each start.
2. **Power-hold card, the same five cold starts.**
3. **Switch test, once, only while the pattern runs.** Release the buttons, wait one full cycle, move the switch OFF once and watch for 20 seconds without touching anything. Then remove the battery.
4. **Toggled start, only if every start in steps 1 and 2 stayed dark.** With the power-hold card and the battery in, hold the same two controls and move the switch about every 0.75 seconds, OFF transitions included, at most 40 times, ending ON. Stop at the first LED and do step 3. If nothing lights by the 40th transition, switch OFF and remove the battery.
5. **Return.** Normal card in, ordinary start with no buttons held, one shot and its playback, then check date, time and settings.

### What the results mean

| Observation | Reading |
| --- | --- |
| The baseline card enters on a cold start | the toggle requirement is particular to XM1-body-01, so this camera cannot test the hold bit against it |
| Baseline dark, power-hold card enters on a cold start | the release at `0x80000330` is the likely cause of the requirement |
| Both dark on cold starts, power-hold card enters when toggled | the release does not explain the requirement |
| Both dark on cold starts, nothing enters when toggled | no reading; host analysis follows |
| The pattern continues after switch OFF | bit 0 keeps the camera powered against the switch |
| The pattern stops within 20 seconds of switch OFF | the switch removes power regardless of bit 0 |

### What to report

Post on [#60](https://github.com/skeet70/latent-image/issues/60): the body firmware version, the card make and capacity, both printed image digests, the outcome of each start in order, the switch-test result and whether the return check passed. Video helps when the pattern is hard to count.
