# Classic Chrome host colour model and fit

Host-only work for [spec/10 section 19.5](../spec/10-film-recipes.md#195-classic-chrome-rc2) and [CLASSIC-CHROME-01](https://github.com/skeet70/latent-image/issues/79). Evidence class: static inference over a host model and byte-read 1.50 develop-stage data (E-CC-COLOUR-MODEL-150). The model reproduces x70view's X70 film-stage model bit for bit; the X-M1 parameter sets take every develop-stage value from the pinned 1.50 images, and the hardware arithmetic they feed is the X70's, which the owner photo session in section 8 tests. Every Classic Chrome result here is an approximation of Classic Chrome as shipped on EXR Processor II with X-Trans CMOS II, derived from sibling bodies, and is not X-M1 ground truth. Fujifilm values (matrices, chroma entries, tone records, gain records, fitted candidate) stay under `private-data/analysis/classic-chrome/`; this report gives addresses, methods, residuals and thresholds.

## 1. Tools

| Target | Role |
| --- | --- |
| `//host:film-model` | `FujiAF.FilmStage` (integer film stage), `FujiAF.FilmInvert` (its floating-point inverse), `FujiAF.Colour` (CIELAB, CIEDE2000, sensor models, patch sets and L*a*b* grids), `FujiAF.FilmFit` (integer matrix and chroma fit), `FujiAF.FilmGrid` (oracle grid), `FujiAF.XM1Develop` (1.50 develop-stage readers, adjustment field encoding), `FujiAF.ClassicChromeCandidate` (the v1 parameter file) |
| `//host/firmware:classic-chrome-model` | `tables`, `oracle-input`, `oracle-compare`, `bundle`, `method`, `fit-cc`, `photo-identical`, `chart-patches`, `e-model`, `settings`, `look-reference` (usage with no arguments) |
| `//host/firmware:film-stage-tests` | oracle fixtures, the 34 published CIEDE2000 pairs (Sharma, Wu and Dalal 2005), the parameter boundary, fit recovery of a reachable target, inverse round trip |
| `//host/firmware:candidate-tests` | the v1 parameter file on a synthetic candidate: field encoding and chroma storage order against hand-worked values, read-back, refusals |
| `//host/firmware:photo-session-tests` | the photo-session commands through the command line on flat JPEGs written by the pinned ffmpeg |
| `//host/firmware:look-reference-tests` | F-Log2 against the data sheet's code values, the F-Gamut and BT.709 matrices, `.cube` parsing and trilinear interpolation, and the relative statistics on pairs with a known chroma, lightness and hue change |

Private reproduction, from the worktree root with `P=private-data/analysis/classic-chrome`, `R=private-data/analysis/x-m1-1.50` and `X` the x70view checkout (revision `a47591d9bd`). `$R/payload.bin` is the length-correct complemented payload of the pinned DAT (bytes from 532 on, XOR 0xff, SHA-256 `8c6f806e…26c0`, [firmware extraction](firmware-extraction.md)); the two decodes come from `//lz:fflzd`:

```sh
classic-chrome-model tables bazel-bin/lz/fflzd private-data/firmware/x-m1-1.50/FWUP0002.DAT \
  private-data/firmware/x-t1-5.52/FWUP0005.DAT $X/firmware/mapped/section4.bin \
  $X/research/film-simulation/recipes-srgb.json $P/tables.json
bazel-bin/lz/fflzd $R/payload.bin 0x160048 $R/lz/id4-0061c400.bin
bazel-bin/lz/fflzd $R/payload.bin 0x2c0048 $R/lz/id5-00bf1a00.bin
classic-chrome-model bundle $P/tables.json $X/research/film-simulation/recipes-srgb.json \
  $X/research/hardware/ff80-film-calibration-20261013T014935Z-4e40b4/session.json \
  $X/research/film-simulation/stock-film-registers.json $X/firmware/mapped/section4.bin \
  $R/lz/id4-0061c400.bin $R/lz/id5-00bf1a00.bin $R/payload.bin $P/bundle.json
classic-chrome-model method $P/bundle.json $P/method.json
classic-chrome-model fit-cc $P/bundle.json $P/method.json $P/candidate.json $P/fit-report.json
```

### 1.1 Sibling tables

`tables` checks the SHA-256 of the X-M1 1.50 DAT, the X-T1 5.52 DAT ([firmware manifest](../manifests/firmware.json)), X70 1.10 section 4 (`9923d542…5acb`) and x70view's `recipes-srgb.json` (`d1fc3551…4191`, the X70 matrix and chroma values from its cfgdata capture). It decodes X-M1 images 4 and 5 and the two X-T1 code streams with `//lz:fflzd`, checks each decode's SHA-256 (the X-T1 streams against x70view `firmware/xt1/5.52/firmware.json`) and writes `tables.json`. Its comparisons (static inference, byte-read):

| Finding | Ledger |
| --- | --- |
| X-M1 image 4 holds 37 tone records (count at `0x824bc4`, records at `0x824bc8`, keys 0-36), each equal byte for byte, key included, to the X70 section-4 record of the same index. The X70 table (count 42 at `0xb624e0`) appends keys 37-41; none is in the X-M1 table. | E-CC-TONE-LINEAGE-150 |
| The X-T1 5.52 DAT carries a complemented fallback adjustment image at payload `0xe0048` (`X-T1` at `+0x15c`, `0xaa55` at `+0x202`). Its matrices 1-5 and 9 and chroma entries 1-5 equal the X70's. In code stream `0x0038025c` (base `0x00eff120`) the chroma reader `0x0108b538` reads the offset table `0x01465d10` in the order `+0, +4, +6, +2`, as X-M1 `FUN_00d60cc4` does; that table has 15 entries, 1-10 at `0xb077 + 8 (id - 1)` and 11-15 at `0xce9a + 8 (id - 11)`. For film index 8 in sRGB the chroma selector table `0x014660d4` (film + 10 x colour space) names adjustment byte `0xcec2`, which holds 11, so Classic Chrome's chroma is entry 11 at `0xce9a`; the matrix selector table `0x014685a0` names `0xce94`, which holds 9. Entry 11 equals the X70's Classic Chrome chroma. | E-CC-XT1-FALLBACK-552 |
| X-T1 code stream `0x0016025c` (base `0x00780400`) holds a 42-record tone table (count at `0x00af45e0`, records at `0x00af45e8`) equal byte for byte to the X70's, so its key-38 record is the X70's Classic Chrome record. `0x00869198` searches it by key. | E-CC-XT1-TONE-552 |
| Composing the X-Trans II film matrix with the inverse X-Trans II PROVIA matrix and the X-M1 PROVIA matrix, in either order, misses the X-M1's own Velvia, ASTIA, PRO Neg Std and PRO Neg Hi matrices by a maximum element error of 0.094 to 0.224, worse than reusing the X-M1 PROVIA matrix for Velvia and ASTIA. | E-CC-MATRIX-TRANSFER-150 |

## 2. Forward model

The film stage takes white-balanced 14-bit camera RGB `w` to 8-bit Y, Cb, Cr, with the arithmetic x70view documents for the X70 RAW conversion (`research/film-simulation/raw-clone.md`, `matrix.md`):

1. Gain table: 22 Q9 points, a cubic Bezier of seven segments (widths 64, 64, 128, 256, 512, 1024, 2048) over `b = (3 xR + 7 xG + 6 xB) >> 4` of `x = min(w >> 2, 4095)`; `x = min((w g) >> 11, 4095)`.
2. Matrix: Q10, 13-bit fields, each product floored to 3 fractional bits, `(sum + 4) >> 3`, clipped to 12 bits.
3. Curve A: the 0x48-byte tone record, nine segments of `16 << e` inputs, Bezier with 9-bit `t`; `>> 6` to 8-bit RGB.
4. BT.601 luma with per-term truncation; `R - Y`, `B - Y`.
5. Chroma 2x2 (Q10, each term `>> 8`), scaled by 578 or 730, clipped to signed 10 bits, then the luma-band factor `gain + (shift Y >> 8)` of the band containing Y (a luma on a bound takes the upper band).

`bundle` builds parameter sets per body and film at DR100 with Color, Highlight and Shadow 0. Both bodies use tone keys PROVIA 1, Velvia 7, ASTIA 12, PRO Neg Std 17, PRO Neg Hi 22 and matrix and chroma ids 1-5.

- X-Trans II: X-T1 matrices and chroma (Classic Chrome: matrix 9 and the X-T1 entry at `0xce9a`), X70 tone records (Classic Chrome key 38), X70 luma-band presets from x70view `recipes-srgb.json`, and X70 gain records from section 4 (section 2.2).
- X-M1: the X-M1's matrices and chroma, its tone records, and the gain record, luma-band preset and chroma gains its develop stages select (section 2.1). `bundle` refuses to write unless each X-M1 matrix and chroma value re-encodes to the raw fields the stage reads from the fallback image and the chroma storage order decodes to the model's element order.

### 2.1 X-M1 develop-stage data (byte-read, 1.50 images and fallback adjustment image)

The develop builder `FUN_00d5d294` (image 5) writes the stage blocks. Its gain-table stage is `FUN_00d6a3f0` over `FUN_00d6a250`; the luma bands are `FUN_00d618a4`; the chroma 2x2 is `FUN_00d60ea8`. Adjustment bytes are read through `FUN_0069f8fc` (byte) and `FUN_0073ed6c` (little-endian halfword) at offsets that image-5 tables list.

| Stage | Byte-read structure | DR100, adjustments 0 |
| --- | --- | --- |
| Gain table | `FUN_00d69df0`: family by develop code from `0xd6a708` (row `code + 8 * mode`), H/S family from `0xd6acec[5 * H + 25 * (family - 1) + S]`, record key from `0xd6a920[6 * (hsfamily - 1) + level2]` (`FUN_00d69c84`, valid families 1-80). `FUN_00690b74` (image 4) copies 22 halfwords of the record with that key from the table at `0x825630` (count 226, records of 0x30 bytes from `0x825634`), record 0 when absent. `FUN_00d6a250` scales each point by record `+0x1a6` percent, which the still builder sets to 100 (`0x00d79620`). Overrides `adj[0xb5]` and `adj[0xcb74]` are 0. | PROVIA (code 0): family 1, key 1. Velvia, ASTIA, PRO Neg Std and Hi (codes 4-7): family 2, H/S family 26, key 101, a flat table at unity gain. Key 1 equals the X70's logged PROVIA DR100 table point for point. |
| Luma bands | `FUN_00d618a4`: preset id from `0xd62040[code + 8 * row]` (1-8 valid, otherwise five fixed fallback bands; code 2 zeroes the block), four bound bytes through `0xd61d74 + 8 * (id - 1)`, ten halfwords through `0xd61cd4 + 20 * (id - 1)`: five shifts, sign-filled when any of bits 10-15 is set, then five gains. | Row 0 selects 1, 1, 1, 1, 2, 1, 3, 3 for codes 0-7. Presets 1-3 equal the X70's presets 1-3 value for value. |
| Chroma gains | `FUN_00d60ea8` multiplies each 2x2 field by five gain bytes and shifts right by 34: Color `0xd62194[+0x10c]` (five values falling from index 0 to 4, unity at index 2), `0xd61f34[10 * +0x160 + +0xf4]`, film `0xd620b0[5 * (sel - 1) + +0x154]` with `sel = 0xd61e04[code]` (PROVIA codes sel 1: unity at every DR index; codes 4-7 sel 2: unity at DR index 0, rising with it), `0xd61e64[+0x13c]` and `0xd62134[FUN_0073f338]`. | Product 2^34, unity, for all five films. |
| Chroma 2x2 | `FUN_00d60cc4` reads the four halfwords whose offsets `0xd61e14 + 8 * (id - 1)` lists (`0xb077`, `+2`, `+4`, `+6` for id 1) in the order `+0, +4, +6, +2`, as (Cr from R-Y, Cr from B-Y, Cb from R-Y, Cb from B-Y). | Raw fields are 13-bit two's complement (bits 13-15 clear). |
| Colour matrix | `FUN_00d670a0` reads nine halfwords listed at `0xd67f88 + 18 * (id - 1)` (`0xacc3` onward for id 1). | Same field encoding. |

Record defaults behind the DR100 column: the record initialiser `FUN_00d791b0` stores Highlight and Shadow indices `+0xfc = +0x100 = 2` of 0-4, Color index `+0x10c = 2` and `+0xf4 = 2`; `mode` is 0 for the still job ([film-simulation report](film-simulation.md#the-develop-records-mode-and-level2-fields-bound)). Two links are inference rather than byte-read: index 2 is the setting 0 (it is the middle of five and the initialiser's value), and DR100 gives `level2 = 1` and DR index `+0x154 = 0` (the film-simulation report leaves `level2`'s source unbound). The gain record this selects for PROVIA equals the X70's logged PROVIA DR100 table, and the tone keys at `level2 = 1` are the X70's DR100 keys, which agree with both readings.

Body bytes (hardware-observed captures, host comparison): the read plan `read-adjustment-42000-v1` (`private-data/read-plans/adjustment-42000.json`, SHA-256 `e12aa824…10c9`) reads the whole adjustment area `0x000000-0x041fff` in 256-byte windows, and its window 1 carries `X-M1` at `+0x5c`, the fallback image's `0x15c`, so window offsets are fallback-image offsets. Every adjustment byte the model's stages read was compared with the fallback image in the four full captures of that plan in the private research archive (`private-data/sessions/20261008T175358Z-read-config`, `20261009T163324Z`, `20261009T233828Z`, `20261018T171652Z`, all `-read-config`). The spans compared are 1,797 bytes: the gain overrides `0xb5` and `0xcb74`; the colour matrix fields `0xacc3-0xad1c`; the chroma fields `0xb077-0xb09e`; the chroma gains and luma-band presets `0xb0db-0xb384`; the luma-band selectors `0xb3fd-0xb401`; the gain families, record keys, tone curve ids and gain family-by-code `0xc7de-0xcb94`; and the tone families `0xccdf-0xccfa`. All 1,797 bytes are captured and equal in all four captures; none is uncaptured and none differs. The 233 bytes where the 2026-10-08 capture differs from the fallback image lie in windows `0x0000-0x0300`, `0x1200`, `0x1a00-0x1d00`, `0x2700`, `0x2800`, `0x2a00`, `0x4000` and `0x9700`, outside every span the model reads. The tone keys the families and curve ids give at `level2 = 1` (1, 7, 12, 17, 22 for codes 0, 4, 5, 6, 7, both A and B tables) are the ones the bundle uses.

### 2.2 X-Trans II gain records

X70 1.10 section 4 holds the same record table at `0x00b5f89c` (236 records; records 0-225 equal the X-M1's byte for byte). Its record 1 equals the logged PROVIA DR100 table, which `bundle` checks. In x70view's live-view capture `stock-film-registers.json` (DR400 with soft Highlight and Shadow), PROVIA and Classic Chrome use record 45 and Velvia, ASTIA, PRO Neg Std and Hi record 145. That is the same +100 family offset the X-M1's selector gives codes 4-7. `bundle` checks the offset and uses record 101 (flat, unity gain) for those four films on the X-Trans II side at DR100 and record 1 for PROVIA and Classic Chrome. The X70's DR100 selection for the four films is inferred from that offset; it is not logged.

## 3. Oracle check

`develop()` of x70view `emulator/film_stage.py` (revision `a47591d9bd`, run in place in x70view's environment) and the Haskell model agree on every point of a 6848-point grid (1728 lattice points over 12 levels per channel, 4096 uniform and 1024 log-spaced points):

| Parameter sets | Points equal |
| --- | --- |
| Three synthetic sets, public fixture (`fixtures/film-stage/PROVENANCE.md`): identity, a realistic invented film, and a field-range stress set with early-ending segments, equal band bounds and clipped chroma | 6848 / 6848 each |
| The six X70 films at DR100 (PROVIA, Velvia, ASTIA, PRO Neg Std, PRO Neg Hi, Classic Chrome), private | 6848 / 6848 each |
| The five X-M1 sets with their selected gain records, private | 6848 / 6848 each |

The fixture test runs without x70view; the oracle command is recorded in the fixture provenance.

## 4. Sensor stage and patches

Scene colours are CIE XYZ under D65. Camera raw RGB is the LibRaw 0.22.1 `adobe_coeff` XYZ(D65)-to-camera matrix (`src/tables/colordata.cpp`): the `X-M1` entry (shared with X-Pro1 and X-E1) for X-Trans CMOS, the `X70` entry for X-Trans CMOS II, and the `X-T1` entry (shared with X-E2) as the sensor sensitivity case. White balance divides each channel by the camera's response to the D65 white. One exposure scale, fixed so that an 18 % grey renders at luma 118 (sRGB L* 50) through X-Trans II PROVIA, maps balanced RGB to the 14-bit stage input for both bodies. Output Y, Cb, Cr is read as JFIF sRGB and compared in CIELAB relative to D65 with CIEDE2000.

| Set | Patches | Source |
| --- | ---: | --- |
| ColorChecker Classic | 24 | X-Rite L*a*b* (D50, 2 degree) for charts made after November 2014, as published in colour-science's `CCS_COLORCHECKER24_AFTER_NOV2014`, Bradford-adapted to D65 |
| Neutral ramp | 25 | D65 greys, Y from 0.005 to 1.0 in equal log steps |
| Gamut set | 642 | CIELAB (D65) grid, L* 20-90 by 10, hue by 20 degrees, C* 15-105 by 15, kept where linear Rec.2020 RGB lies in [0, 1] |

Scenarios: `xm1-vs-x70` (primary), `xm1-vs-xt1` (X-T1 sensor matrix for X-Trans II), `xm1-vs-x70-grey-matched` (each film's selected gain record scaled so an 18 % grey renders at PROVIA's luma).

## 5. Method

The fit chooses an X-M1 matrix (Q10, rows summing to 1024, 13-bit fields) and chroma 2x2 (Q10) minimising the mean squared CIEDE2000 over all 691 patches. The optimiser runs Levenberg-Marquardt on the L*a*b* differences, then rounds of Nelder-Mead on rounded coordinates and integer coordinate search, from three starts (the X-Trans II film's values, the X-M1's PROVIA values, and the identity matrix with the constructed chroma), keeping the best.

Targets:

- Absolute: the X-M1 stage renders each patch as the X-Trans II stage renders the film. Kept as a comparison.
- Relative (primary): the X-M1's own PROVIA rendering plus the X-Trans II film's L*a*b* difference from X-Trans II PROVIA, per patch.

The free relative fit trades matrix against chroma along a flat valley. The variants that tie chroma to a construction from the stage algebra `(Cr, Cb) = C (R - Y, B - Y)`:

- Constructed chroma, input side: `C_film,X-Trans II · C_PROVIA,X-Trans II^-1 · C_PROVIA,X-M1`; output side: `C_PROVIA,X-M1 · C_PROVIA,X-Trans II^-1 · C_film,X-Trans II`. The two differ by at most 10 Q10 for every film because both PROVIA 2x2 matrices are close to diagonal.
- Fixed chroma: the construction, with only the six matrix integers fitted.
- Ridge: all ten integers, each chroma term pulled toward the construction with residual `sqrt(w) (c - c_constructed) / 100`, w in {0.01, 0.1, 1, 10}.

`e_method` is leave-one-out over Velvia, ASTIA, PRO Neg Std and PRO Neg Hi: the film is fitted from its X-Trans II parameters and the X-M1's PROVIA, then rendered through the X-M1 stage and compared with the X-M1 stage under the X-M1's real parameters for that film. The selection takes the lowest mean over the four films of the per-film mean `e_method` in the primary scenario, among the fixed and ridge variants.

## 6. Results

### 6.1 Variant selection (primary scenario, all 691 patches)

| Variant | Mean of film means | Mean of film p95 | Max |
| --- | ---: | ---: | ---: |
| relative, ridge output side, w 0.1 (selected) | 1.24 | 3.11 | 10.38 |
| relative, ridge output side, w 0.01 | 1.25 | 3.19 | 10.38 |
| relative, ridge input side, w 0.1 | 1.25 | 3.14 | 9.74 |
| relative, ridge input side, w 0.01 | 1.26 | 3.15 | 10.38 |
| relative, ridge input side, w 1 | 1.26 | 3.12 | 9.74 |
| relative, ridge, w 10 | 1.28-1.30 | 3.15-3.16 | 9.74 |
| relative, ridge output side, w 1 | 1.29 | 3.20 | 9.74 |
| relative, fixed chroma | 1.32 | 3.18-3.22 | 9.74-10.38 |
| relative, free (comparison) | 1.22 | 3.09 | 10.38 |
| absolute (comparison) | 1.45 | 3.61 | 11.26 |

The constructed chroma alone (fixed variants) is worse than letting chroma move a little: the construction differs from the X-M1's own chroma by up to about 275 Q10 on the leave-one-out films. The ridge variants with weights 0.01 to 1 lie within 0.05 of each other, less than the run-to-run spread of `e_method` in 6.3 (up to 0.15), so the choice among them, and the order of input and output side, is not significant. In the grey-matched scenario they lie at 1.24 to 1.27.

### 6.2 e_method per film (selected variant, primary scenario)

Mean / p95 / max CIEDE2000 against the X-M1 stage with its real parameters.

| Film | Relative, all 691 | Relative, ColorChecker 24 | Absolute, all | Reuse X-M1 PROVIA, all | Copy X-Trans II, all |
| --- | --- | --- | --- | --- | --- |
| Velvia | 1.07 / 2.39 / 4.78 | 0.79 / 1.96 / 2.31 | 1.90 / 5.38 / 11.26 | 3.96 / 7.47 / 12.44 | 2.21 / 5.61 / 11.93 |
| ASTIA | 1.47 / 3.53 / 5.11 | 1.31 / 2.99 / 3.33 | 1.47 / 3.71 / 7.80 | 2.54 / 5.70 / 13.38 | 1.80 / 4.23 / 7.83 |
| PRO Neg Std | 1.14 / 2.72 / 4.89 | 0.92 / 1.67 / 1.99 | 1.18 / 2.67 / 5.82 | 1.69 / 4.94 / 9.35 | 1.33 / 2.87 / 4.75 |
| PRO Neg Hi | 1.28 / 3.83 / 10.38 | 0.98 / 2.96 / 3.76 | 1.26 / 2.67 / 5.34 | 2.00 / 4.67 / 7.72 | 1.40 / 2.86 / 5.18 |

The neutral ramp contributes 0 to `e_method` in every variant: matrices with rows summing to 1024 and a 2x2 on colour differences leave neutrals neutral, and both renderings share the tone record and gain record. The ramp threshold therefore comes from `e_model` alone.

Sensitivity, selected variant, all patches, mean range over the four films: X-T1 sensor 1.30-1.53 (absolute 1.73-2.44); grey-matched gain records 1.04-1.52 (absolute 1.18-1.97). The absolute fit's `e_method` equals, to within 0.03, the distance between the X-M1's real rendering and the X-Trans II rendering: Fujifilm's X-M1 films are retuned, not sensor-compensated copies of the X-Trans II look, and the relative target captures part of that retuning through the X-M1's own PROVIA. With the byte-read gain records the relative method is clearly better than the absolute one for Velvia (1.07 against 1.90) and ties it for ASTIA and both PRO Neg films on the mean, with a higher maximum for PRO Neg Hi (10.38, one gamut patch).

### 6.3 Stability

Runs from the three starts, primary scenario, per film:

| Variant | Largest Q10 difference | Largest mean CIEDE2000 between run renderings | Largest single patch | e_method mean spread |
| --- | ---: | ---: | ---: | ---: |
| ridge output w 0.1 (selected) | 48-81 | 0.47-0.58 | 2.3-2.8 | up to 0.15 |
| fixed chroma | 9-33 | 0.11-0.44 | 1.5-2.7 | up to 0.16 |
| ridge w 10 | 11-44 | 0.18-0.47 | 1.6-2.9 | up to 0.14 |
| ridge w 0.01 | 45-122 | 0.44-0.64 | 2.2-3.1 | up to 0.12 |

Tying chroma removes the largest matrix/chroma excursions but not a residual spread of about 0.4 mean CIEDE2000 between runs, which the fixed-chroma variant shows as well: the integer pipeline's 8-bit output makes the objective flat at that scale, so several integer settings fit equally well. For Classic Chrome the selected variant's start spread is 81 Q10 and 0.41 mean (2.17 max patch); its fits under the X70 and X-T1 sensor matrices differ by 79 Q10 and 0.55 mean (3.51 max patch) when rendered on the same inputs, close to the optimiser spread. The absolute fit's Classic Chrome matrix changes by more than 300 Q10 between the two sensor matrices.

### 6.4 Classic Chrome candidate

Selected variant, primary scenario; tone record X70 key 38 unchanged, PROVIA's gain record, luma bands and chroma gains. The setting is in `private-data/analysis/classic-chrome/candidate.json` (section 6.5). Its chroma lies within 34 Q10 of the construction.

| Comparison | All 691 | ColorChecker 24 |
| --- | --- | --- |
| Candidate vs relative target | 0.89 / 2.07 / 3.50 | 0.54 / 1.30 / 1.42 |
| Candidate vs X-Trans II reference | 1.75 / 4.73 / 8.72 | 1.40 / 2.86 / 4.35 |
| Absolute fit vs X-Trans II reference (comparison) | 0.07 / 0.59 / 1.75 | 0.15 / 0.80 / 1.14 |
| Absolute fit vs relative target (comparison) | 1.88 / 4.97 / 9.64 | 1.49 / 3.51 / 3.58 |

Residual against the relative target by 30-degree CIELAB hue sector: sector means 0.68-1.17, sector maxima 1.94-3.50; patches with target C* below 5 have mean 0.17. The ramp's L* equals the target's at every step and its C* is 0. Under the X-T1 sensor matrix the residual is 0.90 / 2.02 / 3.69, and with grey-matched gain records 0.88 / 2.03 / 3.50.

Discrimination, mean CIEDE2000 to the relative target over the ColorChecker: candidate 0.54; PRO Neg Std 3.20, PRO Neg Hi 3.56, PROVIA 3.65, ASTIA 3.94, Velvia 5.95. PRO Neg Std is the nearest existing mode in every scenario (3.16 with the X-T1 sensor, 3.19 grey-matched).

### 6.5 Parameter file for the patch build

`fit-cc` writes `candidate.json` in the format `latent-image.classic-chrome-parameters.v1` that [the patch design](classic-chrome-patch.md) defines in its section 7, and the residual report to `fit-report.json`. It has the target (X-M1, 1.50, the pinned DAT digest) and `approximation_of`. Provenance carries the variant name, the bundle and method files with their SHA-256, `e_method` and `e_model` as integers in hundredths of mean CIEDE2000 (`e_model` null until the session). The colour matrix is nine raw u16 fields in the adjustment encoding. Chroma is four raw u16 fields in storage order `+0, +2, +4, +6`, that is (Cr from R-Y, Cb from B-Y, Cr from B-Y, Cb from R-Y). The tone record is X70 key 38's nine head bytes and 28 points, and `reuse` is PROVIA for luma band, chroma gain, H/S tone and tone scalar. `fit-cc` reads the file back with the strict v1 parser and refuses to write it unless the read-back equals the fitted setting. `bundle` has already checked the encoding and storage order against the X-M1's own fields (section 2).

### 6.6 Expected response to image adjustments (spec 19.5.6 item 6)

`classic-chrome-model settings BUNDLE.json CANDIDATE.json XM1-ID4.bin XM1-ID5.bin XM1-PAYLOAD.bin X70VIEW-FRONTEND.md OUT.json` renders the patch set under each adjustment of item 6, one at a time, and writes `private-data/analysis/classic-chrome/settings.json`. The X-M1 selectors come from the 1.50 images and the fallback image (section 2.1); the body's bytes in their spans equal the fallback image. The X-Trans II Color factors are read from x70view `research/film-simulation/frontend.md` at revision `a47591d9bd` (SHA-256 `f695de63…11b7a`), which the tool checks.

| Path | X-M1 selector (byte-read) | DR100, adjustments 0 | Varies with |
| --- | --- | --- | --- |
| Color chroma gain | `0xd62194[+0x10c]` | unity | Color: falls from index 0 to 4 |
| Film chroma gain | `0xd620b0[5 (sel - 1) + +0x154]`, `sel = 0xd61e04[code]` | PROVIA sel 1: unity | PROVIA: unity at every DR index |
| Gain-table record | `0xd6a920[6 (0xd6acec[5 H + 25 (family - 1) + S] - 1) + level2]` | PROVIA: record 1 | Highlight indices 0-4: records 81, 61, 1, 21, 41; Shadow indices 0-4: 17, 13, 1, 5, 9; `level2` 1, 2, 3: records 1, 2, 3 |
| Tone record | `0xd685f4[6 (family - 1) + level2]`, family from `0xd684a0` and `0xd683b0` | PROVIA: key 1 | PROVIA: key 1 at every `level2` |

The setting-to-index mappings are inference: index = 2 - setting for Color, Highlight and Shadow (the record initialiser stores 2; the Color gains fall from index 0 to 4; Highlight index 0 raises the upper gain points, Shadow index 0 lowers the lower ones), and DR100, DR200 and DR400 are DR index 0, 1, 2 and `level2` 1, 2, 3 (`FUN_0073fc64`). Records 2 and 3 roughly double and quadruple record 1's gain below mid-grey, so DR200 and DR400 are modelled as one and two stops less sensor exposure with those records.

In the patch design ([classic-chrome-patch.md](classic-chrome-patch.md) sections 4.5 and 5) a Classic Chrome capture carries develop code 0, so every selector above is PROVIA's; the hooks replace only the matrix, the chroma fields before the gains are applied, and the tone record at every curve id. Classic Chrome's tone record therefore does not change with DR, as PROVIA's key 1 does not.

X-Trans II reference, labelled by source: x70view `recipes-srgb.json` gives Classic Chrome PROVIA's film chroma gain (unity at every DR) and key 38 at every DR slot, as PROVIA has key 1; x70view `frontend.md` gives one Color factor table for all films, value-equal to the X-M1's. The X70's still-path gain-record selection by Highlight, Shadow and DR was not read. Its live-view capture (`stock-film-registers.json`, Highlight -2 and Shadow -1) shows PROVIA and Classic Chrome on record 45 and Velvia, ASTIA and PRO Neg on record 145, which is what the X-M1 selector gives codes 0 and 4-7 for those settings at `level2` 1. The reference column below therefore uses the X-M1-selected record numbers on the X-Trans II Classic Chrome stage; the record tables are identical.

Expected change against the same pipeline at the default adjustments: mean dL* over neutral-ramp patches with default L* below 35, 35 to 70 and above 70, then mean dC*ab over the 18 chromatic ColorChecker patches. Primary scenario (X-M1 and X70 sensors).

| Setting | X-M1 gain record | X-M1 Classic Chrome: dL* shadow / mid / highlight, dC* | X-M1 PROVIA | X-Trans II Classic Chrome reference | Same direction |
| --- | ---: | --- | --- | --- | --- |
| Color -2 | 1 | 0.00 / 0.00 / 0.00, -3.76 | 0.00 / 0.00 / 0.00, -4.59 | 0.00 / 0.00 / 0.00, -3.89 | yes |
| Color -1 | 1 | 0.00 / 0.00 / 0.00, -2.00 | 0.00 / 0.00 / 0.00, -2.62 | 0.00 / 0.00 / 0.00, -2.07 | yes |
| Color +1 | 1 | 0.00 / 0.00 / 0.00, +1.74 | 0.00 / 0.00 / 0.00, +1.88 | 0.00 / 0.00 / 0.00, +1.87 | yes |
| Color +2 | 1 | 0.00 / 0.00 / 0.00, +3.57 | 0.00 / 0.00 / 0.00, +4.00 | 0.00 / 0.00 / 0.00, +3.80 | yes |
| Highlight -2 | 41 | 0.00 / -0.75 / -5.26, +0.10 | 0.00 / -0.63 / -5.50, -0.17 | 0.00 / -0.75 / -5.26, +0.01 | yes |
| Highlight -1 | 21 | 0.00 / -0.45 / -2.59, +0.07 | 0.00 / -0.38 / -2.67, -0.19 | 0.00 / -0.45 / -2.59, +0.08 | yes |
| Highlight +1 | 61 | 0.00 / +0.45 / +2.50, +0.01 | 0.00 / +0.38 / +2.65, +0.06 | 0.00 / +0.45 / +2.50, +0.01 | yes |
| Highlight +2 | 81 | 0.00 / +1.12 / +4.85, -0.10 | 0.00 / +0.88 / +5.29, +0.20 | 0.00 / +1.12 / +4.85, +0.02 | yes |
| Shadow -2 | 9 | +4.99 / +0.57 / 0.00, +0.41 | +5.36 / +1.04 / 0.00, +0.45 | +4.99 / +0.57 / 0.00, +0.46 | yes |
| Shadow -1 | 5 | +2.34 / +0.25 / 0.00, +0.18 | +2.50 / +0.62 / 0.00, +0.17 | +2.34 / +0.25 / 0.00, +0.42 | yes |
| Shadow +1 | 13 | -2.03 / -0.41 / 0.00, -0.60 | -2.14 / -0.77 / 0.00, -0.26 | -2.03 / -0.41 / 0.00, -0.48 | yes |
| Shadow +2 | 17 | -3.87 / -0.91 / 0.00, -1.12 | -4.01 / -1.83 / 0.00, -0.81 | -3.87 / -0.91 / 0.00, -1.14 | yes |
| DR200 | 2 | -0.13 / +0.33 / -1.85, +0.15 | -0.23 / +0.41 / -1.78, +0.09 | -0.13 / +0.33 / -1.85, +0.24 | yes |
| DR400 | 3 | -0.17 / +0.26 / -3.14, +0.02 | -0.27 / +0.22 / -3.36, +0.06 | -0.17 / +0.26 / -3.14, +0.33 | yes |

"Same direction" compares the X-M1 Classic Chrome column with the reference component by component: same sign, or both under 0.5. Reusing PROVIA's selectors moves Classic Chrome the same way as the X-Trans II reference for all 14 settings. The ramp columns of the two Classic Chrome pipelines are equal because both use key 38 and the same gain record, and neutrals stay neutral through both matrices; the X-M1 PROVIA column differs in the mid and shadow bands through its own tone curve. The Color gain scales chroma, so Classic Chrome, being less chromatic, changes less than PROVIA (-3.76 against -4.59 at -2) and stays within 0.23 of the reference. Mean CIEDE2000 between the Classic Chrome and PROVIA renderings stays between 4.46 and 4.61 across the 14 settings.

For the item 6 acceptance check, each measured Classic Chrome change against the camera's own default Classic Chrome frame should have the sign of the X-M1 Classic Chrome column wherever that entry is 0.5 or more in magnitude; the magnitudes are model values that `e_model` qualifies.

## 7. Acceptance thresholds for spec 19.5.6 items 1-3

Fixed before the owner session from the selected variant's `e_method` on the ColorChecker (the patch set the session measures), primary scenario, maximum over the four films, rounded up to 0.1: `E_mean = 1.4`, `E_p95 = 3.0`, `E_max = 3.8`. `e_model` comes from the session (section 8) as the maximum over Velvia, ASTIA, PRO Neg Std and PRO Neg Hi of the ColorChecker statistic of the same name. Bounds combine the two in quadrature, `B = sqrt(E^2 + e_model^2)`.

The reference for items 1 and 2 is the relative target for the same frame: the X-M1 PROVIA JPEG's patch L*a*b* plus the modelled X-Trans II Classic Chrome minus X-Trans II PROVIA difference for that patch's published colour. Comparing with the un-anchored X-Trans II rendering would charge the candidate for the X-M1-to-X-Trans II PROVIA difference, which the relative method keeps on purpose.

1. Look: over the 24 ColorChecker patches of the 0 EV chart frame, mean CIEDE2000 to the reference at most `sqrt(1.4^2 + e_model_mean^2)`, p95 at most `sqrt(3.0^2 + e_model_p95^2)`, every patch at most `sqrt(3.8^2 + e_model_max^2)`, and in each 60-degree hue sector holding chromatic patches the sector mean at most the p95 bound.
2. Discrimination: the Classic Chrome output's mean CIEDE2000 to the reference is lower than every existing X-M1 mode's (PROVIA, Velvia, ASTIA, PRO Neg Std, PRO Neg Hi) on the same frame by at least `sqrt(1.4^2 + e_model_mean^2)`. The model's margin to the nearest mode is 2.66.
3. Neutrals and ramp: on the six ColorChecker neutrals at every bracket step, the Classic Chrome output's C*ab exceeds the PROVIA output's by at most `max(1.0, e_model neutral p95)`; along the bracket ramp, each step's L* differs from the reference L* by at most `max(1.0, e_model ramp max)`. The model gives 0 for both, so the floor of 1.0 covers JPEG and noise variation the model does not represent.

With `e_model = 0` the item 1 bounds are 1.4 / 3.0 / 3.8; the candidate's modelled residual on the ColorChecker is 0.54 / 1.30 / 1.42.

## 8. Owner photo session

Ordinary photography on the body as it stands (1.50 with RC0, PRO Neg in the shooting menu). It needs no USB connection and changes no firmware or adjustment data. It can run inside the first Classic Chrome RAM-push session: [patch report 8.4.2a](classic-chrome-patch.md#842a-before-set-on-rc0) shoots the chart set of 8.1 (frames 1 to 3, one natural scene, and the conversions of the 0 EV PROVIA RAF to PROVIA, Velvia and ASTIA) on RC0 before the push, and the rest of 8.1 is optional extra material there. A push leaves the body on RC0 after unload or power-off, so the set serves both as `e_model` input and as the RC0 before-set.

### 8.1 Checklist

Equipment and light:

- ColorChecker Classic (note the chart's edition or purchase date; post-November 2014 matches the published values used here), a tripod, the usual lens at a fixed focal length.
- Daylight: overcast sky or open shade, the chart lit evenly without glare, light at about 45 degrees, no coloured surfaces close by. The chart fills about a third of the frame, square to the camera, in the centre.

Camera settings, checked before the first frame and left unchanged:

- Image quality FINE+RAW, image size L 3:2.
- ISO 200, DYNAMIC RANGE 100 % (not AUTO).
- WHITE BALANCE Daylight, shift R 0 B 0.
- COLOR 0, SHARPNESS 0, HIGHLIGHT TONE 0, SHADOW TONE 0, NOISE REDUCTION 0.
- Film simulation PROVIA/STANDARD to start.
- Exposure mode M; focus on the chart, then keep the camera untouched on the tripod; self-timer 2 s; flash off.

Frames:

1. Chart, PROVIA: set the exposure so the white patch is bright but not clipped (histogram right edge short of the wall), call it 0 EV. Shoot 0 EV, then -2, -1 and +1 EV by shutter speed only. Write down the shutter speeds.
2. Chart, PRO Neg Std from the shooting menu, without moving the camera: the same four exposures (JPEG with its RAF).
3. Chart, PRO Neg Hi from the shooting menu: the same four exposures.
4. Natural scenes, three to five (foliage with sky, a skin tone, strongly coloured objects, a mostly neutral scene), each on the tripod at one exposure: PROVIA, then PRO Neg Std, then PRO Neg Hi, without moving the camera between the three.
5. In-camera RAW conversion of every PROVIA RAF from steps 1 and 4, three times each: to PROVIA, to Velvia and to ASTIA, with every other conversion setting as shot (DR100, WB as shot, Color, Sharpness, Highlight, Shadow and NR 0). The camera writes each conversion as a new JPEG.
6. Copy the card unchanged to `private-data/photo-sessions/classic-chrome-YYYYMMDD/` and keep a note of which frame is which scene and bracket step.

### 8.2 Analysis

- Run the commands inside `devenv shell`, whose `ffmpeg` and `ffprobe` are the pinned ones the tool calls by default (`--ffmpeg PATH` and `--ffprobe PATH` override them).
- Settings: `exiftool -FilmMode -ISO -WhiteBalance -DynamicRange -Saturation -Sharpness -HighlightTone -ShadowTone -NoiseReduction -ExposureTime` on every JPEG; every frame must show the settings above and the intended film.
- Check (a), RAW conversion equals shooting: `classic-chrome-model photo-identical SHOT.JPG CONVERTED-PROVIA.JPG` for each PROVIA RAF. It decodes both with the pinned ffmpeg and passes when the Y, Cb and Cr planes are identical. A failure makes RAW-converted Velvia and ASTIA unusable as stand-ins for shot frames and is recorded before going on.
- Patch positions: `classic-chrome-model chart-patches X,Y X,Y X,Y X,Y SIZE` with the pixel centres of dark skin, bluish green, white 9.5 and black 2 read from the 0 EV PROVIA frame; SIZE about 60 % of the patch pitch. The tool samples the central half of each square.
- Manifest (`e-model.json`), one scene per chart bracket step and per natural scene; frame keys are `provia`, `velvia`, `astia`, `proNegStd`, `proNegHi`, paths relative to the manifest. RAW conversions of one RAF are pixel-aligned, so natural scenes add `"blocks": {"size": 32, "maxLumaSd": 1.5}` for automatic flat blocks; tripod frames from the shooting menu rely on the chart patches.

```json
{"scenes": [
  {"name": "chart 0 EV",
   "frames": {"provia": "DSCF0001.JPG", "velvia": "DSCF0101.JPG", "astia": "DSCF0102.JPG",
              "proNegStd": "DSCF0005.JPG", "proNegHi": "DSCF0009.JPG"},
   "patches": [{"name": "dark skin", "x": 1210, "y": 830, "w": 90, "h": 90}]}
]}
```

- Check (b), `e_model`: `classic-chrome-model e-model BUNDLE.json e-model.json OUT.json`. Each patch or block of the PROVIA frame is inverted through the X-M1 PROVIA stage (luma band, chroma 2x2, BT.601, curve A, matrix and gain table, each 8-bit level read as the middle of its curve bin), developed through the X-M1 stage of the other film, and compared with that film's frame. The output gives `e_model` per film for patches and blocks, the PROVIA re-development residual (the inverse's own error), the number of regions with a clipped channel that the inverse refuses, and the CIEDE2000 of using the PROVIA frame unchanged as the prediction, which shows how much of the film difference the model explains.

`e_model` measures the X-M1 side of the model: the stage arithmetic on this body, the selected gain records, luma bands and chroma gains, the DR100 tone keys, the inferred DR and H/S index mapping, and the JPEG path. It does not test the sensor models or the X-Trans II reference.

### 8.3 First session result (2026-10-21)

The before-set was shot inside the first RAM-push session (E-CC-RAM-PUSH-SESSION-150) without a ColorChecker, on a tabletop scene under incandescent light at ISO 4000 with Incandescent WB. Check (a) failed there: the shot PROVIA JPEG and the PROVIA RAW conversion of its own RAF differ in all three planes, so this set's converted Velvia and ASTIA frames are not stand-ins for shot frames. Conversions of one RAF are comparable with each other, and section 8.4 runs the `e-model` procedure on them with the ISO- and WB-dependent selectors mapped. The ColorChecker statistic of section 7 still needs a chart frame at ISO 200, DR100 and Daylight (or a fixed-WB light).

### 8.4 Implementation check on the RAW-conversion set

Question: does the body with the Classic Chrome RAM payload render the model's candidate stage, that is PROVIA's selectors with the data block's matrix, chroma 2x2 and tone record? Evidence class: hardware-observed images analysed with the host model, and a byte-read selector map of the 1.50 images (E-CC-IMPLEMENTATION-CHECK-150).

Input: the in-camera RAW conversions of `DSCF3149.RAF` (ISO 4000, Incandescent WB, DR100 set manually, adjustments 0, exiftool-checked): `DSCF3164` PROVIA and `DSCF3165` Velvia and `DSCF3166` ASTIA on RC0, `DSCF3169` Classic Chrome with the payload resident. The payload's data block comes from `candidate.json` `188c521d…3e92` ([patch report 9](classic-chrome-patch.md#9-verification-results-2026-10-18-host-and-emulated)), and the PROVIA conversion made with the payload resident is plane-identical to `DSCF3164` (patch report 8.5).

ISO and white balance are handled by mapping the selectors, not by a cancellation argument. ISO amplification and the white-balance multipliers act before the film stage's input `w`, which the inverse recovers from the PROVIA frame whatever they are. A cancellation argument covers only that and the shared gain record: the chroma gains and the luma-band factor sit after the matrix and tone curve, which differ between the two films, so a wrong gain there does not cancel. The map below lists every input of the stages the model represents, byte-read from image 5, image 4 and the fallback image. None of them is the white-balance setting, and the one ISO input leaves the DR100 values unchanged at ISO 4000:

| Stage selector | Inputs | Value for these frames |
| --- | --- | --- |
| Gain record and tone key | develop code, mode, H/S indices, `level2` from `FUN_0073fc64` | `level2` is 0 only when record `+0x19c = 2`; `FUN_00d78db8` stores 2 there only for capture `+0x4bc = 8`, the ISO 100 code of `FUN_00eaec80` (E-FILMSIM-FIELDS-150). Every other ISO leaves `level2` to the DR setting, 1 at DR100 |
| Luma bands, matrix, chroma 2x2 | develop code and row `+0x108` (written only 0) | as at ISO 200 |
| Chroma gain `0xd62194[+0x10c]` | Color index | unity |
| Chroma gain `0xd61f34[10 (+0x160) + (+0xf4)]` | `+0x160`, clamped to 0..3 by its setter `0x00d78c0c` (called from the still builder at `0x00d79568`); `+0xf4`, stored 2 by `FUN_00d791b0` | unity in every entry of rows 0..3 |
| Chroma gain `0xd620b0[5 (sel - 1) + (+0x154)]` | film selector, DR index | unity at DR index 0 |
| Chroma gain `0xd61e64[+0x13c]` | `+0x13c`, set only by the movie builder (`0x00d7868c` from `0x00eef3e0`), clamped to 0..9 | unity in entries 0..9 |
| Chroma gain `0xd62134[FUN_0073f338]` | scene and filter fields `+0xe4`, `+0xe8`, stored 0 by `FUN_00d791b0` | index 0, unity |

In image 5's develop code (`0x00d50000-0x00d80000`) the only word stores to `+0xe4`, `+0xe8` and `+0xf4` besides the initialiser's are in `FUN_00d753b8`, which fills a separate 0xd1c-byte buffer.

Method: `classic-chrome-model e-model BUNDLE.json MANIFEST.json OUT.json CANDIDATE.json` takes the frame key `classicChrome` through the X-M1 PROVIA stage with the candidate's matrix, chroma and tone record, and writes each region's PROVIA, observed and predicted L*a*b*. Flat blocks of `DSCF3164` (32 px, luma SD at most 1.5 over the central 16 px) are inverted through the X-M1 PROVIA stage and developed through the X-M1 stage of Velvia and ASTIA with their stock parameters (the controls) and through the candidate stage, then compared with the same block of the film frame. Two looser block sets test the selection. Refused blocks are those the inverse rejects; target-clipped blocks have a film-frame mean with an sRGB channel within half a level of 0 or 255 and stay in the statistics.

| Blocks | Film | Refused | Target clipped | Prediction, mean / p95 / max CIEDE2000 | PROVIA frame as prediction |
| --- | --- | ---: | ---: | --- | --- |
| 32 px, SD 1.5 (259) | Classic Chrome | 0 | 0 | 0.42 / 1.22 / 1.98 | 2.08 / 4.21 / 4.54 |
| | Velvia | 0 | 3 | 0.68 / 1.60 / 2.57 | 1.46 / 2.46 / 3.95 |
| | ASTIA | 0 | 0 | 0.98 / 1.80 / 2.96 | 0.93 / 1.70 / 2.18 |
| 32 px, SD 3.0 (10,276) | Classic Chrome | 0 | 0 | 0.64 / 1.70 / 3.14 | 3.51 / 4.86 / 6.72 |
| | Velvia | 0 | 474 | 0.73 / 1.64 / 3.52 | 2.48 / 4.68 / 6.04 |
| | ASTIA | 0 | 246 | 0.91 / 1.86 / 3.74 | 1.19 / 2.08 / 3.23 |
| 64 px, SD 3.0 (1,988) | Classic Chrome | 0 | 0 | 0.62 / 1.62 / 2.77 | 3.52 / 4.60 / 5.83 |
| | Velvia | 0 | 80 | 0.73 / 1.61 / 2.74 | 2.41 / 4.57 / 5.14 |
| | ASTIA | 0 | 43 | 0.91 / 1.83 / 3.91 | 1.12 / 2.03 / 3.11 |

The inverse's own floor, PROVIA inverted and developed through PROVIA again, is 0.60 / 1.50 / 2.40 on the 259 blocks (0.57-0.63 mean on the other sets). On the 129 chromatic blocks of the first set (PROVIA C* at least 5) the predictions give Classic Chrome 0.48 / 1.42 / 1.98, Velvia 0.72 / 1.55 / 2.03 and ASTIA 1.12 / 1.64 / 2.08.

Discrimination on the 259 blocks: the Classic Chrome frame predicted with one or more served items replaced by PROVIA's (private variant files in `private-data/analysis/classic-chrome/verify/`).

| Classic Chrome stage | Mean / p95 / max |
| --- | --- |
| Candidate, as the data block serves it | 0.42 / 1.22 / 1.98 |
| PROVIA's tone record | 0.88 / 1.95 / 2.05 |
| PROVIA's matrix | 0.86 / 2.18 / 3.01 |
| PROVIA's chroma 2x2 | 1.61 / 3.39 / 4.18 |
| PROVIA's matrix and chroma | 2.05 / 4.60 / 4.92 |
| PROVIA's stage | 2.38 / 5.01 / 5.64 |

The Classic Chrome error is at or below both controls on every block set, and swapping any one served item back to PROVIA's doubles to quadruples it. The controls also change stages Classic Chrome shares with PROVIA (both use gain record 101, and Velvia its own luma-band preset), so a lower Classic Chrome error is expected. ASTIA differs little from PROVIA on these colours: it fixes the error level but does not discriminate on the strict block set (0.98 against 0.93 for the unchanged PROVIA frame; 0.91 against 1.19 on the looser set).

Limits: the scene holds mostly yellow low-chroma highlights, a few dark blue blocks and dark neutrals (coverage in section 8.5). This is not the ColorChecker `e_model` of section 7, and it does not test the sensor models. The selector map covers word stores with immediate offsets in the develop code; a store from elsewhere or through a computed address is not excluded. The ISO code meaning of capture `+0x4bc` is static inference.

Verdict: the body applies the data block's matrix, chroma 2x2 and tone record over PROVIA's other selectors as the candidate stage describes, within the inverse's resolution of about 0.6 mean CIEDE2000 on the colours this scene holds. Nothing points to a stage that differs from the model.

### 8.5 Direction check against Fujifilm's F-Log2 LUTs

Question: does the X-M1 candidate move colours away from PROVIA the way Fujifilm's own Classic Chrome does? The reference is the GFX ETERNA 55 3D-LUT pack v1.10 (`https://dl.fujifilm-x.com/support/lut/gfx-eterna-55-3d-lut-v110.zip`, SHA-256 `febfc7050999620651ca0cf162bf8b499970ef270b4da632765b61b958cf7940`, kept in `private-data/analysis/classic-chrome/lut/` with `SOURCE.txt`), files `FLog2_to_PROVIA` and `FLog2_to_CLASSIC-CHROME` (also `Velvia` and `ASTIA`) at grids 33 and 65, F-Gamut to BT.709. It is a 2025 cinema reference from a later sensor and processor generation, so this is a direction check, not a target. Evidence class: reported reference and host model (E-CC-LUT-DIRECTION-150).

Method: `classic-chrome-model look-reference FILM PROVIA.cube FILM.cube BUNDLE.json CANDIDATE.json E-MODEL.json OUT.json` (module `LookReference`).

- Scene set, D65 reflection with white Y = 1: the neutral ramp (25), a low-chroma grid (L* 20-90 by 10, hue by 20 degrees, C* 4, 8, 12; 432) and the gamut set (642), all inside F-Gamut, whose primaries equal BT.2020's.
- LUT path, from Fujifilm's F-Log2 Data Sheet Ver.1.1 (`https://dl.fujifilm-x.com/support/lut/F-Log2_DataSheet_E_Ver.1.1.pdf`, SHA-256 `0aadf0c5…29cc`): XYZ to linear F-Gamut from the sheet's primaries and D65 white, the sheet's F-Log2 encoding (10-bit codes 95, 400 and 570 at 0, 18 and 90 % reflection, which the tests check), trilinear interpolation, and the BT.709 output read with the sRGB transfer function into CIELAB D65. The PROVIA LUT puts 18 % grey at 0.457 of full scale (117 of 255), where the model's exposure puts the camera's PROVIA (118).
- Model path: the same scene set through the X-M1 sensor model and the section 4 exposure, X-M1 PROVIA stage against the candidate stage (against the stock X-M1 Velvia and ASTIA stages for the calibration films).
- Pair: the 259 blocks of section 8.4, `DSCF3164` against `DSCF3169` (and the model's prediction for the same blocks).
- Statistics per pair of renderings, binned by the PROVIA rendering's CIELAB hue (30-degree sectors, C* at least 5), L* band and C* band: mean chroma ratio C*film / C*PROVIA, mean dL* and mean hue difference dh in degrees, film minus PROVIA.

The 33 and 65 grids agree within 0.017 in chroma ratio, 0.15 in dL* and 0.45 degrees in dh per sector for the three films, and within 0.31 L* on the ramp; the tables use the 65 grid.

Calibration with the X-M1's own films: the same comparison for Velvia and ASTIA, whose X-M1 side uses Fujifilm's stock X-M1 parameters, shows how well the LUT generation's relative look carries over to this body. Mean (max) absolute difference between the LUT and the X-M1 stage over the 12 sectors:

| Film | Chroma ratio, all chromatic, LUT / X-M1 | Chroma ratio | dL* | dh (degrees) | Sectors agreeing in sign: ratio, dL*, dh |
| --- | --- | --- | --- | --- | --- |
| Velvia (X-M1 stock) | 1.17 / 1.20 | 0.05 (0.08) | 0.74 (1.40) | 2.29 (7.07) | 12, 10, 8 |
| ASTIA (X-M1 stock) | 0.99 / 1.03 | 0.05 (0.09) | 0.81 (2.32) | 1.88 (4.26) | 12, 8, 10 |
| Classic Chrome (X-M1 candidate) | 0.76 / 0.76 | 0.03 (0.08) | 0.90 (2.18) | 1.65 (4.32) | 12, 9, 12 |

Signs agree when both values have the same sign, or both lie within 0.05 of 1 (ratio), 0.5 (dL*) or 2 degrees (dh). Chroma ratio carries over closely for the stock films; sector dL* and small hue shifts are at the level of the generation difference.

Classic Chrome against PROVIA per hue sector, LUT / X-M1 candidate, with the pair's observed values where the scene has blocks:

| PROVIA hue sector | Chroma ratio | dL* | dh | Pair: blocks, ratio, dL*, dh |
| --- | --- | --- | --- | --- |
| 0-30 | 0.80 / 0.81 | -0.67 / +0.15 | +2.8 / +3.9 | none |
| 30-60 | 0.82 / 0.83 | -1.17 / -0.01 | -1.7 / -2.0 | none |
| 60-90 | 0.76 / 0.78 | -0.07 / +0.37 | -3.6 / -6.5 | none |
| 90-120 | 0.71 / 0.71 | -0.40 / -0.39 | -4.7 / -6.1 | 116, 0.64, +1.46, -5.4 |
| 120-150 | 0.70 / 0.71 | -1.47 / -3.14 | -0.4 / +1.5 | none |
| 150-180 | 0.72 / 0.76 | -1.56 / -2.46 | +5.0 / +6.9 | none |
| 180-210 | 0.82 / 0.88 | -1.26 / -1.73 | +1.8 / +2.2 | none |
| 210-240 | 0.85 / 0.91 | -1.10 / -1.29 | -6.2 / -6.1 | none |
| 240-270 | 0.80 / 0.81 | -1.55 / -1.02 | -10.6 / -14.0 | 2, 0.55, -0.35, -18.8 |
| 270-300 | 0.70 / 0.67 | -1.30 / -0.43 | -7.8 / -12.2 | 11, 0.55, -0.38, -16.7 |
| 300-330 | 0.69 / 0.61 | -1.33 / +0.85 | +1.5 / +0.2 | none |
| 330-360 | 0.73 / 0.72 | -0.43 / +1.21 | +5.6 / +6.2 | none |

The pair's blocks are low-chroma: 128 of 129 have PROVIA C* below 15, 114 lie above L* 70 (the 90-120 sector) and 13 below L* 35 (the blue sectors). In the same bins, LUT / candidate / pair observed / pair predicted:

| Bin | Chroma ratio | dL* | dh |
| --- | --- | --- | --- |
| 90-120, C* 5-15 | 0.65 / 0.66 / 0.64 / 0.66 | -0.63 / +0.25 / +1.46 / +1.44 | -6.2 / -6.6 / -5.4 / -4.8 |
| 90-120, L* above 70 | 0.63 / 0.69 / 0.64 / 0.66 | +0.43 / +0.58 / +1.45 / +1.45 | -3.2 / -4.8 / -5.4 / -4.7 |
| 270-300, C* 5-15 | 0.64 / 0.64 / 0.55 / 0.53 | -0.08 / +0.23 / -0.38 / -0.39 | -8.2 / -11.7 / -16.7 / -19.9 |

Tone curve, neutral ramp, Classic Chrome L* minus PROVIA L*:

| Reflection Y | LUT PROVIA L* | LUT dL* | X-M1 PROVIA L* | Candidate dL* |
| ---: | ---: | ---: | ---: | ---: |
| 0.005 | 1.3 | -0.8 | 0.3 | 0.0 |
| 0.019 | 4.9 | -2.3 | 2.7 | 0.0 |
| 0.036 | 11.8 | -4.1 | 9.8 | -1.5 |
| 0.071 | 24.7 | -3.6 | 24.4 | -2.3 |
| 0.137 | 41.7 | -0.9 | 42.0 | -1.2 |
| 0.266 | 59.4 | +1.6 | 60.6 | +1.5 |
| 0.516 | 74.8 | +1.7 | 78.4 | +1.5 |
| 1.0 | 84.7 | +1.0 | 93.4 | +0.7 |

Reading:

- Chroma: both lower chroma in every sector, by the same amount overall (0.76 against 0.76) and within 0.08 per sector, closer than the stock-film calibration. The pair shows the same reduction (0.64 against the LUT's 0.65 in its main bin).
- Hue: the signs agree in all 12 sectors. Both turn blues (240-300) toward cyan by 8-14 degrees and yellows (60-120) toward orange, greens (150-180) toward cyan and magenta-reds (330-30) toward orange. The candidate's blue rotation is 1.3-1.6 times the LUT's, and the pair's dark low-chroma blues rotate further still (-17), which the model predicts for those blocks (-20).
- Lightness: the signs agree in 9 sectors. In the red-magenta sectors 300-30 the LUT darkens by 0.4-1.3 and the candidate lightens by 0.2-1.2, a disagreement of the size the calibration films show (8 and 10 sectors agreeing).
- Tone curve: the same shape, darker shadows and lighter mid-tones and highlights, crossing between Y 0.14 and 0.27. The LUT deepens the shadows about twice as much and already changes the darkest steps, where the candidate equals PROVIA up to Y 0.03; the Velvia and ASTIA LUTs show the same stronger shadow change against the X-M1's own films, so the gap belongs to the reference generation. The pair's yellow highlights are 1.45 L* lighter, which the ramp's highlight lift and the model's prediction for those blocks both give.

Coverage of the pair: the scene has chromatic blocks only in the 90-120 sector (116), 270-300 (11) and 240-270 (2), all below C* 15 but one, and neutral blocks only below L* 10. It does not cover the 0-90 sectors (reds, oranges, skin), 120-240 (greens, cyans), 300-360 (purples, magentas), any C* above 15, or neutral mid-tones and highlights; for those the comparison rests on the LUT and the model alone.

Limits: a different generation (GFX ETERNA 55, 2025) with its own PROVIA, rendered from F-Log2 scene data rather than through an in-camera JPEG path; the BT.709 output is read with the sRGB curve; colorimetric scene colours without spectra; the X-M1 side uses the public X-M1 sensor matrix; spec 19.5.6 asks for the F-Log2 path to be calibrated against the X-M1's own films, which is done here in relative form for Velvia and ASTIA only (no PRO Neg Std pixel-aligned frame).

Verdict: the candidate's Classic Chrome moves colours from PROVIA in the same direction as Fujifilm's Classic Chrome LUT in every hue sector for chroma and hue and on the tone curve, with matching overall chroma reduction; lightness differs in sign only in the red-magenta sectors, inside the spread the stock films show.

## 9. Limits and open risks

- The X-M1 gain records, luma-band presets and chroma gains are byte-read from the 1.50 images and the fallback adjustment image, and the body's own adjustment bytes in every span the stages read equal the fallback image in four full captures (section 2.1). Two links in their selection are inference: Highlight and Shadow 0 as index 2, and DR100 as `level2 = 1` with DR index 0.
- The X-Trans II gain records for Velvia, ASTIA and PRO Neg at DR100 (record 101) follow from the +100 family offset in an X70 DR400 live-view capture; the X70's DR100 conversion for those films is not logged.
- The hardware arithmetic applied to the X-M1 values (gain-table Bezier, luma-band factor, chroma algebra, #42) is the X70's; `e_model` tests it.
- The sensor models are linear 3x3 matrices from public raw-converter data, and the two X-Trans II entries (X70, X-T1) disagree. The relative method keeps the Classic Chrome rendering near the optimiser spread across them; the absolute method does not.
- Patches are colorimetric (Bradford-adapted CIELAB, no spectra), so metamerism between sensors is ignored.
- The relative method assumes the X-M1-to-X-Trans II difference measured on PROVIA carries over to Classic Chrome, as it does within `e_method` for the four leave-one-out films. Classic Chrome's chroma is much lower than any of them, so this is an extrapolation in chroma.
- The integer fit has a run-to-run spread of about 0.4-0.6 mean CIEDE2000 between equally good settings; the candidate is the best of three starts.
- Results other than section 6.6 are for DR100 with all adjustments at 0. Section 6.6 moves one adjustment at a time; combined adjustments and DR with Highlight or Shadow set are not tabulated, though `FUN_00d69df0` selects them by the same formula.
