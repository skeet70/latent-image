# RAM-push D3 design: pushing the complete f6502602 delta over installed RC0

Imported from the private research history (fuji-af) at `179c22e`; commit ids here name that history. It designs the 249-publication push; the 250-publication contract in the tree adds the D3 reopen fix ([session script](ram-push-session-script.md) P1).

Host-only design, 2026-10-17. No camera I/O, no emulator run and no source change were made for this report. Evidence labels: **byte-read** (a direct read of a pinned artifact in this pass), **static-inference** (a conclusion drawn from those bytes), **hardware-observed** and **emulated** (cited records), **inherited** (a named record reused without re-derivation), **open** (a named missing fact).

## 1. Result

The installed body runs the RC0 container `acb0fbbb…9ca8`. The full candidate is `f6502602…bba3` (`private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/package-field-marker/recipe-complete.DAT`). The bank part of that candidate is already RAM-pushed through 94 publications (E-RAM-PUSH-INSTALL-150, E-RAM-PUSH-SESSION-150). Everything else the candidate flashes falls into four groups:

1. **D3 runtime words, 124 changed words plus 31 validate-only words = 155 new publications.** All are data or code read after boot and are patchable in RAM inside the existing two-core rendezvous. They are the 516-byte prefix window `[0x0061c1fc,0x0061c400)` (129 words), two film conversion rows (4 words), six list-node and two Q-node pointer words (8 words), the image-5 site word `0x00d50a48` (1 word) and the image-2 `0x83d` name record (13 words).
2. **The 14 RC0 availability cells: zero delta.** RC0 already carries them (byte-read: every one of the 14 cells at `0x00795db4..0x00795e60` holds the D3 value in `update/prefix/id4-patched.bin`).
3. **Bank flash-boot carrier: not pushed.** The `0x0069d2e0` boot hook, the 72 bootstrap stub words in four freed stock-array runs, the heap-ceiling instruction `0x0072b130` and the 0xa140-byte source package in the plain-image gap exist only to publish the bank at boot. The reviewed RAM installer replaces all of them.
4. **Container-only and boot-loader-consumed fields: cannot be pushed, and the running image needs none of them.** These are the main checksum, the record-4 header, the segment and trailer load/size words and the RAM copy of segment entry 4 at `0x0061c0d4`/`0x0061c0d8`.

The extended transaction carries **249 publications: 167 setup (the bank's 69 plus D3's 98) and 82 activation (D3's 57 plus the bank's 25)**. The bank's root word, logical 54, stays last. All 249 old words are validated before COMMIT, and rollback is the exact reverse. The publication table grows from 0x5e0 to 0xf90 bytes. With the installer text unchanged at 0xa84 bytes, the installer extent becomes 0x1b20 and the fixed allocation becomes **0x1b6b8** (from 0x1ad18). The installer logic, the rendezvous, the peer-cache protocol, rollback and host authentication are unchanged. The changes are constants, the publication table and the builder's derivation of the new entries.

A RAM push built this way runs the same D3 bytes, at the same addresses, as the flashed image. That includes the hook's instruction fetch from `[0x0061c3ac,0x0061c3f8)`, the one unproved premise of the flashed D3 ([recipe-patch-d3](recipe-patch-d3.md) section 5.7). The RAM push does not test the bank's flash-boot path (bootstrap gates, source fetch, refusal fallback, 64 KiB heap reduction) or the container itself. Section 6 lists the residual differences.

## 2. Pinned inputs

| Role | Path | SHA-256 |
| --- | --- | --- |
| installed container (RC0) | `private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT` | `acb0fbbb9507421f48a6254920cafb5ce20e1fd6bf821bc80c912c1f99c29ca8` |
| RC0 image 4 decode, base `0x0061c340` | `update/prefix/id4-patched.bin` | `a4d169bd961ec7df34e3b9b7108b624b79680c3cc50297e434e0b3607fb65551` |
| RC0 image 5 (= stock), base `0x00bf1a00` | `lz/id5-00bf1a00.bin` | `3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a` |
| RC0 image 2 (= stock), base `0x00130000` | `filmsim/id2-00130000.bin` | `236d12a11c8b386f71192675c12398358840327f21e392e6eaa91ddfd6e020a5` |
| RC0 plain image (= stock), base `0x01100000` | `lz/plain-01100000.bin` | `ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d` |
| candidate container | `recipes/rc-complete-20261013/package-field-marker/recipe-complete.DAT` | `f65026021aa5f58027c89374109eff3435ac71f9296c163030751c8645bbb7a3` |
| candidate image 4, base `0x0061c1fc` | `…/image/id4-final.bin` | `fcacb735eea3239b0d1043c663731323a33d60d6236efba1edce810340b71201` |
| D3-only image 4 (candidate minus bank boot carrier) | `…/d3/id4-patched.bin` | `6e5df0b7a8a8083679d5cd78e314598e192f7aa04969ca296f5ea381ea5661e0` |
| candidate and D3 image 5 (identical) | `…/image/id5-final.bin`, `…/d3/id5-patched.bin` | `5626062e760975dee8cff85c0d162e86e0be2f36df738af47df9dfd8cc85c8bb` |
| candidate image 2 | `…/d3/id2-patched.bin` | `1b8a281c34450ae7d9a195298fb66398e750ddbb599c3b98e152782416dcf294` |
| reviewed RAM runtime at base `0x00aa0d60` | `ram-push/operator-linked-2/runtime.bin` | `624fe80a26bbb9fdbad07d44c190bba957c9d6c319e4a329652ba42802ab6759` |

All paths below `private-data/analysis/x-m1-1.50/` unless absolute. The full-container comparison `recovery/rc1-preflash/compare-rc0-vs-f6502602.txt` lists the changed 128-KiB blocks: 10 (segment table), 11-21 (record 4), 26 (record 5), 50 (source package), 119 (image 2) and 293 (trailer). The independent whole-image review E-RECIPE-RC-FINAL-IMAGE-REVIEW-150 accounts for every byte in them.

## 3. The enumerated delta

### 3.1 Method

The method is word-by-word comparison of the decoded images, each at its own load address (byte-read in this pass):

- Image 4: RC0 `id4-patched.bin` at `0x0061c340` against `id4-final.bin` at `0x0061c1fc` over `[0x0061c1fc,0x0089ec00)`, with RC0 words below `0x0061c340` taken as zero (section 4.1). **184 words differ.** Exactly 110 of them are in the D3-only decode's difference from RC0, and the other 74 are the bank boot carrier, which the D3-only decode lacks. The two sets are disjoint and their union is the 184 (computed).
- Image 5: stock against candidate. **One word differs**, `0x00d50a48`.
- Image 2: stock against candidate. **47 bytes differ** in `[0x0037b7c6,0x0037b7f8)`, which is 13 aligned words in `[0x0037b7c4,0x0037b7f8)`.
- Plain image: the 0xa140-byte source package at `[0x011ad800,0x011b7940)` (block 50). No other plain byte differs (E-RECIPE-RC-FINAL-IMAGE-REVIEW-150).
- Container fields: read from both DATs at media `0x160000` and `0x2c0000` and the table block (section 3.4).

### 3.2 D3 publications (the 155 new entries)

The `#` column is the proposed logical index, continuing after the bank's 0..93. The table lists entries in publication order. Validate-only rows have old = new: the installer checks the word and stores the same value. The band rows carry them because no RC0 decode covers that band (section 4.1), and the overlap rows because the whole 516-byte window is published as one unit.

