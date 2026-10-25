# Latent Image

Open-source host tools and firmware research for the Fujifilm X-M1, working toward improved autofocus and [named film recipes](spec/10-film-recipes.md).

Today, the project provides firmware analysis tools, a synthetic autofocus simulator, bounded ARM execution harnesses, recovery preparation tools, and detailed findings about X-M1 firmware and interfaces. It is not a demonstrated autofocus upgrade, a general-purpose firmware installer, or a faithful hardware emulator.

## Try it on your host

The supported build host is Linux x86_64 with [Nix](https://nixos.org/download/) and [devenv](https://devenv.sh/getting-started/). Bazel owns the build; the checked-in devenv environment supplies pinned dependencies. Initial setup downloads toolchains and dependencies.

```sh
git clone https://github.com/skeet70/latent-image.git
cd latent-image
devenv shell -- bazel build //...
devenv shell -- bazel test //...
devenv shell -- bazel run //af:simulate -- af/fixtures/peak-delayed.json
devenv shell -- bazel run //emulation:native-case -- --runner bazel-bin/emulation/native-runner emulation/fixtures/arithmetic.json
```

These commands perform no camera I/O. The simulation exercises a contrast-search controller against an artificial lens model; the native case checks ARM execution against a synthetic fixture. Neither demonstrates camera behavior. See [build integration](build-support/README.md) for toolchain and offline-cache details, and package READMEs for supported commands.

## Explore the tools and findings

| Area | Start here |
| --- | --- |
| Autofocus models and observations | [Sensor-backed process](spec/05-autofocus.md), [AF simulation](af/README.md), [low-interaction session kit](reports/af-session-kit.md), [recovered AF interfaces](reports/autofocus-static-map.md) |
| Firmware analysis | [Extraction and format findings](reports/firmware-extraction.md), [update-container contract](reports/update-container-rebuild.md), [matching-C pilot](matching/README.md) |
| ARM execution and emulation | [Bounded native harness](emulation/README.md), [full-system findings and limits](reports/full-system-emulation.md) |
| Recovery preparation | [X-M1 recovery kit and limits](recovery/README.md) |
| Film recipes | [Recipe specification](spec/10-film-recipes.md), [editable-bank design](reports/recipe-design.md), [shareable recipe text](#share-recipes-as-text) |
| Camera interfaces and read planning | [Transport tools](transport/README.md), [bounded read plans](readplan/README.md), [service and settings findings](reports/service-query-and-jig-reset.md) |

The [specification](spec/README.md) describes the intended system and acceptance criteria, not a list of shipped camera features. Technical reports distinguish static findings, host experiments, and hardware observations; addresses and behavior are firmware-version-specific.

## Share recipes as text

A recipe is one plain text file of `Field: Value` lines, using the fields the camera's menus show, so recipes can be written and shared on a computer and imported into the editable custom bank. The tool is host-only: import writes a bank file without operating a camera.

```sh
# one text file per recipe, C1 first; BASE is the declared payload base
bazel-bin/host/firmware/recipe-bank-text import BASE OUT_DIR recipe1.txt recipe2.txt ...
# export occupied slots as C1.txt, C2.txt, ... in slot order
bazel-bin/host/firmware/recipe-bank-text export BASE RECIPEA.BIN OUT_DIR
```

`BASE` is one 0x28-byte slot payload, given as 80 hex characters or as a file that holds it: the body's own C-payload defaults record when you have one, or an all-zero payload, which gives every field stored code 0. Every field a recipe omits takes its stored code from `BASE`, and the report names those fields.

The current text tool writes format-3 banks for the Classic Chrome-aware bank encoding. Classic Chrome is the sibling-derived approximation described in the [model report](reports/classic-chrome-model.md), not a stock X-M1 mode. The combined RAM bank is volatile and does not consume SD bank files; legacy flash banks refuse format 3. Text import is offline preparation, not an installation or a way to add persistence to that RAM deployment. The [format decision and compatibility matrix](reports/recipe-design.md#1811-classic-chrome-bank-encoding-needs-the-owners-decision) still need the owner's decision.

The keys are the camera's field names; matching ignores case and spacing, so `Film Simulation`, `FILM SIMULATION` and the X70 project's `film=` all name the same field. A line may use `:` or `=`; blank lines and lines starting with `#` are ignored. One recipe per file, and the file's name without its extension is the slot name when the file has no `Name` line.

| Key | Values |
| --- | --- |
| `Name` | up to 12 printable ASCII characters, the bank's slot-name bound |
| `Film Simulation` | `PROVIA/STANDARD`, `Velvia/VIVID`, `ASTIA/SOFT`, `B&W`, `SEPIA`, `PRO Neg. Std`, `PRO Neg. Hi`, `Classic Chrome` |
| `Dynamic Range` | `AUTO`, `100`, `200`, `400` |
| `Highlight Tone`, `Shadow Tone`, `Sharpness` | `-2`, `-1`, `0`, `+1`, `+2` (the menu names `SOFT`..`HARD` are accepted) |
| `Color` | `-2`, `-1`, `0`, `+1`, `+2` (the menu names `LOW`..`HIGH` are accepted) |
| `Noise Reduction` | `-2`, `-1`, `0`, `+1`, `+2` (`LOW`..`HIGH`) |
| `White Balance` | `AUTO`, `CUSTOM`, `FINE`, `SHADE`, `FLUORESCENT LIGHT-1`, `FLUORESCENT LIGHT-2`, `FLUORESCENT LIGHT-3`, `INCANDESCENT` |
| `ISO` | `AUTO`, `L100`, `200`..`6400`, `H12800`, `H25600` |

The X70 project's `film=`, `dr=`, `highlight=`, `shadow=`, `color=`, `sharpness=`, `nr=`, `wb=`, `iso=` and `name=` spellings are accepted where they mean the same thing on this body, and so are `dr200`, `Pro Neg Std` and the numeric ISO spellings `100`, `12800` and `25600`.

A file is rejected as a whole, with no substitution and no dropped field, when it names an unknown field; a feature the X-M1 1.50 does not implement (`Kelvin`, `WB Shift`, `Clarity`, `Grain`, `Color Chrome`, `D Range Priority`, exposure compensation); an X70 Auto ISO preset (`auto_iso1_*`..`auto_iso3_*`, `iso=auto1`..`auto3`); an unavailable film (`Eterna`, filtered monochrome) or white balance (`DAYLIGHT`, `CLOUDY`); a value outside the field's offered set; a field twice; an empty value; a line without a separator; or a name longer than 12 characters. Import reports each rejection and continues with the other files.

Export writes the canonical form: the `Name` line and exactly the fields whose stored code differs from `BASE`, in the order of the table above. Importing an exported file over the same `BASE` reproduces the slot payload byte for byte. Each occupied slot exports its saved payload. A slot whose payload leaves fields the text cannot spell (for example ISO's second AUTO row, stored code 0x1b) or whose bytes differ from `BASE` outside the nine fields is reported and skipped rather than approximated.

Export requires a format-3 bank. It refuses a legacy D3 bank before exporting any slot; legacy code `0xd` denotes D3 A, not Classic Chrome.

Both commands exit 0 when they write their output, reporting each rejected file or skipped slot on stdout; they exit 1 for a run they refuse (an unreadable `BASE`, more files than slots, or nothing imported or exported).

## Firmware extensions

Apply these at your own risk. Each has worked on at least one physical X-M1, but nothing guarantees it will not brick your camera. Start from the exact firmware version and checksum listed for the extension. If you don't know what your camera is running, the surest way to be certain is to install the listed stock firmware first with Fujifilm's [ordinary update procedure](https://www.fujifilm-x.com/global/support/download/firmware/cameras/x-m1/).

| Extension | Extension manifest | Base firmware | Built `FWUP0002.DAT` SHA-256 | Known limits |
| --- | --- | --- | --- | --- |
| Unlock the PRO Neg. Std and PRO Neg. Hi film simulations that already exist in the stock firmware | [`extensions/rc0.json`](extensions/rc0.json) | Official X-M1 1.50 `FWUP0002.DAT`, SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490` | `acb0fbbb9507421f48a6254920cafb5ce20e1fd6bf821bc80c912c1f99c29ca8` | Adds both modes to SHOOTING MENU > FILM SIMULATION. The Q menu's film-simulation cell still offers the stock five. [Patch report](reports/filmsim-patch-01.md) |

An extension manifest names the base firmware, the patch sets and other parts to apply, and the expected result. It is not a firmware image: you build the patched firmware yourself from the official DAT you download.

### Build the patched firmware

Download the base firmware from [Fujifilm's X-M1 page](https://www.fujifilm-x.com/global/support/download/firmware/cameras/x-m1/) into `private-data/` in your checkout, then run the command with the row's extension manifest as the first argument:

```sh
devenv shell -- bazel run //extensions:build-extension -- \
  extensions/rc0.json private-data/FWUP0002.DAT private-data/rc0-build
```

The output directory must be new. The tool works out which firmware parts the extension changes and runs the matching rebuild steps. It refuses the build if the stock DAT, any part or the result differs from the hashes in the manifest, or if the manifest needs a step it doesn't support. It writes `FWUP0002.DAT` into the output directory and prints `PASS` with its SHA-256 only when every check succeeds. On any failure it removes the output directory, and you should not install anything. [The extensions README](extensions/README.md) describes the manifest format and the steps.

### Install it

1. Format a card in the camera, then copy the built `FWUP0002.DAT` to the card's root as its only file, keeping that exact name.
2. Use a fully charged battery, close the card and battery doors, and disconnect USB.
3. Power on while holding DISP/BACK, then check the version screen. It should offer an update from 1.50 to 1.50. If the screen differs or refuses the update, power off. Nothing has been written yet.
4. Select BODY and confirm each screen. The final confirmation starts the write. From then on, don't press anything, open a door or remove power until the camera reports completion and asks you to switch it off.
5. Switch off, remove the card and start normally. Check that the menus and photography work and that the new film simulations appear.

[The install procedure](reports/install-procedure.md#6-the-procedure-step-by-step) explains each step, the [abort points](reports/install-procedure.md#7-abort-points-and-the-point-of-no-return), and what an [interrupted update](reports/install-procedure.md#8-interruption-residues-and-which-are-recoverable-by-repeating-the-update) leaves behind. To return to stock, install the official base DAT the same way. That direction has not yet been tried on hardware. If the camera no longer starts, read [the recovery kit](recovery/README.md) before trying anything else.

## Contribute

Use [GitHub Issues](https://github.com/skeet70/latent-image/issues) to find open work, report bugs, or propose research. Check existing issues before starting, and include the tool, firmware version where relevant, and a reproducible example that does not disclose proprietary inputs or private camera data.

Read the relevant package README and [project principles](docs/principles.md) before making changes. The [specification](spec/README.md) records requirements, [technical reports](reports/) explain findings, and the [evidence ledger](docs/evidence.jsonl) records the scope of supporting observations. Synthetic host fixtures let contributors work without a camera or firmware image.

## Camera safety and private inputs

Live camera access is separate from the host quickstart and exposed through explicit commands under `//transport:camera`. Camera operations can change state or damage a device; passing host tests or emulator cases does not establish safe installation, successful recovery, or hardware acceptance. Recovery tooling is not a universal repair method. Review the [operation classes](spec/07-governance-and-handoff.md#32-operation-classes) and [hardware safety rules](spec/07-governance-and-handoff.md#33-standing-rules) before connecting hardware.

Publish tools, implementations, and detailed firmware findings, not proprietary inputs. Firmware, extracted proprietary code/data, dumps, calibration, private identities, raw camera logs, and local artifacts belong under ignored `private-data/`, not in issues or pull requests. Firmware-backed commands require independently obtained inputs and their documented identity checks. See the [publication boundary](docs/principles.md#publication-and-asset-boundaries) and each tool's input requirements.

## License and credits

Original Latent Image software is licensed under [AGPL-3.0-only](LICENSE). Commercial use is allowed under its conditions, including applicable source-sharing obligations for distribution and modified versions offered over a network.

The inherited emulator implementation in `emulation/xm1-upstream/` comes from **[jitl (justjake)](https://github.com/justjake)'s x70view**, with X-M1 adaptations. It is redistributed with permission requiring credit and retains its separate terms; it is not relicensed under the project's AGPL grant. Upstream components and dependencies retain their own licenses. See [LICENSING.md](LICENSING.md) and [third-party attribution](third_party/README.md) for detailed permissions, notices, and artifact-specific redistribution obligations.

## AI assistance

This project makes heavy use of AI assistance in its research, code, and documentation.
