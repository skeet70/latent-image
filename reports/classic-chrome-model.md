# Classic Chrome host colour model and fit

Host-only work for [spec/10 section 19.5](../spec/10-film-recipes.md#195-classic-chrome-rc2) and [CLASSIC-CHROME-01](https://github.com/skeet70/latent-image/issues/79). Evidence class: static inference over a host model and byte-read 1.50 develop-stage data (E-CC-COLOUR-MODEL-150). The model reproduces x70view's X70 film-stage model bit for bit; the X-M1 parameter sets take every develop-stage value from the pinned 1.50 images, and the hardware arithmetic they feed is the X70's, which the owner photo session in section 8 tests. Every Classic Chrome result here is an approximation of Classic Chrome as shipped on EXR Processor II with X-Trans CMOS II, derived from sibling bodies, and is not X-M1 ground truth. Fujifilm values (matrices, chroma entries, tone records, gain records, fitted candidate) stay under `private-data/analysis/classic-chrome/`; this report gives addresses, methods, residuals and thresholds.

## 1. Tools

| Target | Role |
| --- | --- |
| `//host:film-model` | `FujiAF.FilmStage` (integer film stage), `FujiAF.FilmInvert` (its floating-point inverse), `FujiAF.Colour` (CIELAB, CIEDE2000, sensor models, patch sets), `FujiAF.FilmFit` (integer matrix and chroma fit), `FujiAF.FilmGrid` (oracle grid), `FujiAF.XM1Develop` (1.50 develop-stage readers, adjustment field encoding), `FujiAF.ClassicChromeCandidate` (the v1 parameter file) |
| `//host/firmware:classic-chrome-model` | `tables`, `oracle-input`, `oracle-compare`, `bundle`, `method`, `fit-cc`, `photo-identical`, `chart-patches`, `e-model`, `settings`, `relative` (usage with no arguments) |
| `//host/firmware:film-stage-tests` | oracle fixtures, the 34 published CIEDE2000 pairs (Sharma, Wu and Dalal 2005), the parameter boundary, fit recovery of a reachable target, inverse round trip |
| `//host/firmware:candidate-tests` | the v1 parameter file on a synthetic candidate: field encoding and chroma storage order against hand-worked values, read-back, refusals |
| `//host/firmware:photo-session-tests` | the photo-session commands through the command line on flat JPEGs written by the pinned ffmpeg, and `relative` on packed planes: chroma ratio, block count and offset registration |

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

The before-set was shot inside the first RAM-push session (E-CC-RAM-PUSH-SESSION-150) without a ColorChecker, on a tabletop scene under incandescent light at ISO 4000 with Incandescent WB. Check (a) failed there: the shot PROVIA JPEG and the PROVIA RAW conversion of its own RAF differ in all three planes, so this set's converted Velvia and ASTIA frames are not stand-ins for shot frames. `e_model` was not computed: `e-model` has no ISO or WB input, and which develop selectors change with ISO 4000 or Incandescent WB is unmapped. A repeat at ISO 200, DR100 and Daylight (or a fixed-WB light) with the chart gives the intended input.

### 8.6 Relative check against X70 live view (2026-10-21 set)

Without a chart, the session set is compared by relative differences: on the same scene, how Classic Chrome moves each colour away from PROVIA, set beside the same differences on real X70 frames and in the model. Evidence class: hardware-observed frames (X70 live view; X-M1 1.50 with the RAM-pushed payload, E-CC-RAM-PUSH-SESSION-150) with host analysis (E-CC-RELATIVE-XREF-150).

`classic-chrome-model relative MANIFEST.json OUT.json [BUNDLE.json CANDIDATE.json]` reads pairs of a PROVIA and a Classic Chrome frame (JPEGs, or packed Y0 Y1 Cb Cr planes). It keeps the blocks whose sampled central half has luma SD at most `maxLumaSd` in both frames, the Classic Chrome block displaced by the pair's `offset`. Per block it computes the Classic Chrome over PROVIA C*ab ratio, dL* and the hue shift dh in degrees, binned by the PROVIA block's 30-degree hue sector (C* at least 5), its chroma band (5-15, 15-30, 30-50, 50 and above), and its L* for the tone response, neutral (C* below 5) and chromatic blocks separately. Given the bundle and candidate it adds two predictions:

- Patch set: the 691 patches of section 4 through the X-Trans II PROVIA and Classic Chrome stages (X70 sensor), and through the X-M1 PROVIA stage and the candidate (X-M1 sensor).
- Per block, for pairs marked `xm1Prediction`: each PROVIA block inverted through the X-M1 PROVIA stage and developed through the candidate with PROVIA's gain table, luma bands and chroma gains, as the RAM push installs it.

| Source | Frames | Blocks (chromatic) |
| --- | --- | ---: |
| X70 live view | x70view `research/hardware/ff80-film-calibration-20261011T233703Z-0d92ed` (revision `a47591d9bd`), captures `stock-0` and `stock-13`: three 720x480 live-view planes each of a laptop-screen target (24 hues at two strengths, grey ramp, white). x70view's film-probe mod writes each film's stock live-view film-stage words and reads them back equal. The two films' words differ only in matrix, tone and chroma. Matrix and tone equal the bundle's X-Trans II PROVIA and Classic Chrome values, and the chroma words are the still entries scaled by one factor common to both films. Gain table and luma bands are shared. 16-pixel blocks, SD at most 2.5 | 1816 (774) |
| X70 repeat control | PROVIA captures 0 and 1 of the same setting | 625 (283) |
| X-M1 3164/3169 | In-camera RAW conversions of DSCF3149.RAF to PROVIA and Classic Chrome, pixel-aligned; ISO 4000, Incandescent WB, DR100, adjustments 0 (EXIF). 32-pixel blocks, SD at most 3.0 | 9714 (8858) |
| X-M1 3168/3167 | Shot PROVIA and Classic Chrome frames of the same scene, same settings. Offset (+2, 0) from an RMSE subimage search over five regions, which give (+2, 0) at top and centre and (0, 0) at the bottom | 9631 (8150) |

The X70 set holds real-hardware PROVIA and Classic Chrome renderings of one scene with read-back settings, so public same-scene pairs were not collected. The X70 set covers the live-view path only: no still develop and no JPEG encoder.

Per hue sector, chromatic blocks pooled over chroma bands. Each cell is n, C* ratio, dL*, dh:

| Hue sector | X70 live view | Model X-Trans II | X-M1 3164/3169 | X-M1 3164/3169 predicted per block | Model X-M1 candidate | X-M1 3168/3167 |
| --- | --- | --- | --- | --- | --- | --- |
| 0-30 | 66, 0.82, -1.41, +3.5 | 50, 0.81, +0.28, +3.0 | - | - | 52, 0.82, +0.23, +2.9 | 2, 0.77, -0.03, -2.4 |
| 30-60 | 128, 0.83, -2.59, -1.3 | 70, 0.82, -0.25, -1.7 | 313, 0.83, -0.97, -5.1 | 313, 0.83, -1.08, -4.6 | 76, 0.82, -0.13, -1.7 | 278, 0.82, -0.90, -5.6 |
| 60-90 | 217, 0.74, +0.24, -3.9 | 54, 0.78, +0.47, -2.9 | 401, 0.77, -1.61, -9.7 | 401, 0.77, -1.72, -8.4 | 48, 0.78, +0.48, -4.1 | 332, 0.75, -1.32, -10.5 |
| 90-120 | 72, 0.77, +0.83, -5.9 | 53, 0.77, -0.46, -4.7 | 7700, 0.64, +0.04, -7.1 | 7700, 0.66, -0.02, -5.8 | 54, 0.74, -0.51, -5.8 | 7143, 0.62, +0.49, -7.6 |
| 120-150 | 18, 0.80, -4.15, +2.4 | 105, 0.75, -3.98, +1.2 | 162, 0.58, -2.08, -4.6 | 162, 0.60, -2.16, -4.3 | 99, 0.73, -3.80, +1.5 | 69, 0.56, -1.26, -5.6 |
| 150-180 | 26, 0.82, -2.57, +5.5 | 57, 0.79, -2.94, +6.4 | 6, 0.76, +0.29, +4.7 | 6, 0.80, +0.22, +3.3 | 45, 0.78, -3.50, +7.0 | 7, 0.75, +0.94, +8.0 |
| 180-210 | - | 37, 0.91, -2.41, -0.1 | 2, 0.95, -0.06, +2.0 | 2, 0.92, -0.19, +1.8 | 46, 0.89, -2.64, +3.9 | 4, 0.97, +0.27, +6.3 |
| 210-240 | - | 27, 0.92, -1.48, -7.9 | 5, 0.97, -0.51, -3.4 | 5, 0.97, -0.53, -6.3 | 34, 0.93, -2.03, -5.5 | 4, 0.99, +0.20, -2.1 |
| 240-270 | 7, 0.86, -0.57, -18.0 | 31, 0.82, -1.59, -13.1 | 69, 0.63, -1.19, -16.2 | 69, 0.62, -1.30, -22.6 | 37, 0.82, -1.40, -12.8 | 39, 0.74, -1.18, -8.2 |
| 270-300 | 60, 0.74, -1.41, -9.7 | 64, 0.68, -0.44, -12.2 | 110, 0.57, -1.03, -14.5 | 110, 0.55, -1.09, -18.4 | 66, 0.68, -0.57, -12.1 | 172, 0.62, -0.72, -12.5 |
| 300-330 | 110, 0.71, +0.19, -0.5 | 52, 0.65, +0.36, -0.7 | 87, 0.53, -1.06, -4.3 | 87, 0.49, -1.17, -6.3 | 44, 0.63, +0.95, +0.7 | 98, 0.57, -0.94, -5.4 |
| 330-360 | 70, 0.80, -0.69, +4.1 | 60, 0.72, +1.29, +5.2 | 3, 0.56, -1.28, +3.0 | 3, 0.55, -1.41, +14.4 | 59, 0.72, +1.40, +5.8 | 2, 0.66, -1.51, -4.1 |
| All chromatic | 774, 0.77, -0.74, -1.9 | 660, 0.77, -1.06, -1.5 | 8858, 0.65, -0.15, -7.2 | 8858, 0.67, -0.21, -6.2 | 660, 0.77, -1.05, -1.4 | 8150, 0.63, +0.30, -7.7 |

The sector mean mixes chroma and lightness, and the X70 target is mostly at C* above 50 while the X-M1 scene is mostly below 15, so like cells are compared by sector and chroma band. These are all cells where both X70 and X-M1 3164/3169 hold at least five blocks:

| Sector, chroma band | X70 live view | X-M1 3164/3169 | X-M1 predicted per block | Model X-Trans II | Model X-M1 candidate |
| --- | --- | --- | --- | --- | --- |
| 30-60, 5-15 | 15, 0.72, -0.26, -0.5 | 20, 0.73, -0.94, -5.1 | 20, 0.71, -1.09, -3.0 | 4, 0.73, +0.70, +4.0 | 4, 0.76, +0.54, -1.6 |
| 30-60, 15-30 | 8, 0.72, -1.63, -3.5 | 15, 0.76, -1.26, -4.3 | 15, 0.76, -1.39, -2.9 | 12, 0.81, +0.35, -3.9 | 12, 0.83, +0.51, -2.6 |
| 30-60, 50+ | 105, 0.85, -3.00, -1.2 | 179, 0.85, -0.82, -5.2 | 179, 0.85, -0.92, -4.9 | 37, 0.84, -0.66, -1.1 | 43, 0.83, -0.53, -1.4 |
| 60-90, 5-15 | 124, 0.70, +1.10, -4.9 | 74, 0.76, -1.66, -10.1 | 74, 0.77, -1.74, -6.3 | 4, 0.68, +0.83, -5.7 | 5, 0.72, +0.92, -8.6 |
| 60-90, 15-30 | 54, 0.73, -1.63, -5.5 | 244, 0.76, -1.63, -9.9 | 244, 0.76, -1.75, -9.1 | 10, 0.72, +0.69, -6.9 | 10, 0.75, +0.81, -6.5 |
| 90-120, 5-15 | 63, 0.75, +1.14, -6.2 | 7248, 0.64, +0.09, -6.9 | 7248, 0.66, +0.04, -5.7 | 5, 0.71, +0.61, -3.1 | 6, 0.64, +0.55, -6.3 |
| 90-120, 50+ | 9, 0.91, -1.36, -3.8 | 14, 0.82, -0.71, -4.7 | 14, 0.82, -0.76, -4.5 | 25, 0.83, -0.76, -4.1 | 23, 0.81, -0.81, -4.8 |

Tone response, each cell n and dL* for neutral blocks, then for chromatic blocks, by PROVIA L*:

| PROVIA L* | X70 live view | Model X-Trans II | X-M1 3164/3169 | X-M1 predicted per block | Model X-M1 candidate | X-M1 3168/3167 |
| --- | --- | --- | --- | --- | --- | --- |
| 0-10 | 1025, +0.01; 46, -0.51 | 11, -0.33; 28, -1.03 | 656, -0.50; 175, -0.74 | 656, -0.54; 175, -0.83 | 11, -0.33; 28, -1.05 | 855, -0.24; 214, -0.45 |
| 10-20 | 17, -1.30; 68, -1.54 | 2, -1.91; 26, -2.99 | 181, -1.48; 1723, -2.01 | 181, -1.57; 1723, -2.08 | 2, -1.91; 25, -2.50 | 620, -1.19; 1704, -1.61 |
| 20-30 | -; 3, -2.26 | 1, -2.26; 45, -2.91 | 8, -2.53; 1678, -2.44 | 8, -2.56; 1678, -2.55 | 1, -2.26; 47, -2.92 | 4, -1.23; 1006, -1.67 |
| 30-40 | -; 3, -2.44 | 3, -2.18; 62, -3.16 | 11, -2.40; 511, -1.56 | 11, -2.44; 511, -1.66 | 3, -2.18; 65, -2.77 | 2, -2.33; 401, -1.20 |
| 40-50 | -; 193, -1.65 | 2, -0.82; 64, -1.90 | -; 122, -0.91 | -; 122, -1.02 | 2, -0.82; 67, -1.79 | -; 114, -0.70 |
| 50-60 | -; 221, -1.07 | 2, +0.59; 97, -0.99 | -; 337, +0.80 | -; 337, +0.73 | 2, +0.59; 98, -1.00 | -; 542, +0.91 |
| 60-70 | -; 104, -0.10 | 3, +1.63; 109, +0.21 | -; 1893, +1.66 | -; 1893, +1.62 | 3, +1.63; 107, +0.34 | -; 2474, +1.72 |
| 70-80 | -; 134, +1.00 | 2, +1.65; 131, -0.01 | -; 2367, +1.61 | -; 2367, +1.58 | 2, +1.65; 130, -0.18 | -; 1661, +1.65 |
| 80-90 | -; 2, +1.44 | 3, +1.19; 98, -0.70 | -; 52, +1.12 | -; 52, +1.11 | 3, +1.19; 93, -0.84 | -; 34, +1.15 |

Findings:

- The X70 hardware pair agrees with the model's X-Trans II pair in every sector the target covers: C* ratio within 0.08 and hue shift of the same sign within 5 degrees. The X70's ratio is higher by 0.06-0.08 in 270-360 degrees. dL* differs by up to 2.3 between the screen colours and the patch set. The repeat control stays within 0.007 in ratio, 0.09 in dL* and 0.6 degrees in every sector, so these differences are not capture noise. This is the first hardware check of the X-Trans II reference the fit targets, on the live-view path.
- On the X-M1 conversion pair, the per-block prediction matches the measured Classic Chrome in every sector with at least 20 blocks: ratio within 0.04 and dL* within 0.11. The hue shift is within 1.3 degrees from 30 to 150 degrees. In 240-330 degrees, where the blocks are mostly below C* 15, the measured rotation is 2-6 degrees smaller than predicted. Over all blocks the predicted Classic Chrome is 0.64 / 1.70 / 3.14 CIEDE2000 (mean / p95 / max) from the measured, against the inverse's own PROVIA re-development residual of 0.57 / 1.60 / 3.36 and 3.50 / 4.79 / 6.72 for the PROVIA frame taken unchanged. With blocks at SD at most 1.5 (242 blocks) these are 0.40, 0.62 and 1.97, and the pooled ratio is 0.63 measured against 0.64 predicted.
- Desaturation: every populated X-M1 sector has a ratio between 0.53 and 0.85. The ratio rises with chroma (0.64, 0.72, 0.80 and 0.85 over the four bands), as in the model (0.67-0.79) and the X70 (0.72 to 0.80-0.84). It is lowest in the blue-purple sectors (0.53-0.63 at 240-330 degrees), where the model and the X70 are also lowest. In matched cells the X-M1 ratio is within 0.06 of the X70's except at 90-120 degrees: 0.64 against 0.75 at C* 5-15, where the X70 blocks are the screen's tinted greys, and 0.82 against 0.91 at C* 50+ (14 and 9 blocks). The model gives 0.64-0.71 and 0.81-0.83 there.
- Hue: the X-M1 rotates negative in every sector with at least 20 blocks: -4.6 to -9.7 degrees from 30 to 150 degrees, -14.5 to -16.2 from 240 to 300 and -4.3 at 300-330. The patch-set model has the same sign and a smaller rotation at 30-120 degrees (-1.7 to -5.8) and at 240-300. At 120-150 and 300-330 it gives -0.7 to +1.5, where the measurement and the per-block prediction both give -4.3 to -6.3. In matched cells the X-M1 rotates 0.7-5.2 degrees more than the X70, most in 30-90 degrees. The per-block prediction reproduces the measured rotation on the X-M1's own colours, for example -4.9 against -5.2 at 30-60 degrees, C* 50+. In the matched cells with at least 10 patches each, the two patch-set model columns agree within 1.3 degrees. The extra rotation therefore comes from where the scene's colours sit within each bin, not from the candidate departing from X-Trans II.
- Tone: neutrals darken by 1.5 L* at PROVIA L* 10-20 and by 2.4-2.5 at L* 20-40. That is within 0.1 of the per-block prediction and within 0.45 of the model's key-38 curve on the neutral ramp. Chromatic blocks at L* 60-80 gain 1.6, within 0.05 of the per-block prediction and close to the model's neutrals at those levels (+1.63, +1.65). The patch set's chromatic patches there move -0.2 to +0.3. The X70 grey ramp renders at C* 5 and above in both films, so X70 neutral data exists only below L* 20 (-1.30 against the model's -1.91 at L* 10-20).
- The shot pair 3168/3167 gives the same direction (pooled ratio 0.63 against 0.65, per sector within 0.11 of the conversion pair). Its predicted-against-measured residual is 1.08 mean CIEDE2000 and folds in misregistration and two exposures.