| # | Phase | Group | Image | Address | Old (RC0) | New (f6502602) | Content |
| ---: | --- | --- | --- | --- | --- | --- | --- |
| 94 | setup | S1 prefix band | img4 | `0x0061c1fc` | `00000000` | `00000000` | values array (validate only) |
| 95 | setup | S1 prefix band | img4 | `0x0061c200` | `00000000` | `00000000` | values array (validate only) |
| 96 | setup | S1 prefix band | img4 | `0x0061c204` | `00000000` | `00000003` | values array |
| 97 | setup | S1 prefix band | img4 | `0x0061c208` | `00000000` | `00000000` | values array (validate only) |
| 98 | setup | S1 prefix band | img4 | `0x0061c20c` | `00000000` | `00000001` | values array |
| 99 | setup | S1 prefix band | img4 | `0x0061c210` | `00000000` | `00000000` | values array (validate only) |
| 100 | setup | S1 prefix band | img4 | `0x0061c214` | `00000000` | `00000007` | values array |
| 101 | setup | S1 prefix band | img4 | `0x0061c218` | `00000000` | `00000000` | values array (validate only) |
| 102 | setup | S1 prefix band | img4 | `0x0061c21c` | `00000000` | `0000000c` | values array |
| 103 | setup | S1 prefix band | img4 | `0x0061c220` | `00000000` | `00000000` | values array (validate only) |
| 104 | setup | S1 prefix band | img4 | `0x0061c224` | `00000000` | `00000005` | values array |
| 105 | setup | S1 prefix band | img4 | `0x0061c228` | `00000000` | `00000000` | values array (validate only) |
| 106 | setup | S1 prefix band | img4 | `0x0061c22c` | `00000000` | `00000006` | values array |
| 107 | setup | S1 prefix band | img4 | `0x0061c230` | `00000000` | `00000000` | values array (validate only) |
| 108 | setup | S1 prefix band | img4 | `0x0061c234` | `00000000` | `00000002` | values array |
| 109 | setup | S1 prefix band | img4 | `0x0061c238` | `00000000` | `00000000` | values array (validate only) |
| 110 | setup | S1 prefix band | img4 | `0x0061c23c` | `00000000` | `00000004` | values array |
| 111 | setup | S1 prefix band | img4 | `0x0061c240` | `00000000` | `00000000` | values array (validate only) |
| 112 | setup | S1 prefix band | img4 | `0x0061c244` | `00000000` | `7ffffffe` | values array |
| 113 | setup | S1 prefix band | img4 | `0x0061c248` | `00000000` | `00000000` | values array (validate only) |
| 114 | setup | S1 prefix band | img4 | `0x0061c24c` | `00000000` | `0000048a` | shooting labels |
| 115 | setup | S1 prefix band | img4 | `0x0061c250` | `00000000` | `00000489` | shooting labels |
| 116 | setup | S1 prefix band | img4 | `0x0061c254` | `00000000` | `0000048c` | shooting labels |
| 117 | setup | S1 prefix band | img4 | `0x0061c258` | `00000000` | `0000048b` | shooting labels |
| 118 | setup | S1 prefix band | img4 | `0x0061c25c` | `00000000` | `0000048e` | shooting labels |
| 119 | setup | S1 prefix band | img4 | `0x0061c260` | `00000000` | `0000048d` | shooting labels |
| 120 | setup | S1 prefix band | img4 | `0x0061c264` | `00000000` | `00000490` | shooting labels |
| 121 | setup | S1 prefix band | img4 | `0x0061c268` | `00000000` | `0000048f` | shooting labels |
| 122 | setup | S1 prefix band | img4 | `0x0061c26c` | `00000000` | `00000492` | shooting labels |
| 123 | setup | S1 prefix band | img4 | `0x0061c270` | `00000000` | `00000491` | shooting labels |
| 124 | setup | S1 prefix band | img4 | `0x0061c274` | `00000000` | `00000494` | shooting labels |
| 125 | setup | S1 prefix band | img4 | `0x0061c278` | `00000000` | `00000493` | shooting labels |
| 126 | setup | S1 prefix band | img4 | `0x0061c27c` | `00000000` | `0000049f` | shooting labels |
| 127 | setup | S1 prefix band | img4 | `0x0061c280` | `00000000` | `0000049e` | shooting labels |
| 128 | setup | S1 prefix band | img4 | `0x0061c284` | `00000000` | `000004a2` | shooting labels |
| 129 | setup | S1 prefix band | img4 | `0x0061c288` | `00000000` | `000004a0` | shooting labels |
| 130 | setup | S1 prefix band | img4 | `0x0061c28c` | `00000000` | `000004b0` | shooting labels |
| 131 | setup | S1 prefix band | img4 | `0x0061c290` | `00000000` | `000004af` | shooting labels |
| 132 | setup | S1 prefix band | img4 | `0x0061c294` | `00000000` | `000004b2` | shooting labels |
| 133 | setup | S1 prefix band | img4 | `0x0061c298` | `00000000` | `000004b1` | shooting labels |
| 134 | setup | S1 prefix band | img4 | `0x0061c29c` | `00000000` | `0000049b` | shooting labels |
| 135 | setup | S1 prefix band | img4 | `0x0061c2a0` | `00000000` | `0000049a` | shooting labels |
| 136 | setup | S1 prefix band | img4 | `0x0061c2a4` | `00000000` | `00000499` | shooting labels |
| 137 | setup | S1 prefix band | img4 | `0x0061c2a8` | `00000000` | `0000049c` | shooting labels |
| 138 | setup | S1 prefix band | img4 | `0x0061c2ac` | `00000000` | `00000497` | shooting labels |
| 139 | setup | S1 prefix band | img4 | `0x0061c2b0` | `00000000` | `00000496` | shooting labels |
| 140 | setup | S1 prefix band | img4 | `0x0061c2b4` | `00000000` | `00000495` | shooting labels |
| 141 | setup | S1 prefix band | img4 | `0x0061c2b8` | `00000000` | `00000498` | shooting labels |
| 142 | setup | S1 prefix band | img4 | `0x0061c2bc` | `00000000` | `0000048a` | shooting labels |
| 143 | setup | S1 prefix band | img4 | `0x0061c2c0` | `00000000` | `00000489` | shooting labels |
| 144 | setup | S1 prefix band | img4 | `0x0061c2c4` | `00000000` | `0000048c` | shooting labels |
| 145 | setup | S1 prefix band | img4 | `0x0061c2c8` | `00000000` | `0000083d` | shooting labels |
| 146 | setup | S1 prefix band | img4 | `0x0061c2cc` | `00000000` | `0000048e` | shooting labels |
| 147 | setup | S1 prefix band | img4 | `0x0061c2d0` | `00000000` | `0000048d` | shooting labels |
| 148 | setup | S1 prefix band | img4 | `0x0061c2d4` | `00000000` | `00000490` | shooting labels |
| 149 | setup | S1 prefix band | img4 | `0x0061c2d8` | `00000000` | `0000048f` | shooting labels |
| 150 | setup | S1 prefix band | img4 | `0x0061c2dc` | `00000000` | `ffffffff` | shooting labels |
| 151 | setup | S1 prefix band | img4 | `0x0061c2e0` | `00000000` | `ffffffff` | shooting labels |
| 152 | setup | S1 prefix band | img4 | `0x0061c2e4` | `00000000` | `ffffffff` | shooting labels |
| 153 | setup | S1 prefix band | img4 | `0x0061c2e8` | `00000000` | `ffffffff` | shooting labels |
| 154 | setup | S1 prefix band | img4 | `0x0061c2ec` | `00000000` | `ff00077b` | Q labels |
| 155 | setup | S1 prefix band | img4 | `0x0061c2f0` | `00000000` | `00000000` | Q labels (validate only) |
| 156 | setup | S1 prefix band | img4 | `0x0061c2f4` | `00000000` | `00000000` | Q labels (validate only) |
| 157 | setup | S1 prefix band | img4 | `0x0061c2f8` | `00000000` | `00000000` | Q labels (validate only) |
| 158 | setup | S1 prefix band | img4 | `0x0061c2fc` | `00000000` | `ff00077c` | Q labels |
| 159 | setup | S1 prefix band | img4 | `0x0061c300` | `00000000` | `00000000` | Q labels (validate only) |
| 160 | setup | S1 prefix band | img4 | `0x0061c304` | `00000000` | `00000000` | Q labels (validate only) |
| 161 | setup | S1 prefix band | img4 | `0x0061c308` | `00000000` | `00000000` | Q labels (validate only) |
| 162 | setup | S1 prefix band | img4 | `0x0061c30c` | `00000000` | `ff000773` | Q labels |
| 163 | setup | S1 prefix band | img4 | `0x0061c310` | `00000000` | `00000000` | Q labels (validate only) |
| 164 | setup | S1 prefix band | img4 | `0x0061c314` | `00000000` | `00000000` | Q labels (validate only) |
| 165 | setup | S1 prefix band | img4 | `0x0061c318` | `00000000` | `00000000` | Q labels (validate only) |
| 166 | setup | S1 prefix band | img4 | `0x0061c31c` | `00000000` | `ff000777` | Q labels |
| 167 | setup | S1 prefix band | img4 | `0x0061c320` | `00000000` | `00000000` | Q labels (validate only) |
| 168 | setup | S1 prefix band | img4 | `0x0061c324` | `00000000` | `00000000` | Q labels (validate only) |
| 169 | setup | S1 prefix band | img4 | `0x0061c328` | `00000000` | `00000000` | Q labels (validate only) |
| 170 | setup | S1 prefix band | img4 | `0x0061c32c` | `00000000` | `ff00077a` | Q labels |
| 171 | setup | S1 prefix band | img4 | `0x0061c330` | `00000000` | `00000000` | Q labels (validate only) |
| 172 | setup | S1 prefix band | img4 | `0x0061c334` | `00000000` | `00000000` | Q labels (validate only) |
| 173 | setup | S1 prefix band | img4 | `0x0061c338` | `00000000` | `00000000` | Q labels (validate only) |
| 174 | setup | S1 prefix band | img4 | `0x0061c33c` | `00000000` | `0000049a` | Q labels |
| 175 | setup | S2 name record | img2 | `0x0037b7c4` | `003f0030` | `00570030` | 0x83d buffer |
| 176 | setup | S2 name record | img2 | `0x0037b7c8` | `e142e150` | `00520041` | 0x83d buffer |
| 177 | setup | S2 name record | img2 | `0x0037b7cc` | `e149e15f` | `0020004d` | 0x83d buffer |
| 178 | setup | S2 name record | img2 | `0x0037b7d0` | `e147e14d` | `00540053` | 0x83d buffer |
| 179 | setup | S2 name record | img2 | `0x0037b7d4` | `e153e15f` | `004e0041` | 0x83d buffer |
| 180 | setup | S2 name record | img2 | `0x0037b7d8` | `e141e145` | `00410044` | 0x83d buffer |
| 181 | setup | S2 name record | img2 | `0x0037b7dc` | `e143e152` | `00440052` | 0x83d buffer |
| 182 | setup | S2 name record | img2 | `0x0037b7e0` | `e15fe148` | `00000000` | 0x83d buffer |
| 183 | setup | S2 name record | img2 | `0x0037b7e4` | `e147e157` | `00000000` | 0x83d buffer |
| 184 | setup | S2 name record | img2 | `0x0037b7e8` | `e153e150` | `00000000` | 0x83d buffer |
| 185 | setup | S2 name record | img2 | `0x0037b7ec` | `e142e15f` | `00000000` | 0x83d buffer |
| 186 | setup | S2 name record | img2 | `0x0037b7f0` | `e152e141` | `00000000` | 0x83d buffer |
| 187 | setup | S2 name record | img2 | `0x0037b7f4` | `e146e15f` | `00000000` | 0x83d buffer |
| 188 | setup | S3 film row | img4 | `0x0077ac48` | `00000002` | `0000000d` | film row 2 col0 |
| 189 | setup | S3 film row | img4 | `0x0077ac50` | `00000002` | `00000000` | film row 2 col2 |
| 190 | setup | S3 film row | img4 | `0x0077ac60` | `00000008` | `0000000e` | film row 4 col0 |
| 191 | setup | S3 film row | img4 | `0x0077ac68` | `00000006` | `00000003` | film row 4 col2 |
| 192 | activation | A1 list pointer | img4 | `0x007823c8` | `0061c380` | `0061c24c` | shooting list labels ptr |
| 193 | activation | A1 list pointer | img4 | `0x007823c4` | `0061c340` | `0061c1fc` | shooting list values ptr |
| 194 | activation | A1 list pointer | img4 | `0x00785d98` | `0061c380` | `0061c24c` | Fn list labels ptr |
| 195 | activation | A1 list pointer | img4 | `0x00785d94` | `0061c340` | `0061c1fc` | Fn list values ptr |
| 196 | activation | A1 list pointer | img4 | `0x0077d1a8` | `0061c380` | `0061c24c` | node 0x77d184 labels ptr |
| 197 | activation | A1 list pointer | img4 | `0x0077d1a4` | `0061c340` | `0061c1fc` | node 0x77d184 values ptr |
| 198 | activation | A2 prefix overlap | img4 | `0x0061c340` | `00000000` | `00000000` | Q labels (validate only) |
| 199 | activation | A2 prefix overlap | img4 | `0x0061c344` | `00000000` | `00000000` | Q labels (validate only) |
| 200 | activation | A2 prefix overlap | img4 | `0x0061c348` | `00000003` | `00000000` | Q labels |
| 201 | activation | A2 prefix overlap | img4 | `0x0061c34c` | `00000000` | `00000496` | Q labels |
| 202 | activation | A2 prefix overlap | img4 | `0x0061c350` | `00000001` | `00000000` | Q labels |
| 203 | activation | A2 prefix overlap | img4 | `0x0061c354` | `00000000` | `00000000` | Q labels (validate only) |
| 204 | activation | A2 prefix overlap | img4 | `0x0061c358` | `00000007` | `00000000` | Q labels |
| 205 | activation | A2 prefix overlap | img4 | `0x0061c35c` | `00000000` | `ff00077b` | Q labels |
| 206 | activation | A2 prefix overlap | img4 | `0x0061c360` | `0000000c` | `00000000` | Q labels |
| 207 | activation | A2 prefix overlap | img4 | `0x0061c364` | `00000000` | `00000000` | Q labels (validate only) |
| 208 | activation | A2 prefix overlap | img4 | `0x0061c368` | `00000005` | `00000000` | Q labels |
| 209 | activation | A2 prefix overlap | img4 | `0x0061c36c` | `00000000` | `ff00077c` | Q labels |
| 210 | activation | A2 prefix overlap | img4 | `0x0061c370` | `00000006` | `00000000` | Q labels |
| 211 | activation | A2 prefix overlap | img4 | `0x0061c374` | `00000000` | `00000000` | Q labels (validate only) |
| 212 | activation | A2 prefix overlap | img4 | `0x0061c378` | `7ffffffe` | `00000000` | Q labels |
| 213 | activation | A2 prefix overlap | img4 | `0x0061c37c` | `00000000` | `ffffffff` | Q labels |
| 214 | activation | A2 prefix overlap | img4 | `0x0061c380` | `0000048a` | `ffffffff` | Q labels |
| 215 | activation | A2 prefix overlap | img4 | `0x0061c384` | `00000489` | `ffffffff` | Q labels |
| 216 | activation | A2 prefix overlap | img4 | `0x0061c388` | `0000048c` | `ffffffff` | Q labels |
| 217 | activation | A2 prefix overlap | img4 | `0x0061c38c` | `0000048b` | `00030102` | recipe row tables |
| 218 | activation | A2 prefix overlap | img4 | `0x0061c390` | `0000048e` | `0b000102` | recipe row tables |
| 219 | activation | A2 prefix overlap | img4 | `0x0061c394` | `0000048d` | `01020201` | recipe row tables |
| 220 | activation | A2 prefix overlap | img4 | `0x0061c398` | `00000490` | `08000003` | recipe row tables |
| 221 | activation | A2 prefix overlap | img4 | `0x0061c39c` | `0000048f` | `01100104` | property table |
| 222 | activation | A2 prefix overlap | img4 | `0x0061c3a0` | `00000492` | `010e0111` | property table |
| 223 | activation | A2 prefix overlap | img4 | `0x0061c3a4` | `00000491` | `00ef010f` | property table |
| 224 | activation | A2 prefix overlap | img4 | `0x0061c3a8` | `00000494` | `014100ed` | property table |
| 225 | activation | A2 prefix overlap | img4 | `0x0061c3ac` | `00000493` | `e92d4ff8` | hook code |
| 226 | activation | A2 prefix overlap | img4 | `0x0061c3b0` | `0000049f` | `e3550004` | hook code |
| 227 | activation | A2 prefix overlap | img4 | `0x0061c3b4` | `0000049e` | `e30c438c` | hook code |
| 228 | activation | A2 prefix overlap | img4 | `0x0061c3b8` | `000004a2` | `e3404061` | hook code |
| 229 | activation | A2 prefix overlap | img4 | `0x0061c3bc` | `000004a0` | `02844008` | hook code |
| 230 | activation | A2 prefix overlap | img4 | `0x0061c3c0` | `000004b0` | `0a000001` | hook code |
| 231 | activation | A2 prefix overlap | img4 | `0x0061c3c4` | `000004af` | `e3550002` | hook code |
| 232 | activation | A2 prefix overlap | img4 | `0x0061c3c8` | `000004b2` | `1a000007` | hook code |
| 233 | activation | A2 prefix overlap | img4 | `0x0061c3cc` | `000004b1` | `e30c639c` | hook code |
| 234 | activation | A2 prefix overlap | img4 | `0x0061c3d0` | `0000049b` | `e3406061` | hook code |
| 235 | activation | A2 prefix overlap | img4 | `0x0061c3d4` | `0000049a` | `e3a07008` | hook code |
| 236 | activation | A2 prefix overlap | img4 | `0x0061c3d8` | `00000499` | `e0d600b2` | hook code |
| 237 | activation | A2 prefix overlap | img4 | `0x0061c3dc` | `0000049c` | `e4d41001` | hook code |
| 238 | activation | A2 prefix overlap | img4 | `0x0061c3e0` | `00000497` | `eb1cce23` | hook code |
| 239 | activation | A2 prefix overlap | img4 | `0x0061c3e4` | `00000496` | `e2577001` | hook code |
| 240 | activation | A2 prefix overlap | img4 | `0x0061c3e8` | `00000495` | `1afffffa` | hook code |
| 241 | activation | A2 prefix overlap | img4 | `0x0061c3ec` | `00000498` | `e8bd4ff8` | hook code |
| 242 | activation | A2 prefix overlap | img4 | `0x0061c3f0` | `ffffffff` | `e1a00004` | hook code |
| 243 | activation | A2 prefix overlap | img4 | `0x0061c3f4` | `ffffffff` | `e12fff1e` | hook code |
| 244 | activation | A2 prefix overlap | img4 | `0x0061c3f8` | `ffffffff` | `00000000` | zero fill |
| 245 | activation | A2 prefix overlap | img4 | `0x0061c3fc` | `ffffffff` | `00000000` | zero fill |
| 246 | activation | A3 Q pointer | img4 | `0x0078b024` | `00789e2c` | `0061c2ec` | Q labels ptr |
| 247 | activation | A3 Q pointer | img4 | `0x0078b020` | `00782348` | `0061c1fc` | Q values ptr |
| 248 | activation | A4 site | img5 | `0x00d50a48` | `e1a00004` | `ebe32e57` | case 0xfe first insn -> bl 0x61c3ac |

