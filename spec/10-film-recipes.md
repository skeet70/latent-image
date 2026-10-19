# Film recipes

## 19. Named film recipes in the stock menu

### 19.1 Goal and relation to autofocus

Add named film recipes to the X-M1's film-simulation menu, selectable and persistent like the stock simulations. Autofocus ([section 11](05-autofocus.md)) remains the primary mission; the owner's feature priority is compiled D3 rows with Q-menu exposure and a separate seven-slot editable bank (E-RECIPE-OWNER-PRIORITY-150). [GitHub Issues](https://github.com/skeet70/latent-image/issues) owns current scheduling and candidate readiness. Recipes share the firmware map (G2), checksum binding, recovery evidence and the persistent-installation decision (G7) with autofocus, and use none of the AF timing work. Wanting a persistent menu feature does not make flashing a substitute for the temporary execution loop that AF research requires.

A recipe here means a named preset: one base processing mode plus values for the X-M1's existing image parameters. A recipe from another camera that needs features the X-M1 lacks (for example Classic Chrome, grain, Color Chrome or clarity) is rejected or explicitly labelled as an approximation. The tooling never substitutes silently. Classic Chrome as new processing is [section 19.5](#195-classic-chrome-rc2); until it is accepted, recipe tooling keeps rejecting it as a base.

### 19.2 Current observations

| Observation | Evidence class and scope |
| --- | --- |
| The stock menu offers `PROVIA/STANDARD`, `Velvia/VIVID`, `ASTIA/SOFT`, `B&W` and `SEPIA`, without PRO Neg or filtered B&W. The RC0 prefix install exposes seven entries including PRO Neg Std and PRO Neg Hi; film simulations/settings remain available after recovery by owner report. | Stock controls owner-reported on 1.00 and 1.50 (E-FILMSIM-150-OWNER, E-RECIPE-150-OWNER, [1.50 record](../docs/camera-menus-human-recorded-1.50.txt)); seven entries hardware-observed 2026-10-09 (E-FILMSIM-INSTALL-150); post-recovery retention owner-reported (E-RECOVERY-V5-RETENTION-150). |
| The mode dial's `C` position stores one unnamed custom setting. C-slot color +2 remains after recovery by owner report. | User-reported; fields statically mapped (E-CUSTOM-SETTING-MAP-150, [field map](../reports/custom-settings.fields.json)); C-slot cache fields body-observed (E-ADJ-READ-150); post-recovery retention E-RECOVERY-V5-RETENTION-150. This does not accept a new recipe bank. |
| Fuji's 1.01–1.50 release notes add no film simulation. | Official support page preserved with the 1.50 DAT. |
| The 1.50 payload contains an ASCII table naming `PRO Neg. Std`, `PRO Neg. Hi` and `MONOCHROME+R/+Ye/+G` alongside the stock names. | Static inference; see [the payload survey](../reports/firmware-extraction.md#payload-content-survey). Names alone don't show the processing exists in this build. |

Before designing recipes, record privately the installed body firmware version and every film-simulation, image-quality and custom-setting menu with its value range. That record defines the X-M1 recipe parameter model. Don't fill it from the manual of another model.

### 19.3 Tiers

| Tier | Change | Prerequisite evidence |
| --- | --- | --- |
| RC0: expose unoffered processing | Add menu entries for processing modes the build already implements but does not offer. In 1.50 this means PRO Neg Std/Hi: a menu-list change plus an availability-matrix change ([mapping](../reports/film-simulation.md#processing-by-value)). Filtered B&W is excluded because it reaches plain B&W processing. | The menu/enumeration/develop dispatch and distinct PRO Neg [tone records](../reports/film-simulation.md#tone-curves-and-the-curve-record-table) are mapped; adjustment selectors are read (E-ADJ-READ-150). The installed prefix shows seven entries (E-FILMSIM-INSTALL-150), and film simulations/settings survive recovery and ordinary startup by owner report (E-RECOVERY-V5-RETENTION-150). Detailed rendered-output acceptance and downstream ISP programming remain unverified. Installed-1.50 byte correspondence is established apart from 781 bytes, none in an AL-condition word and 317 in 41-90% AL-density data blocks (E-DUMP-RAM-150). |
| RC1: named recipes | Compiled film-simulation rows that apply stock base processing plus explicit parameters, with D3's two rows and Q exposure retained in the owner's candidate (E-RECIPE-OWNER-PRIORITY-150). | Host compiler and native cases exist, but final composed bytes, label rendering, recorded settings, persistence and stock regressions still require the checks in 19.4. Nothing is installed. |
| RC1b: editable recipe bank | Seven named editable slots with Save Current, Recall, rename, edit and reset; stock C preserved, no auto-update, supported X-M1 parameters only. | SD-backed candidate sources, host tooling and bounded native cases exist. Their accepted scope remains in reports and evidence; remaining placement, publication, state-apply, navigation and file/media work is tracked in [GitHub Issues](https://github.com/skeet70/latent-image/issues). Host acceptance does not accept final image composition or installed behavior; the card dependency and exact installation require the owner's go. |
| RC2: new processing | New tone curves or colour tables, or making existing unreachable branches real (for example the filtered-B&W colour matrices, which in 1.50 need the monochrome filter index set). | Understanding of the image-processing pipeline and hardware tables. Classic Chrome is the one justified RC2 item ([section 19.5](#195-classic-chrome-rc2)); anything else stays out of scope until separately justified. |

Each tier states the exact firmware version it targets and refuses any other version.

The recipe candidate combines RC1/D3 and RC1b. D3 has a host compiler, assembled hook and recorded native behavior cases; the editable bank has host tooling, target sources and modeled storage/boot cases. Neither has an accepted complete composed firmware image or installed behavior. The historical combined container omits the bank executable and boot publication (E-RECIPE-PREBOOT-COMPOSITION-LIMIT-150); its old install procedure is withdrawn. [GitHub Issues](https://github.com/skeet70/latent-image/issues) tracks remaining work separately from source/runtime acceptance and modeled boundaries in reports and evidence. A host plan or modeled pass is not an installable candidate.

[The recipe design](../reports/recipe-design.md) owns the parameter model, film-simulation presentation, recipe-index field and compiled layouts; [the recipe-set schema](../schemas/recipe-set.schema.json) defines host input. The Haskell tools `//host/firmware:build-compiled-recipes` and `//host/firmware:build-recipe-bank` generate the candidate artifacts. [The reach report](../reports/named-recipe-reach.md) binds the storage limits: no stock recipe-name storage, a corrected 516-byte image-4 decode tail with 192 bytes used by the unlock (E-FILMSIM-PREFIX-WIDENING-150), and separate placement proof required beyond accepted extents. The original arbitrary-label RC1 design budgets about 1.05 KB in image 4 plus 660 bytes in image 2 and four hooks; that design has no accepted placement. D3 and the seven-slot bank have their own current designs and ownership requirements.

### 19.4 Verification and installation

1. Offline: show the patched menu and dispatch paths statically and, where practical, in [selected-function emulation](03-phases-and-gates.md#3b-native-function-harness). Diff the patched image against stock and explain every changed byte. Recompute the bound checksum with an independently checked algorithm.
2. On camera, if a temporary loop (G4/G5) can exercise the change without flashing, test it that way first.
3. Persistent installation is a [persistent-state operation](07-governance-and-handoff.md#32-operation-classes): the owner's go naming that exact image, one independent review of the image diff, the preserved body-specific data, and the recovery level required by [section 10](04-recovery.md). The user has stated that a modified firmware flash is an expected eventual step.
4. Acceptance: each recipe is selectable, survives a power cycle, and produces images whose recorded settings match its definition; stock modes produce unchanged output on a fixed scene; no other menu or setting regresses. For RC0, independent reference output for the newly offered modes does not exist on this body; record that limitation rather than borrowing sibling-model images as ground truth. The installed RC0 list is selectable (E-FILMSIM-INSTALL-150), and film simulations/settings remain available after recovery and ordinary startup by owner report (E-RECOVERY-V5-RETENTION-150). The per-mode rendered-output, recorded-settings match and fixed-scene comparison remain separate acceptance work.
5. Reference intent for the bank's UX comes from the linked Fuji X Weekly article's manual screenshots (E-RECIPE-REFERENCE-WORKFLOW-150): EDIT/SAVE CUSTOM SETTING with 7 CUSTOM banks; per bank Save Current Settings, Edit Custom Name, Edit and Reset; parameter-item editing confirmed by explicit OK/CANCEL on DISP/BACK; a separate SELECT CUSTOM SETTING to recall. This shapes the acceptance wording below only. It is not evidence of any X-M1 surface, and the X-M1's own menus and storage decide what exists.
6. Acceptance items for the recipe candidate, per part, with designed, built and installed recorded as separate states. RC1/D3: the two compiled rows appear in the film-simulation lists on every surface the patch covers including the Q cell; each is selectable; each image's recorded settings match its definition; both survive a power cycle; the compiled name renders as list-row text; stock rows and uncovered surfaces are unchanged. RC1b bank: Save Current captures the record's 81 bound fields into the named slot and Recall applies them, preserving every bound value, with canonical unbound +27 bits 2-7 left zero and no live or C-position write before OK or on cancel; the recalled images' recorded settings match the slot; a slot can be renamed on camera with explicit OK/CANCEL confirmation; Edit is per-slot and staged, with every parameter-item change confirmed by explicit OK/CANCEL on DISP/BACK; an immediate-save, reduced-UX or otherwise narrowed substitute is not acceptable; Reset behaves as the reference intent describes; slots survive a power cycle; the stock C dial slot and CUSTOM RESET behaviour are untouched; nothing auto-updates a slot; no other menu or setting regresses.

If both features ship as one image, keep the recipe and AF changes separately reviewable and separately revertible in the patch description.

### 19.5 Classic Chrome (RC2)

Add Classic Chrome to the 1.50 film-simulation menu as new processing. The result is labelled an approximation of Classic Chrome as Fujifilm shipped it on EXR Processor II with X-Trans CMOS II (X-T1 5.52, X70 1.10). The X-M1 has the earlier X-Trans CMOS sensor and no Classic Chrome of its own, so no X-M1 ground truth exists. The reference is sibling-derived by necessity, and every result says so.

#### 19.5.1 Starting observations

Host reads from 2026-10-18, static, reproduced by `//host/firmware:classic-chrome-model tables` from the pinned inputs and recorded as E-CC-TONE-LINEAGE-150, E-CC-XT1-FALLBACK-552, E-CC-XT1-TONE-552 and E-CC-MATRIX-TRANSFER-150. No X-Trans CMOS I firmware carries Classic Chrome: the X-Pro1 3.82 and X-E1 2.71 lack it as the X-M1 1.50 does (E-CC-XTRANS1-SURVEY).

| Observation | Inputs |
| --- | --- |
| The 37 X-M1 tone records at id4 `0x824bc8` are byte-identical after the key to X70 records 0-36. The X70 table starts with count 42 at `0xb624e0` and has records at `0xb624e8`; it appends keys 37-41, and Classic Chrome selects key 38 at every DR setting. | X-M1 1.50 id4; X70 1.10 section4; x70view `research/film-simulation.md` |
| The X-T1 5.52 DAT carries a complemented fallback adjustment image at DAT offset `0xe025c` (`X-T1` at +0x15c, `0xaa55` at +0x202), in the same layout as the X-M1's. Its colour matrices 1-5 and 9 at `0xacc3` and its chroma ids 1-5 at `0xb077` equal the X70 body values x70view reports; matrix 9 is Classic Chrome's. The X70's Classic Chrome chroma entry (selector 11) is at `0xce9a`, outside the 10-entry `0xb077` table. | X-T1 5.52 DAT, SHA-256 `4cf56ba6abb4298a844422c016289228bb0acce0a2ae2c6755c05fffb38e92db`, `https://dl.fujifilm-x.com/support/firmware/x-t1-552-9cy17ibx/FWUP0005.DAT` |
| X-M1 matrices 1-5 and chroma ids 1-5 differ from the X-Trans II values, so Fujifilm retuned them per sensor. Composing the X-Trans II film matrix with the inverse PROVIA matrix and the X-M1 PROVIA matrix, in either order, does not reproduce the X-M1's own Velvia, ASTIA or PRO Neg matrices: maximum element error is 0.094 to 0.224, worse than reusing PROVIA's matrix for Velvia and ASTIA. Matrix transfer therefore needs a colour-space method. | X-M1 1.50 fallback image at DAT `0xe025c` |
| The X-M1 develop stages have the X70's stage structure with byte-read data of their own (E-CC-COLOUR-MODEL-150). The chroma reader `FUN_00d60cc4` reads each entry's fields in the order +0, +4, +6, +2, the X70's 2x2 element order. The gain table is a keyed record table in image 4 at `0x825630` whose records equal the X70's; at DR100 with adjustments 0 PROVIA selects record 1 and Velvia, ASTIA and PRO Neg select record 101. The hue-range selector `0xd62040` (codes 0-7: 1, 1, 1, 1, 2, 1, 3, 3) names luma-band presets whose values equal the X70's presets 1-3. The five chroma gains multiply to unity at DR100 for all five films, and PROVIA's film chroma gain is unity at every DR, as Classic Chrome's is on the X70. The body's adjustment bytes in every span these stages read equal the fallback image. Open: Highlight and Shadow setting 0 as record index 2, and DR100 as `level2` 1, are inference. | X-M1 1.50 images 4 and 5, fallback image and body captures; x70view `research/film-simulation.md` |

On these readings, Classic Chrome on this processor family is one tone curve, one 3x3 colour matrix and one 2x2 chroma matrix over PROVIA's luma-band, gain-table and chroma-gain data. The X-M1 has each of those stages with its own byte-read data, so Classic Chrome on the X-M1 adds the three data items and reuses PROVIA's selectors. Whether the X-M1 applies them with the X70's arithmetic is what the X-M1 model check in 19.5.6 tests.

#### 19.5.2 What the X70 sibling tools can and cannot show

The X70 full-system emulator's still ISP is a monochrome fixture that keeps uploaded calibration without applying it (x70view `emulator/REGISTER_IMAGING.md`). RAW conversion run in the emulator therefore produces no film colour. Its metadata identity check probably refuses an X-M1 RAF (static, unmeasured). The emulator and its DriverHarness are used for register words instead: the stock X70 builders give Classic Chrome's and PROVIA's parameter-bank words at default settings. The existing `stock-film-registers.json` capture used Color +2 and DR400.

The pixel reference is x70view's host model of the development pass (`emulator/film_stage.py`). It is bit-exact on 38,552 flat camera patches across nine X70 calibration sessions. Its demosaic is a stand-in, and Classic Chrome is register-checked there without a real-photo pixel check. The project's forward model reimplements the documented arithmetic in Haskell or Rust and uses `film_stage.py` only as an external oracle. No x70view code is copied.

#### 19.5.3 Inputs and rights

The X-T1 5.52 DAT becomes a pinned input beside the X-M1 1.50 DAT in `manifests/`. X70 values come from x70view at a pinned revision. Fujifilm tables and values extracted from either camera stay under `private-data/` and are extracted at build time from inputs the owner supplies. Tools, addresses, layouts and the method are published under [the asset boundaries](../docs/principles.md#publication-and-asset-boundaries).

#### 19.5.4 Derivation

- Tone: the X-Trans II Classic Chrome record (X70 key 38, which the X-T1 5.52 code image carries unchanged, E-CC-XT1-TONE-552) is used as is.
- Matrix and chroma: fitted in colour space by the relative method. The target is the X-M1's own PROVIA rendering plus the X-Trans II Classic Chrome difference from X-Trans II PROVIA, over a patch set rendered through the X-M1 and X-Trans II forward models; chroma is ridged toward the PROVIA-relative 2x2 construction. Method, residuals and the candidate parameter file are in [the colour model report](../reports/classic-chrome-model.md) (E-CC-COLOUR-MODEL-150).
- Method error: leave one out over Velvia, ASTIA, PRO Neg Std and PRO Neg Hi. Each film is fitted from its X-Trans II parameters and the X-M1's PROVIA and compared with its rendering under the X-M1's real parameters. The result is `e_method`.
- Luma band, gain table and chroma gains: PROVIA's stock selectors, whose X-M1 data is byte-read (19.5.1).

#### 19.5.5 Mechanism

The design and its host verification are in [the patch report](../reports/classic-chrome-patch.md). The approved design:

- Stored code `0xd` in col0 and col2, a conversion-table row, an availability-matrix row in PRO Neg's seven mode columns, the shooting-menu values array and the RAW-conversion list `0x0078bd5c`.
- Develop dispatch stays stock, so `0xd` reaches the stages as develop code 0. A 4-word rewrite of the `+0x110` setter marks the record with `+0x110 = 0xd`; develop code 8 is not used, because readers outside the develop builder index past their tables with it.
- Hooks at the colour-matrix, chroma and tone-record reads serve patch-owned data for a marked record. The tone record is served by the hook, not added to the tone table as key 37. Every other selector is PROVIA's.
- Body adjustment data is never written. The data block lives in the image-4 decode-prefix window.
- Metadata `FUN_00eab258` emits FilmMode `0x600`; Saturation follows PROVIA.
- The label is the stock special id `0x83e`.
- The Q cell and the FILM 1-3 bracket lists stay stock. Live view previews and movie records as PROVIA.
- D3 as designed ([recipe-patch-d3](../reports/recipe-patch-d3.md)) is incompatible: Classic Chrome owns conversion row 2, code `0xd` and 484 of the window's 512 bytes, and D3 must be re-planned around them.
- Delivery is a flash container built on RC0 through the extension tooling; the RAM variant waits for issues 77 and 78. The container refuses any firmware other than 1.50.

#### 19.5.6 Verification

| Level | Check | Class |
| --- | --- | --- |
| Host oracle | The forward model reproduces `film_stage.py` outputs for X70 parameters, and the leave-one-out run gives `e_method` | Static, model |
| X-M1 model check | Owner shoots RAW+JPEG of a ColorChecker Classic, a grey ramp and natural scenes at ISO 200, DR100, WB Daylight with all image adjustments 0, then uses in-camera RAW conversion of each RAF to PROVIA, Velvia and ASTIA; PRO Neg frames come from the shooting menu. RAW-converted PROVIA must equal the shot PROVIA JPEG. Driven from the PROVIA output, the forward model predicts the X-M1's Velvia, ASTIA and PRO Neg output within the residual `e_model` | Hardware-observed images, ordinary photography with no USB or firmware change |
| Native and emulator | Site cases and develop-builder cases on stock and patched images: the marker reaches the patch data and stock codes give byte-identical parameter blocks; `//emulation:flash-boot-check` and the owner's visual check in the viewer | Emulated |
| Camera | The flash container ([camera-run procedure](07-governance-and-handoff.md#3-camera-runs)), then the live acceptance in [patch report 11.4](../reports/classic-chrome-patch.md#114-acceptance-after-install-spec-194-and-1956), including selection, capture and persistence across a power cycle with Classic Chrome stored in the shooting setting and in C | Hardware-observed |
| Secondary | The GFX ETERNA 55 3D-LUT pack v1.10, the only official pack with a Classic Chrome LUT, is a direction check after its F-Log2 path is calibrated against the X-M1's own PROVIA, Velvia, ASTIA and PRO Neg Std output. Review JPEGs are not used | Reported reference |

Thresholds are fixed from `e_method` and `e_model` before the candidate is accepted: [model report section 7](../reports/classic-chrome-model.md#7-acceptance-thresholds-for-spec-1956-items-1-3) gives the bounds, [section 8](../reports/classic-chrome-model.md#8-owner-photo-session) the owner photo session that measures `e_model`, and [section 6.6](../reports/classic-chrome-model.md#66-expected-response-to-image-adjustments-spec-1956-item-6) the expected per-setting changes for item 6. Acceptance:

1. Look: per-patch CIEDE2000 between the X-M1 Classic Chrome output and the relative target for the same frame (the X-M1 PROVIA JPEG's patch L*a*b* plus the modelled X-Trans II Classic Chrome minus X-Trans II PROVIA difference, model report section 7) stays within the fixed bound, reported per hue sector and along the grey ramp.
2. Discrimination: the output is closer to that relative target than any existing X-M1 mode is.
3. Neutrals stay neutral, and the ramp's L* curve is within the bound.
4. Stock modes: RAW conversion of the fixed RAFs gives byte-identical JPEGs before and after the patch. The owner photo session runs on the RC0 body before the install, so its in-camera RAW conversions (model report section 8.1 step 5) are the before set.
5. exiftool reads FilmMode `Classic Chrome`, and the recorded settings match.
6. Color ±2, Highlight and Shadow ±2, and DR200/400 move the output as the reference does.
7. The 19.4 items: selectable, survives a power cycle, the owner's visual check, no other regression.

Delivery follows 19.4: emulator, the owner's visual check, then a flash after `//emulation:flash-boot-check` and the owner's go naming the digest.