Verdict: relative to PROVIA, the X-M1 Classic Chrome behaves as the model expects. Direction is right in every tested sector: lower chroma, negative hue rotation where the model has it, shadows darker and light tones lighter. Magnitude matches the per-block prediction within 0.04 in C* ratio, 0.11 in dL* and about 1.3 degrees of hue in the warm sectors. The X70 hardware frames confirm the X-Trans II reference the candidate was fitted to, within 0.08 in ratio and 5 degrees in hue. This is not the look acceptance of section 7, which needs the chart frames.

Limits:

- Scene coverage: the blocks come from few surfaces of one tabletop, so neighbouring blocks are not independent; counts are blocks, not surfaces. 7248 of the 8858 chromatic X-M1 blocks are low-chroma yellow (90-120 degrees, C* 5-15). Untested on the X-M1: reds (0-30 degrees, at most 2 blocks), greens through azure (150-240 degrees, at most 7 blocks per sector, all below C* 30) and magenta-red (330-360 degrees, at most 3 blocks). Chroma above 30 is tested only at 30-120 degrees. Blue (240-300 degrees) is tested only below C* 15 apart from 4 blocks, and purple (300-330) only below C* 30. Neutrals above L* 40 are not in either camera's data. The X70 target lacks 180-240 degrees.
- ISO 4000 and Incandescent WB: the patch-set model renders D65 colours at the DR100 selectors, the conditions of section 6. Both frames of a pair come from one raw, or from one scene with the same settings, and a Classic Chrome capture carries PROVIA's develop code, so any ISO- or WB-dependent selector acts on both frames. Conditioned on the PROVIA output colour, the difference then depends on Classic Chrome's matrix, chroma and tone record, the stage arithmetic and the selectors they meet. The light changes which colours the scene holds and how noisy they are. Most flat blocks render at C* 5-15 in the yellow sector, and at C* 8 one unit of a* or b* is 7 degrees of hue, which limits the low-chroma hue comparison. The per-block prediction inverts and re-develops with the DR100 selectors. The inverse absorbs a selector that acts before the matrix (the gain table), and the residual at the inverse's own level (0.64 against 0.57) shows no other unmodelled difference at this precision. The ISO- and WB-dependent selectors stay unmapped. Patch-set columns therefore give direction and rough magnitude, while the per-block columns are the like-for-like comparison.
- The X70 frames are live view: the live view's own gain table, live-view chroma scaling, 8-bit clipping of saturated screen colours in both films, and an emissive target under the live view's own white balance.