Group totals: S1 81 (55 changed, 26 validate-only), S2 13, S3 4, A1 6, A2 48 (43 changed, 5 validate-only), A3 2, A4 1. That is 98 setup and 57 activation entries, 124 changed and 31 validate-only. They touch 26 distinct 32-byte lines, none shared with the bank's 72 lines, so the transaction covers 98 lines (computed). No D3 address equals a bank address, lies in the allocation, or touches the callback slot `0x015c22ac` or the bootstrap window `[0x015c22b4,0x015c22d4)`.

The middle word of each film row (`0x77ac4c`, `0x77ac64`, value `0x7ffffffe`) is unchanged in RC0 and the candidate and is not published. The 14 availability cells are not published (section 1, item 2).

### 3.3 Bank flash-boot carrier (not pushed)

| Address or range | Words | Old (RC0) | New (f6502602) | Role |
| --- | ---: | --- | --- | --- |
| `0x0069d2e0` | 1 | `e12fff33` (`blx r3`) | `ea0397ad` (`b 0x0078319c`) | boot hook into `rb_boot_entry`. The stock function `0x0069d2b4` looks up loader descriptor 5 and calls its `+0xc` entry. Its one caller is `0x0073cdc8` (byte-read). |
| `0x0072b130` | 1 | `e34030bf` | `e34030be` | heap ceiling `0x00bf1a00` to `0x00be1a00` (feasibility note section 3) |
| `[0x011ad800,0x011b7940)` | 0x2850 | erased fill outside the plain extents | `source-package.bin` `d5720aa4…28b3` | runtime image and publication table that the flash boot copies |

