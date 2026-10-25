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
| Film recipes | [Recipe specification](spec/10-film-recipes.md), [editable-bank design](reports/recipe-design.md) |
| Camera interfaces and read planning | [Transport tools](transport/README.md), [bounded read plans](readplan/README.md), [service and settings findings](reports/service-query-and-jig-reset.md) |

The [specification](spec/README.md) describes the intended system and acceptance criteria, not a list of shipped camera features. Technical reports distinguish static findings, host experiments, and hardware observations; addresses and behavior are firmware-version-specific.

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