Reproduction (`P=private-data/analysis/classic-chrome`; the manifest under `$P/xref/` names the frames by absolute path, and `manifest-sd1.5.json` is the SD 1.5 variant):

```sh
classic-chrome-model relative $P/xref/manifest.json $P/xref/relative.json $P/bundle.json $P/candidate.json
```

## 9. Limits and open risks

- The X-M1 gain records, luma-band presets and chroma gains are byte-read from the 1.50 images and the fallback adjustment image, and the body's own adjustment bytes in every span the stages read equal the fallback image in four full captures (section 2.1). Two links in their selection are inference: Highlight and Shadow 0 as index 2, and DR100 as `level2 = 1` with DR index 0.
- The X-Trans II gain records for Velvia, ASTIA and PRO Neg at DR100 (record 101) follow from the +100 family offset in an X70 DR400 live-view capture; the X70's DR100 conversion for those films is not logged.
- The hardware arithmetic applied to the X-M1 values (gain-table Bezier, luma-band factor, chroma algebra, #42) is the X70's; `e_model` tests it.
- The sensor models are linear 3x3 matrices from public raw-converter data, and the two X-Trans II entries (X70, X-T1) disagree. The relative method keeps the Classic Chrome rendering near the optimiser spread across them; the absolute method does not.
- Patches are colorimetric (Bradford-adapted CIELAB, no spectra), so metamerism between sensors is ignored.
- The relative method assumes the X-M1-to-X-Trans II difference measured on PROVIA carries over to Classic Chrome, as it does within `e_method` for the four leave-one-out films. Classic Chrome's chroma is much lower than any of them, so this is an extrapolation in chroma.
- The integer fit has a run-to-run spread of about 0.4-0.6 mean CIEDE2000 between equally good settings; the candidate is the best of three starts.
- Results other than section 6.6 are for DR100 with all adjustments at 0. Section 6.6 moves one adjustment at a time; combined adjustments and DR with Highlight or Shadow set are not tabulated, though `FUN_00d69df0` selects them by the same formula.