The four stub runs are `rb_boot_sum`, `rb_boot_entry`, `rb_boot_publish`/`rb_boot_fail`/`rb_boot_done` and `rb_boot_check` (`boot/boot.disasm`). The old words are the stock arrays that RC0 or D3 leaves unreferenced (byte-read: in RC0 only `0x0078b020`/`0x0078b024` name `0x00782348`/`0x00789e2c`, nothing names `0x0078319c`/`0x0078d4f8`, and in the candidate nothing names any of the four).

| Run | Words | Old content | Old words (RC0) | New words (f6502602) |
| --- | ---: | --- | --- | --- |
| `0x00782348` | 12 | stock Q values array (unreferenced after D3 moves `0x78b020`) | `00000000 00000000 00000003 00000000 00000001 00000000 00000007 00000000 0000000c 00000000 7ffffffe 00000000` | `e2848020 e1a0900a e4980004 e0866000 e1a063e6 e4890004 e2555004 1afffff9 e594000c e1560000 1a001ebd ea001eac` |
| `0x0078319c` | 12 | stock shared list values (unreferenced since RC0) | `00000000 00000000 00000003 00000000 00000001 00000000 00000007 00000000 0000000c 00000000 7ffffffe 00000000` | `e92d5fff e3a00047 e30013e8 ebfe866e e3500000 1a001b2d e3a00047 ebfe8690 e30d4800 e340411a ea0028cb e320f000` |
| `0x00789e2c` | 24 | stock Q labels array (unreferenced after D3 moves `0x78b024`) | `ff00077b 00000000 00000000 00000000 ff00077c 00000000 00000000 00000000 ff000773 00000000 00000000 00000000 ff000777 00000000 00000000 00000000 ff00077a 00000000 00000000 00000000 ffffffff ffffffff ffffffff ffffffff` | `e28a7080 e28a9d12 e28ab080 e1570009 2a00000a e4970008 e3500000 0a288f94 e1a0800a e158000b 2a000004 e8b81008 e0403003 e153000c 2afffff9 eafffff2 e30f0ffc e34000be e3a01002 e5801000 e8bd5fff e30de2e4 e340e069 e12fff13` |
| `0x0078d4f8` | 24 | stock shared list labels (unreferenced since RC0) | `0000048a 00000489 0000048c 0000048b 0000048e 0000048d 00000490 0000048f 00000492 00000491 00000494 00000493 0000049f 0000049e 000004a2 000004a0 000004b0 000004af 000004b2 000004b1 ffffffff ffffffff ffffffff ffffffff` | `e3070940 e340000b e3041834 e3401091 e5911000 e1510000 3afff255 e5940000 e3041a46 e3441252 e1500001 1afff250 e5945008 e3150003 1afff24d e30a0120 e1550000 3afff24a e3550ce6 2afff248 e3a06000 e301aa00 e340a0be eaffd37b` |

### 3.4 Container-only and boot-loader-consumed fields

| Field | RC0 | f6502602 | Consumer |
| --- | --- | --- | --- |
| segment entry 4 load word, media block 10 and its RAM table-block copy `0x0061c0d4` | `0x0061c340` | `0x0061c1fc` | boot loader, where image 4 is decoded |
| segment entry 4 size word, media and RAM `0x0061c0d8` | `0x002828e8` | `0x00282a2c` | boot loader |
| record 4 header word 0, decoded size (media `0x160000`) | `0x002828c0` | `0x00282a04` | decompressor |
| record 4 header word 1, total length | `0x00146602` | `0x00146739` | decompressor |
| record 4 header words 2-4 | `0x52`, `0x4000`, `4` | unchanged | |
| record 4 stream, blocks 11-21 | RC0 stream | candidate stream | decompressor, decoding to `id4-final.bin` |
| record 5 header words 0-4 (media `0x2c0000`) | `0x004f1e60 0x002c66d1 0xb2 0x4000 4` | unchanged | |
| record 5 stream, block 26 | stock stream | candidate stream | decompressor, decoding to `id5-final.bin` |
| trailer image load/size words, block 293 | RC0 values | candidate values (marker `0xaaaa` unchanged) | updater and boot loader |
| main checksum word | `0x8f0a6b52` | `0x8ee980a9` | updater validation |

The RAM table-block words were byte-read from both DATs' table blocks, the only two differing words in that block. Their addresses are as bound by the hardware readback E-FILMSIM-READBACK-150, which saw `0x0061c0d4`/`0x0061c0d5`/`0x0061c0d8` move at the RC0 install. The header and checksum values are byte-read from the media or cited from E-RECIPE-RC-FINAL-IMAGE-REVIEW-150 and [recipe-patch-d3](recipe-patch-d3.md) section 2.

## 4. Classification, bound from the bytes

### 4.1 S1, prefix band `[0x0061c1fc,0x0061c340)`: patchable, read after boot only through pointers that the push itself moves

- In RC0 the band lies below the image-4 load. Nothing in RC0 image 4, image 5, image 2 or the plain image holds a literal or a `movw`/`movt` pair naming any address in `[0x0061c1fc,0x0061c400)` other than the six list pointer words that name `0x0061c340`/`0x0061c380` (byte-read scan in this pass). Before A1/A3 the band is therefore unreachable, and storing into it has no observable effect (static-inference).
- Old word 0. The stock-boot probes saw the window `[0x61c1fc,0x61c400)` all zero (hardware-observed, stock firmware, 2026-10-07 sessions, [filmsim-prefix-widening](filmsim-prefix-widening.md) section 1). The emulated stock load path writes nothing there (E-LOAD-WINDOW-TRACE-150). The post-RC0 maintenance-boot readback found that `[0x61c000,0x81c000)` differs from the pre-install dump only at manifest bytes and the two segment-table words (E-FILMSIM-READBACK-150), so the band was unchanged by RC0. The value of the band in an **ordinary** RC0 boot has not been read on the body (open, section 8). Every band word is a publication, so a non-zero word refuses the install with no store.
- The extent descriptor ends at `0x0061c1fb` (byte-read: the entry at `0x0061c1e0` is `{3, 0x011e0000, 0x0026a5e0, 0, 0, 0x47, -1}`), so the band starts above it.

### 4.2 S2, image-2 `0x83d` record: patchable data, unread while the bank's label publications are live, carried for byte equality

- Byte-read: the stock decode holds `?` followed by the debug string `PB_IMG_SEARCH_WGPS_BAR_F` in the 0xe1-offset form, and the candidate holds `WARM STANDARD` in UTF-16LE with zero fill. The layout halfwords at `0x0037b7fe` are unchanged.
- The bank's setup entries 0-35 raise the special-class bound from `0x83d` to `0x849` and move the record base into the bank's name table (byte-read, `image/publication.json`). That table's record 1, `0x83d`, already reads `WARM STANDARD` with the same layout halfwords, both in the flashed `name-records.bin` and in the RAM runtime `operator-linked-2/runtime.bin` at offset `0xaa66` (= name base `0xaa24` + 66, byte-read). In the flashed image with the bank published, and in the RAM push, the shooting-list name of recipe A is therefore read from the bank table (static-inference). The image-2 buffer is read in the flashed image only on the bank-refusal fallback, a state that one RAM transaction cannot produce.
- It is carried anyway, so that the RAM-push state equals the flashed decode at every D3-touched byte. The cost is 13 setup words. No stock surface names id `0x83d` (inherited, E-LABEL-RESOLVE-150 census), so it can be stored at any point in the setup phase.
- Old words are the stock image-2 decode (image 2 is loaded uncompressed: `resources2.bin` at `0x00130000`, hash `236d12a1…`, `BuildFullRecoveryCard.hs`). Image-2 RAM has not been read on the body (open).

### 4.3 S3, film conversion rows 2 and 4: patchable, read on every conversion

- All nine recipe-relevant conversion tables are reached through `*(0x0081b09c + 4*property)` at the time of each call. The `0xfe` pointer is `0x0081b494` → `0x0077ac30`, and `FUN_00631a00` loads it at `0x00631a14..0x00631a1c` on every lookup (byte-read). The only reference to `0x0077ac30` in images 4/5/plain is that pointer word (byte-read scan). No copy exists.
- The not-found cell `0x0089edc8` is written and read only inside `0x00631a00..0x00631b70` as a per-lookup status (byte-read). It holds no state across lookups.
- Rows 2 and 4 are named by no values array in RC0 (inherited, [recipe-patch-d3](recipe-patch-d3.md) section 1, re-checked by the compiler). Their RC0 col0 codes `2` and `8` are unreachable from stock surfaces. Rewriting them before A1 therefore changes no reachable conversion (static-inference). There is one exception, a stored code of 2, 8, `0xd` or `0xe` already present in a record at boot (section 4.7).
- Old words are the RC0 decode, hardware-observed equal on the body in a maintenance boot (E-FILMSIM-READBACK-150 covers `[0x61c340,0x81c000)`).

### 4.4 A1 and A3, node pointer words: patchable, read when a list or the Q cell is opened or drawn

- Byte-read: the six RC0 list words and the two Q words are the only references to their targets (section 4.1 scan). The flashed D3 changes no row-count or cache field in the nodes, only these words. So in the flashed image the nine rows come from the arrays at draw and open time, and a RAM change of the same words has the same effect if no boot-time copy of the pointer values exists. A copy of a pointer *value* into another structure cannot be excluded by a literal scan, so this is **open (emulator)**. The FILMSIM-EMUL-01 cases walk the values array at draw time (`FUN_00d52684` case `0xfe` → `FUN_0061d534`, inherited E-FILMSIM-EMUL-150), which supports the read-at-use reading for the list.
- Old words are the RC0 decode (hardware-observed as above).
- Order: per node, the labels pointer first and then the values pointer, the flashed D3 manifest's own rule. In every intermediate state the labels array is at least as long as the values array: the new 9+terminator labels with the old 7-row values, or the stock 5-row Q values with the new Q labels. The reverse rollback keeps the same property.

### 4.5 A2, prefix overlap `[0x0061c340,0x0061c400)`: patchable only after A1

- In RC0 these 48 words are the live RC0 values array (`0x0061c340`, 16 words) and labels array (`0x0061c380`, 32 words). The six list words point at them until A1. After A1 nothing names them (section 4.1 scan). They then receive the Q-label tail, both row tables, the property table, the 76-byte hook and the zero fill.
- Old words are the RC0 decode, hardware-observed equal (E-FILMSIM-READBACK-150). Five words are equal in RC0 and the candidate and are carried validate-only so that the whole window is checked as one unit.
- `[0x0061c3ac,0x0061c3f8)` becomes code. The hook is fetched only through A4. I-cache invalidation on both cores is already part of the protocol (`synchronize_current_core` and `peer_cache_action` end with `ICIALLU; DSB; ISB`). D-cache lines are cleaned on CPU0 after the stores and cleaned and invalidated on CPU1 before PARKED and after GO (byte-read, `menu/ram_push_installer.c`).
- Executability: same address and same MPU region as the flashed hook. Region 0 is reprogrammed to 1 GiB, access `0x303`, by the payload boot record, and image 4 sets only regions 5-7 at heap bases (inherited, E-XM1-MPU-REGIONS-150). Stores into that region are hardware-observed working by the bank install itself (its image-4 publications such as `0x0061eac4`). Instruction fetch from the prefix is not observed. The RAM push is the first exercise of it, which is a reason to keep D3 at its flashed address rather than relinking it into the allocation.

### 4.6 A4, image-5 site `0x00d50a48`: patchable code word, the D3 activation

- Byte-read: stock `e1a00004` (`mov r0, r4`) is the first instruction of `case 0xfe` of `FUN_00d4fc74`. The candidate has `ebe32e57` = `bl 0x0061c3ac`. The site is reached only by the property-`0xfe` menu commit (inherited, [recipe-patch-d3](recipe-patch-d3.md) section 5.1). It is the last D3 store, after the hook (A2), the tables (A2), the arrays (S1/A2) and the rows (S3).
- A task preempted in the arm before the site is safe after the swap, because the hook is complete before A4. During rollback A4 is restored first. The hook region is overwritten afterwards, so a task suspended *inside* the hook (in a nested `FUN_00d4fc74` call, LR `0x0061c3e4`) would return into RC0 label data. The quiescent-shooting-screen precondition excludes this: the commit runs to completion in the UI task before that task returns to its queue. This is the same premise the bank's own code relies on at unload (static-inference, **open (emulator)** for whether `FUN_00d4fc74` can block inside a nested commit).
- Old word: stock image 5. The body's image-5 RAM has been read only at the bank's sites (E-RAM-PUSH-PREFLIGHT-150), so `0x00d50a48` is **open (body read)**. The host preflight reads it before any write.

### 4.7 Boot-consumed state

- **Stored film code.** Any state the firmware derives at boot from the stored film code of the current record goes through `0x0077ac30`, the only conversion table for property `0xfe` (byte-read, section 4.3). The boot-time consumer itself, for example a write of the live cell `0x009568b8` or the develop state, is not bound (static-inference). For every stock code the conversion is identical in RC0 and in D3, because D3 changes only rows 2 and 4. Codes `0xd`/`0xe` convert in D3 but not in RC0: `FUN_00631a00` returns 0 and sets the not-found cell. Codes 2/8 convert in RC0 but not in D3. A RAM-pushed camera matches the flashed one only if, at the boot before the push, neither the ordinary block nor the C record (slot 0) holds 2, 8, `0xd` or `0xe` (static-inference). A previous RAM session can leave `0xd`/`0xe` behind: selecting a recipe writes the ordinary block, which persists at orderly power-off, and `CUSTOM SET` persists the C record. What RC0 then does with `0xd`/`0xe` at boot (keeps the code, rewrites it, or shows PROVIA for recipe B's Velvia base) is **open (emulator)**. Section 5.7 states the operating rule that follows.
- **Segment entry 4 in the RAM table block** (`0x0061c0d4`/`0x0061c0d8`): consumed by the boot loader. The lookup `0x00657114` is called with constant ids 2, 3, 5, 6 and 7 only (byte-read call sites `0x0069d2c0`, `0x006a96d0`, `0x006a9854`, `0x006aa214`, `0x006aa5f4`, `0x006aa708`). No image-4/5/plain word or `movw`/`movt` pair names `0x002828e8`, `0x002828c0`, `0x0061c1fc` or `0x00282a2c`. `0x0061c340` is named only by the six list pointer words as the RC0 values array (byte-read scan). These words are left at RC0 values. They describe the flash that is actually installed, and changing them would make the RAM table disagree with the container on flash. Whether any runtime path looks up id 4 through a non-constant argument (`0x006571bc`) is **open**. It does not affect film-simulation behaviour.
- **Heap ceiling and bank boot.** The flashed image has 64 KiB less heap than the stock or RAM-push boot. The RAM push instead takes 0x1b6b8 bytes from the stock heap. This difference is accepted by the bank design (feasibility note section 3) and is not D3.
- The prefix itself is not consumed at boot. Nothing names it before the pointer words move (section 4.1).

### 4.8 Impossible in RAM

The container fields of section 3.4, persistence across power-off, and the next-boot behaviour of the flashed image (bootstrap gates, refusal fallback, source fetch). None of them is part of the running image after boot. They stay covered by E-RECIPE-RC-FINAL-IMAGE-REVIEW-150 and the flash-path emulator work, not by the RAM push.

## 5. Publication model extension

### 5.1 Unchanged

Allocation through the 32-byte bootstrap, upload while unreachable, the host's rebuild-and-compare authentication at the retained base, the 0x450006 callback install path, `prepare_peer`, the two-core rendezvous (async CPU1 park through `0x0072503c`, channel 0x58, command 0x6800), IRQ masking on both cores, validate-all-before-COMMIT, single aligned `str` per entry, CPU0 line maintenance plus `ICIALLU`, CPU1 clean+invalidate before PARKED and after GO, the -7/-8 status semantics, rollback by exact reverse with old-or-new validation, and the mandatory power-off on uncertainty. The installer functions are already table-driven over `RAM_PUSH_PUBLICATION_COUNT` (byte-read: `validate_old_words`, `store_new_words`, `restore_old_words`, `synchronize_current_core`, `peer_cache_action`, `read_peer_report`, `validate_layout`).

### 5.2 Count and phases

| | Bank (reviewed) | D3 (new) | Total |
| --- | ---: | ---: | ---: |
| setup | 69 | 98 (S1 81, S2 13, S3 4) | 167 |
| activation | 25 | 57 (A1 6, A2 48, A3 2, A4 1) | 82 |
| all | 94 | 155 | **249** |

### 5.3 Order

Positions 0..248 in the table, rollback in exact reverse:

1. Positions 0-68: the bank's 69 setup entries in the reviewed order `0-35, 55-68, 85, 36-53`.
2. Positions 69-166: D3 setup, S1 in ascending address, then S2 ascending, then S3 (row 2 col0, row 2 col2, row 4 col0, row 4 col2).
3. Positions 167-223: D3 activation, A1 (`0x007823c8`, `0x007823c4`, `0x00785d98`, `0x00785d94`, `0x0077d1a8`, `0x0077d1a4`), then A2 ascending, then A3 (`0x0078b024`, `0x0078b020`), then A4 `0x00d50a48`.
4. Positions 224-248: the bank's 25 activation entries in the reviewed order `75,76,72,73,74,69,70,71,77,78,79,80,86..93,81..84,54`. Logical 54, the parent-values root, stays the final store and the first restored.

The order is dependency-safe even without the masked-window premise. Nothing reachable points into storage being overwritten: S1/S2/S3 are unreachable until A1/A3, A2 is unreferenced once A1 lands, A3 lands after A2 completes the Q labels, and A4 lands after the hook and tables. The bank can now apply recipe film codes from its slots (it stores the film field raw, `menu/recipe_bank_fields.inc`), so D3's rows and site precede all bank activations. The bank does not depend on D3 in any other way, and D3 does not depend on the bank (the `0x83d` name renders `WARM STANDARD` through either the image-2 record or the bank table).

### 5.4 Cache lines and cores

The 155 D3 entries add 26 distinct 32-byte lines: `0x61c1e0`-`0x61c3e0` (17), `0x37b7c0`, `0x37b7e0`, `0x77ac40`, `0x77ac60`, `0x77d1a0`, `0x7823c0`, `0x785d80`, `0x78b020` and `0xd50a40`. None is shared with the bank's 72, for 98 lines in total. Both cores maintain every line: neither the UI task that runs the hook nor the menu readers of the arrays has a bound core, so every D3 manifest entry carries `"core": "unknown"`. The peer report's `lines` field stays the entry count (249). `line_seen_before` is quadratic. At 249 entries each pass is at most 30,876 address compares, run once on CPU0 and twice on CPU1 inside the masked window. The emulator measurement of `cpu0_cycles` and `cpu1_cycles` with 249 entries decides whether a host-sorted distinct-line list is needed (section 8).

The CPU1 park wait is bounded at `0x80000` iterations while CPU0 validates. The body measured 888 park and 907 CPU1 spins at 94 entries (E-RAM-PUSH-INSTALL-150). Validation grows to 249 loads, which keeps the wait two orders of magnitude inside the bound (static-inference). The emulator confirms it.

### 5.5 Allocation

- Publication table: 249 × 16 = 0xf90 bytes (was 0x5e0).
- Installer extent = `align32(T + 0xf90) + 0x80 rendezvous + 0x80 mailbox`, where T is the installer text and rodata. T = 0xa84 in the reviewed runtime (byte-read: the table starts at runtime offset `0xb844` = installer start `0xaabb20` + 0xa84 for base `0xaa0d60`). The new extent is therefore `0x1a20 + 0x100 = 0x1b20` if T is unchanged. The count immediates 249, 167 and 82 are all A32-encodable, so no literal-pool growth is expected. The builder's existing extent check catches any change.
- Fixed allocation = worst bank end `0x19b98` + extent = **0x1b6b8** (was 0x1ad18, +0x9a0 bytes).
- Upload: 453 function-3 chunks of 248 bytes (was 443). Preflight and unload old-word reads: 249 each (was 94). The journal caps are derived from the transfer plan (`86079af`) and follow the counts.

### 5.6 Old-word validation per class

| Group | Old word source | Body evidence today | Validation |
| --- | --- | --- | --- |
| S1 band | 0 | stock ordinary boots zero (hardware-observed), RC0 maintenance boot unchanged from stock (hardware-observed), RC0 ordinary boot unread | host preflight read, installer before COMMIT |
| S2 name | stock image 2 decode | unread | host preflight read, installer |
| S3 rows, A1/A3 pointers, A2 overlap | RC0 image-4 decode | maintenance-boot readback equal (E-FILMSIM-READBACK-150) | host preflight read, installer |
| A4 site | stock image 5 decode | unread at this address | host preflight read, installer |

On any mismatch the installer returns -4 with zero stores. Rollback accepts old-or-new per word and restores all 249 in reverse.

### 5.7 Operating rule that follows from section 4.7

Before the push, the stored film simulation must be a stock code. Before the orderly power-off that ends a session, the owner selects a stock film simulation if a recipe row is selected, and does not run `CUSTOM SET` with a recipe selected. The emulator case of section 8 decides whether the rule can be relaxed. The session script should carry it until then.

## 6. Residual differences from the flashed camera

1. Bank runtime base: relinked into a heap allocation instead of `0x00be1a00` (reviewed and accepted).
2. Heap: no 64 KiB ceiling reduction. Instead 0x1b6b8 bytes are allocated from the stock heap.
3. Freed stock-array runs hold stock data instead of the boot stub, and `0x0069d2e0` stays `blx r3`. Nothing reachable reads either after boot (section 3.3).
4. The RAM table block describes RC0 (section 4.7).
5. A stored film code of 2, 8, `0xd` or `0xe` at the preceding boot derives boot state through RC0 rows (section 4.7).

Everything the running image reads for film simulation, recipes, the Q cell and the bank is byte-equal to the flashed decode at the same addresses, apart from the bank runtime's base-dependent words.

## 7. Source changes needed (not implemented)

RecallBanner and StockStyleEdit are editing `menu/recipe_bank_*` and `host/firmware/BuildRecipeBank.hs`. The implementation of item 3 must be coordinated with them.

1. `menu/ram_push_abi.h`: `RAM_PUSH_PUBLICATION_COUNT 249u`, `RAM_PUSH_SETUP_COUNT 167u`, `RAM_PUSH_ACTIVATION_COUNT 82u`. `RAM_PUSH_ABI_VERSION` goes to 2, because the host's mailbox checks and the table layout change.
2. `menu/recipe_bank_ram.ld`: the assert becomes `249 * 16`. `menu/ram_push_installer.c` needs no logic change.
3. `host/firmware/BuildRecipeBank.hs` (`emitRamPush`):
   - read and pin the D3 decodes (`d3/id4-patched.bin` `6e5df0b7…`, `d3/id5-patched.bin` `5626062e…`, `d3/id2-patched.bin` `1b8a281c…`) and the RC0 image 2 (`filmsim/id2-00130000.bin` `236d12a1…`);
   - derive the D3 entries as: the full window `[0x0061c1fc,0x0061c400)` (old 0 below `0x0061c340`, RC0 decode above), plus every word where the D3 image-4 decode differs from the RC0 image-4 decode outside the window, plus the image-5 diff, plus the aligned image-2 diff words. Refuse unless the derived address set equals a pinned list (the 155 of section 3.2), is disjoint from the bank's 94 and from the allocation, and the image-4 diff outside the window is exactly the 4 row words and 8 pointer words;
   - extend `installedImages` with RC0 image 2 at `0x00130000`, with the band as a declared zero region;
   - emit the order of section 5.3, the phases 167/82, logical indexes 94..248 for D3, and `"core": "unknown"` for D3 entries;
   - add the three new inputs (D3 image 4, D3 image 5, RC0 image 2; `d3/id2-patched.bin` is already pinned as `ramNameImagePath`) to `source_identity`, giving 27 entries inside the host's 32-entry bound;
   - leave the legacy flash metadata block (allowlist plus 94 `(address,new)` pairs at runtime offset 0) unchanged.
4. `host/firmware/RecipeBankRamLayout.hs`: `runtimeInstallerExtent 0x1b20`, `runtimeAllocationSize 0x1b6b8` (both as measured from the rebuilt link), and `safePublicationOrder` extended to the 249-position order (or a D3 order list beside it).
5. `host/firmware/build-recipe-bank-ram.sh`: the base bound uses the new size, and the D3 and RC0-image-2 paths are passed to the builder (new env vars, Bazel data deps in `host/firmware/BUILD.bazel` and `transport/BUILD.bazel`).
6. `transport/FujiAF/RamPush.hs`: `reviewedAllocationSize 0x1b6b8`; manifest parse to 249 entries with indexes 0..248 and phases 167 setup then 82 activation; `parsePublication` index bound 248; `publicationReadExchanges` 249; mailbox checks `abi == 2`, `validated == 249`, `setup_stored == 167`, `activation_stored == 82`, `restored == 249`, peer `lines == 249`; and the note strings.
7. `transport/Main.hs` usage text, `transport/FujiAF/Rehearsal.hs` counts, `transport/RamPushTest.hs` fixture (249 publications, new size, reverse-order and exact-destination checks), `emulation/Xm1RamPushE2E.hs` `allocationBytes` and its unload string.
8. Emulator cases (`emulation/RecipeBankUiCases.hs` or a sibling) for the open facts of section 8.
9. Docs: `transport/README.md`, `emulation/README.md`, the feasibility note section 5 (superseded by this report), `reports/ram-push-session-script.md` (owner checks: FILM SIMULATION shows nine entries, the Q cell offers nine, WARM STANDARD applies ISO 400 and DR 200 %, plus the rule of section 5.7).

Checks to run after implementation: `//transport:ram-push-tests`, `//transport:cli-rejections`, `//host/firmware:build-recipe-bank-ram-tests`, a relink at the worst bases `c20000`/`c21f88` with the new size, and the `--ram-push-e2e` emulator run under the shared lock.

## 8. Open facts

Emulator (full system, exact RC0 base, `--ram-push-e2e` extended):

1. With 249 entries the transaction completes with status 0, validated 249, peer lines 249. Record `cpu0_cycles`, `cpu1_cycles`, `cpu0_park_spins` and `cpu1_spins` for the masked window. Then unload restores all 249 and the RC0 bytes compare equal.
2. After install, without restarting: SHOOTING MENU and Fn FILM SIMULATION list nine rows, and the Q cell offers nine rows with the stock glyphs. This settles that no boot-time copy of the node pointers exists (section 4.4). After unload, seven rows and Q five.
3. Selecting WARM STANDARD from the list and from the Q cell executes `0x0061c3ac` and applies ISO 400 and DR 200 % (the hook's nested commits). Recipe B applies its row. Then, in the same run, unload with the shooting screen idle succeeds.
4. Boot RC0 with the ordinary-block film field set to `0xd`, then `0xe`, and separately the C record holding `0xe`. Record the stored code and the live cell `0x009568b8` after boot, before and after a D3 push, and compare with the same boot of f6502602 (section 4.7).
5. Whether `FUN_00d4fc74` can block inside a nested recipe commit (task switch with LR `0x0061c3e4` on a stack).

Body (read-only first, then the owner's go):

6. Ordinary-boot RC0 values of the 81 band words, the 13 image-2 words and `0x00d50a48`, by a read-only `read-ram` plan (windows `[0x0061c1fc,0x0061c2f4)`, `[0x0061c2f4,0x0061c3ec)`, `[0x0061c3ec,0x0061c400)`, `[0x0037b7c4,0x0037b7f8)`, `[0x00d50a48,0x00d50a4c)`), like E-RAM-PUSH-PREFLIGHT-150. Then the allocation of 0x1b6b8 bytes returns an aligned base inside the stock heap.
7. Instruction fetch from the prefix on silicon: the owner's selection of WARM STANDARD after the push, with ISO and DR observed on screen.
8. The ordinary-block offset of property `0xfe`, so that the host preflight can read the stored film code (the slot-record payload offset is `0x24` bits 0-3, [custom-settings.fields.json](custom-settings.fields.json). The ordinary block is at `0x0096cc64 + 0x1c2*bank` with per-property offsets, [custom-settings](custom-settings.md) section 3). The host preflight would then refuse a push with a stored code in `{2, 8, 0xd, 0xe}` instead of relying on the owner rule of section 5.7.

## 9. Implementation and emulated results (2026-10-17)

The design is implemented as written: `RAM_PUSH_ABI_VERSION` 2, 249 publications (167 setup, 82 activation), installer extent `0x1b20`, allocation `0x1b6b8`. `d3RamPublications` in `host/firmware/BuildRecipeBank.hs` derives the 155 D3 entries from the pinned D3 decodes against RC0. It refuses unless the image-4 body diff is exactly the 4 film-row words plus the 8 pointer words, image 5 differs only at `0x00d50a48`, and image 2 differs only in `[0x0037b7c4,0x0037b7f8)`. A relink at `0x00c20000` reproduces the section 3.2 table entry for entry; the entry offsets stay at installer start + `0x600`/`0x838`/`0x864`. The publication contract digest is `d5ff9d61e9f7c2a1daa355761ff2b5035aa4bebc92e99f040484d531b937a0cd`.

Emulated (`private-data/analysis/x-m1-1.50/ram-push/emu/e2e/run-d3-249-3`, full system, RC0 base, exit 0):

- INSTALL and ROLLBACK both return status 0 with 249 validated and peer lines 249; ROLLBACK restored 249. CPU0 park/ack spins were 27624/79 and CPU1 spins 36286, against the 524288 bound. The bank's checked trajectories all match.
- The FILM SIMULATION list offers nine rows and draws WARM STANDARD. The hook at `0x0061c3ac` runs on each highlighted row, because the list commits the highlighted row as a live preview. With WARM STANDARD selected, DR is 200 % and ISO is 400, and the NR, highlight, shadow and sharpness cells hold recipe A's codes. Reopening the list puts the cursor on PROVIA. After rollback the list has seven rows again.
- The Q film cell was not reached by the harness key path, so Q exposure stays open. The stored-code-at-boot question (section 4.7) stays open too, because the emulator's restart discards NAND changes.

The read-only body plan for section 8 item 6 is `private-data/read-plans/ram-push-d3-prepush.json` (rehearsed, four windows).

### 9.1 Publication 250: the film current-row getter

QAndD3Rows' D3 revision adds one image-5 word: `0x00d5a088`, `1a000009` (`bne 0xd5a0b4` in `FUN_00d588b8` case `0xfe`) becomes `e320f000` (`nop`). With the NOP, the current-row getter maps the stored film code through col0, so reopening the list highlights the recipe row and the Q cell steps from it. The D3 image-5 decode is `private-data/analysis/x-m1-1.50/recipes/d3-q-rows-20261017/d3-emit/id5-patched.bin` (`9050aae0d456aae40b90630a3aea16cab57ecafc66dce28e9cfa77eb1cea7d76`). The RAM push carries the word as D3 activation logical 249, after the site word. Totals: 250 publications (167 setup, 83 activation), table 0xfa0 bytes, installer extent `0x1b40`, allocation `0x1b6d8`. The word is read only when the menu resolves the film row, so it is patchable after boot in the same way as the site.
