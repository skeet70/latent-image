# Sources and starting references

## 17. Sources and starting references

References retain source pins, inspection dates and evidence scope. Reproduce pinned sources and record local hashes/audits before relying on camera effects. Current scoped conclusions are recorded in [the evidence ledger](../docs/evidence.jsonl), [transport audit](../reports/source-and-transport-audit.md) and [firmware findings](../reports/firmware-extraction.md); other references remain research leads.

<a id="s01"></a>

- **[S01] Fujihack:** [repository](https://github.com/fujihack/fujihack), [wiki](https://wiki.fujihack.org/), [supported-family caveat](https://wiki.fujihack.org/supported/), [existing Unicorn material](https://github.com/fujihack/fujihack/tree/master/etc/unicorn).

<a id="s02"></a>

- **[S02] `fffw` repository:** [README at the inspected commit](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/README.md).

<a id="s03"></a>

- **[S03] Command implementations:** [`ff80_cmd/cmd.py`](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/ff80/ff80_cmd/cmd.py), [`ff80.py`](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/ff80/ff80.py). Source-inspected at the pinned revision.

<a id="s04"></a>

- **[S04] Service and transport code:** [`ffjlib.py`](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/ff80/ffjlib.py), [`fftlib.py`](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/ff80/fftlib.py). Source-inspected at the pinned revision.

<a id="s05"></a>

- **[S05] `ffun` availability:** [subdirectory README](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/ffun/README.md). Read locally; the public tree is incomplete, so assess component availability instead of trusting the top-level “unpublished” label.

<a id="s06"></a>

- **[S06] Boot examples:** [`xe2-start.S`](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/boot/xe2-start.S), [`xe2-led0-main.c`](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/boot/xe2-led0-main.c). Read locally; these contain model-specific register writes.

<a id="s07"></a>

- **[S07] Maintainer research notes:** [Route 0xff80](https://github.com/tiredboffin/fffw/wiki/Route-0xff80), [EXR II CPU](https://github.com/tiredboffin/fffw/wiki/EXR-II-CPU). These describe related-camera findings, not a complete validated X-M1 port.

<a id="s08"></a>

- **[S08] Official X-M1 firmware:** [Fujifilm support page](https://www.fujifilm-x.com/en-us/support/download/firmware/cameras/x-m1/), read 2026-10-25. The vendor lists 1.50 and reports still-image AF accuracy changes in 1.01, lens-specific AF/compatibility changes in 1.20, 1.30 and 1.50, and lens focus-limiter support in 1.40. These motivate source-bound same-model comparisons and lens-parameter research; they do not establish a defect or an improvement for the current Viltrox lens. No firmware download, installation or acceptance of new license terms is part of this source inspection.

<a id="s09"></a>

- **[S09] devenv:** [basics and command execution](https://devenv.sh/basics/), [options reference](https://devenv.sh/reference/options/). Confirm the installed version before adopting newer options.

<a id="s10"></a>

- **[S10] Emulation and USB:** [Unicorn repository](https://github.com/unicorn-engine/unicorn), [Unicorn API tutorial](https://www.unicorn-engine.org/docs/tutorial.html), [libusb documentation](https://github.com/libusb/libusb/wiki), [Ghidra](https://github.com/NationalSecurityAgency/ghidra).

<a id="s11"></a>

- **[S11] Learned image-based AF:** [Deep Learning for Camera Autofocus, author code](https://github.com/ChengyuWang1007/Deep-Learning-for-Camera-Autofocus), [Intelligent Autofocus preprint](https://arxiv.org/abs/2002.12389), [IEEE article summary](https://signalprocessingsociety.org/publications-resources/ieee-transactions-computational-imaging/2022/01/deep-learning-camera-autofocus). Inspection on 2026-10-25 found estimator/discriminator code, calibration code, an RL-agent directory and pretrained weights. Reuse permission was not established; public files alone are not a license. First reproduce a bounded ordinary-image experiment and quantify lens/domain calibration and target cost.

<a id="s12"></a>

- **[S12] Learning to Autofocus (CVPR 2020):** [project, paper links and dataset](https://learntoautofocus-google.github.io/), read 2026-10-25. The project provides 510 real-world focal stacks and compares single-image, two-step and full-stack prediction with conventional and dual-pixel inputs. Only the conventional-image branch is an X-M1 candidate absent independent hardware evidence. The project describes MobileNetV2 with ordinal regression and offers code contact rather than a turnkey reproduction. Establish data rights and a bounded subset before large downloads; prediction error is not end-to-end AF latency.

<a id="s13"></a>

- **[S13] Practical contrast-scan reference:** [Raspberry Pi libcamera AF controller at revision `957f0f4bd06a163466ff1f3adda66f64fb66cd64`](https://github.com/raspberrypi/libcamera/blob/957f0f4bd06a163466ff1f3adda66f64fb66cd64/src/ipa/rpi/controller/rpi/af.cpp), source and file-revision history inspected 2026-10-25. The file declares BSD-2-Clause. Reference for coarse/fine search, peak interpolation, confidence and settling policy. Account for its hybrid-AF assumptions and sensor tuning; none of its lens units, PDAF inputs or physical timing transfers to X-M1 by analogy.

<a id="s14"></a>

- **[S14] X-T3 case study:** [author's Reddit post](https://www.reddit.com/r/fujifilm/comments/1wda2x6/xt3_running_classic_negative_eterna_bleach_bypass/). The user supplied the detailed account. Its test structure informs this plan; claimed results and recovery are not independently reproduced here.

<a id="s15"></a>

- **[S15] X-M1 service manual:** Fujifilm X-M1 Service Manual, Ref.No ZM01086-107, revised 2014-08-14. Owner-purchased PDF, sha256 `7ebe30b1feb41f7fc20488ad674080de69cc5e38fc4e29b4647a04b08dd6ac9b`, kept in the owner workspace and not committed (confidential). Documents jig/service mode: internal codename **LX30A**, service firmware image `LX30ADSC***.bin`, SD-BOOT `ffboot.bin`/`ffsys.bin`/`ffuser.bin` (ZJ02217), PC adjustment soft ZJ02216 (HyperFFW) with jig driver ZJ00684-100 or ZJ02197-100, jig-entry key combos, the identity readback shape and the [F8]/[F12] firmware-download and End-Setting flows. Transcribed facts and the owner's entry/exit observation are in [the evidence ledger](../docs/evidence.jsonl) as E-SERVICE-MANUAL and E-JIG-OBSERVED.

The hash-verified S15 source binds all three SD-BOOT files to the extracted ZJ02217 folder on printed pages 3-9–3-10 and names `https://cs-support-eid.fujifilm.co.jp` as the download host (inspection 2026-10-12). The X-Pro1's different package split is not an X-M1 fact. [The recovery source findings](../reports/recovery-hardware-routes.md#7-external-source-discriminators) retain acquisition leads and their limits.

<a id="s16"></a>

- **[S16] `fffw` wiki platform notes:** [CPU/OS/format history table](https://github.com/tiredboffin/fffw/wiki/CPU-and-OS-History-Table), [Does XF1 have dual CPU?](https://github.com/tiredboffin/fffw/wiki/Does-XF1-have-dual-CPU%3F). Unversioned web content fetched 2026-10-07, so wiki HEAD does not bind what was read; a later revision may differ. The table's X-M1 row names platform X4, SoC FF4227, ELF container, EXR Processor II Cortex-R4F and dual-kernel micro-ITRON, recorded as E-XM1-PLATFORM-LEAD with reported scope. Sibling rows and the XF1 dual-CPU discussion are comparison material only.

<a id="s17"></a>

- **[S17] fujihack issue 23:** [Compressed firmware partitions (X-Processor Pro era) use a simple 2KB-window LZSS](https://github.com/fujihack/fujihack/issues/23), opened 2026-09-24 by @reroads203-lab. A static account of the compressed partitions of the FUJIFILM X100F Ver 2.12 `FPUPDATE.DAT`: a 0x14-byte header of five little-endian u32 `uncompressed_size, compressed_size, num_blocks, block_size (0x4000), flags`, then one continuous token stream in which a control byte below 0x80 copies that many literal bytes and a control byte at or above 0x80 takes one more byte to form a 4-bit length and an 11-bit distance, with a zero distance or length a no-op; the 0x4000 block size is an engine feed chunk, not a window reset. Scope: a different model and firmware version, host-static only; the author reports partitions 5 and 6 decoding to their declared sizes, has not solved the update-header checksum and has not flashed anything. The element-by-element cross-check against the X-M1 grammar and the round-trip result are in [the decoder section](../reports/firmware-extraction.md#decompression-grammar); no X-M1 fact is inferred from this source.

<a id="s19"></a>

- **[S19] X-T3 case study, extended account and comment thread (S-B):** the write-up of [S14](#s14) with its UPDATE 2 text and the reported comment-thread findings, captured verbatim at `local://paste-2.md`; the author modified an X-T3 running 5.11 with X-T4 2.12 as the donor, adding Classic Negative, Eterna Bleach Bypass, Color Chrome FX Blue and per-recipe white-balance shifts. Scope: a different model, a different firmware pair and an unversioned forum text, read for failure modes only. Reported points: the outer container encoding is mostly XOR 0xFF with small raw trailer segments; the inner objects use a custom literal and back-reference compression whose unobserved command cases the author's decoder rejected rather than guessed; menu position, internal simulation value, processing-table index and text identifier are separate numberings that must agree, and a wrong mapping made Classic Negative behave like Eterna; adding resources was not enough because existing text-range checks excluded the new identifiers and twelve upper-level bounds across a 35-language structure had to be repaired; each simulation takes its tone curve from a curve table and the new entries initially fell back to PROVIA's curve; one safety check read a byte holding per-camera calibration data so it failed on some bodies; several places still assumed a fixed simulation count; a per-recipe white-balance shift was not saved until the settings were stored through the save path; and the author later bricked that body with a further experimental build. Reliability: a single uncorroborated account by a non-project author with no independent reproduction, and the test body no longer exists, so none of it is X-M1 evidence. Every X-M1 claim in [the recipe design](../reports/recipe-design.md#14-sibling-failure-modes-checked-against-150-s-b) is re-derived from X-M1 bytes and these points stay leads.

<a id="s20"></a>

- **[S20] `fffw` wiki, Route 0xff80:** [notes on recovering and using the 0xff80 interface](https://github.com/tiredboffin/fffw/wiki/Route-0xff80), unversioned wiki text read 2026-10-08, so a later revision may differ. First-hand account by the `ff80` author: the protocol was recovered from `FTLUSB.dll` in an old FinePix printer driver, the camera firmware handlers were matched to it, config (cfgdata) reads can come from RAM or EEPROM while writes reach RAM only until a save is issued, config offset `0xf7` enables a high-level debug mode, a service boot is described as configurable through cfgdata, and app80 is described as the internal function table service software uses, triggered by writing a function code to config offset `0x80`, with camera-specific tools or scripts per model. Sibling findings with reported scope only; the X-M1 is not named by model in its capability claims and nothing in it is X-M1 evidence.

<a id="s21"></a>

- **[S21] `fffw` `ff80` tool source at the manifest pin:** [`ff80.py`](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/ff80/ff80.py), [`ff80_cmd/cmd.py`](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/ff80/ff80_cmd/cmd.py), [`cmd_data.py`](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/ff80/ff80_cmd/cmd_data.py), [`cmd_exp.py`](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/ff80/ff80_cmd/cmd_exp.py), [`ffjlib.py`](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/ff80/ffjlib.py), `fftlib.py`. MIT licensed; commit `bedc091e0b54a1a34aaf6929dd08e1db36d13b08` is the manifest pin and was still upstream HEAD on 2026-10-08, so this text is what the pinned tree holds. Enumerates the transport opcodes, the `0x150000`/`0x1a0002`/`0x200001` command classes, the app80 request and response layout at config offset `0x80`, the device-info tag set, data-test and diagnostic opcodes, the key-name table with `JIG` at 0x33f, and the declared-but-unimplemented subcommands already recorded in E-FFFW-NO-END-SETTING. Sibling models only; no X-M1 address, value or behaviour follows from it.

<a id="s22"></a>

- **[S22] fujihack wiki pages:** [key codes](https://wiki.fujihack.org/keys/), [EEPROM](https://wiki.fujihack.org/eep/), [syslog](https://wiki.fujihack.org/syslog/), [AUTO ACT script](https://wiki.fujihack.org/autoactscr/), [EXIF/IMFIDX10 and S3](https://wiki.fujihack.org/exif/), [code execution](https://wiki.fujihack.org/code_execution/), [firmware header](https://wiki.fujihack.org/firmware/), [FAQ](https://wiki.fujihack.org/faq/). Site content last modified 2025-06-17; the pages say the key map is crude and IDs differ per camera. Community documentation: key 825 named `JIG`; EEPROM offset `0xa2` as the AUTO_ACT.SCR boot flag and `0x1e8`/`0x1e9` as the software version bytes; the AUTO_ACT script keywords including `key`; S3 files as Motorola S-records in `IMFIDX10`, with a reader that appears to patch EEPROM and is reached from `usbcont`. Sibling findings with reported scope; the `0xff80` page there is a stub and the `ptp` page links out.

<a id="s23"></a>

- **[S23] fujihack issues 3, 4, 9 and 13:** [AUTO_ACT.SCR](https://github.com/fujihack/fujihack/issues/3), [Hidden Menu](https://github.com/fujihack/fujihack/issues/4), [Resetting firmware](https://github.com/fujihack/fujihack/issues/9), [S3 and EEPROM](https://github.com/fujihack/fujihack/issues/13). Read 2026-10-08 through the public API. Reports the firmware string `OSD DEBUG MODE SCREEN SELECT` with hidden-menu prompt text, the EEPROM `0xa2` value 2 as the AUTO_ACT activation condition with PTP/USB marked "NOT" a trigger, a maintainer statement that a service manual button combination "seemed to open the bootloader" with a green LED and no screen output, and S3 files as Motorola S-records beside `FPUPDATE.DAT`. Bodies named are HS20EXR, XF-1 and unnamed cameras; reported scope only.

<a id="s24"></a>

- **[S24] FinePix REAL 3D W3 service manual:** FinePix REAL 3D W3 Service Manual, Ref. No. ZM00868-102, 2010, confidential-marked Fujifilm service document hosted publicly by iFixit at [documents.cdn.ifixit.com/ecmDJrY12Ojmyip3.pdf](https://documents.cdn.ifixit.com/ecmDJrY12Ojmyip3.pdf), read 2026-10-08. A different platform and a FinePix body, but the same product family of documents as S25 and the X-M1 manual. Documents jig-mode entry (shutter and up key, green LED) and a BOOT SD path, the HyperFFW adjustment software and its script menu, the DSC jig driver `ZJ00684-100`, MAIN PWB Initialize as firmware download plus flash ROM reset plus internal-memory format, the per-part adjustment orders, the item list `[I] [D] [F1] [F2] [F4] [F5] [F6] [F7] [F8] [F11] [F12]`, the identity readback fields, End Setting as destination, USB_ID, repair site ID, repair date, administrator ID and repair serial writes plus the repair-to-product mode switch, and the adjustment value protection reset after a firmware download. Reliability: primary vendor document, but sibling evidence only and unversioned as a web copy.

<a id="s25"></a>

- **[S25] X-Pro1 service manual:** public adjustment chapter on ManualsLib, [manual 2198832](https://www.manualslib.com/manual/2198832/Fujifilm-X-Pro1.html?page=61), read 2026-10-08, pages 61 to 100. An X-series body one year before the X-M1. Documents the `X-Pro1_W` PC adjustment software package `ZJ01971` and its HyperFFW exit text, jig-mode entry by powering on while pressing the shutter button and up key with the LED lit green, a BOOT SD card for MAIN PWB Initialize, the caution to run End Setting after any use of the software, and the item list `[I] [F1] [F4] [F5] [F8] [F9] [F12] [E] [L] [H]` with its per-part orders. Reliability: primary vendor document as posted by a third party; sibling evidence only.

<a id="s26"></a>

- **[S26] FinePix F30 service manual index:** [ManualsLib manual 2548609](https://www.manualslib.com/manual/2548609/Fujifilm-Finepix-F30.html), read 2026-10-08 as a page index only. Shows the same function-key adjustment menu existed by 2006, with `[F3] LCD Adjustment` and `[F8] Firmware Download` entries. Sibling body; index-level evidence only.

<a id="s27"></a>

- **[S27] danielc.dev, "2 Years of Fujihack":** [blog post](https://danielc.dev/blog/2-years-of-fujihack), 2023-07-10, read 2026-10-08. Maintainer account of the X-A2 and XF-1 ports. Reported points used here: almost all Fujifilm cameras have a power-on shortcut that enters a low-power state exposing a USB interface from the boot ROM; the S3 file is executed by the `usbcont` task; a PTP/USB buffer overflow is known as a code-execution route. Dated and single-author; sibling scope.

<a id="s28"></a>

- **[S28] tiredboffin.github.io overview:** [Is it worth the trouble?](https://tiredboffin.github.io/), read 2026-10-08. The `ff80` author's summary of the firmware landscape. Describes `ff80` as the undocumented "jig" protocol used for diagnostics and adjustments at service centres and for developer logging, usable for a full live RAM dump with no firmware modification, and boot-ROM dumping on EXR bodies; states recovery is understood in principle for EXR models with a proof of concept on the X-E2 only. Sibling scope; no X-M1 claims.

<a id="s29"></a>

- **[S29] iFixit X-M1 disassembly guide:** [Fujifilm X-M1 Disassembly](https://www.ifixit.com/Guide/Fujifilm+X-M1+Disassembly/56288), read 2026-10-08. The only substantive public teardown material for this body; screw-level disassembly with no jig, service-mode or maintenance content, so it contributes a negative on repair-community coverage rather than a lead.

<a id="s30"></a>

- **[S30] Normal-mode Fujifilm PTP clients:** [petabyt/libfuji](https://github.com/petabyt/libfuji) and [malc0mn/ptp-ip](https://github.com/malc0mn/ptp-ip). Read 2026-10-08 at the repository level only. These implement ordinary PTP and PTP/IP control and file transfer, not a service protocol, so they are recorded as consulted without maintenance-mode content.

<a id="s31"></a>

- **[S31] X-M1 service manual adjustment and inspection procedures:** the procedure chapters of [S15](#s15), sections 3-1 to 3-15 (adjustments) and 4-1 to 4-3-4 (inspection) plus the jig and inspection tool lists of section 1, read 2026-10-08 from the same owner-purchased PDF (sha256 `7ebe30b1feb41f7fc20488ad674080de69cc5e38fc4e29b4647a04b08dd6ac9b`, confidential, not committed). Per procedure it records what the technician does, the equipment named, the mode entered and what the camera is claimed to do: the [I] MAIN PWB Initialize SD-BOOT and PC flows, [F9] Mount Data Write, [F4] CMOS Defect Correction, [F5] Camera Adjustment, [F7] Flash Adjustment, [F1] Battery Voltage Adjustment, [F8] Firmware Download with its Adjustment Value Protection Reset, [F12] End Setting identity writes and the repair-to-product switch, the identity readback screen, the OK/NG and "Temperature NG" error text, and the ZJ02218 dust inspection capture flow. Per-lead firmware status (confirmed, refuted, unchased) is in [the procedure sweep](../reports/service-manual-procedures.md). Scope: documentary claims about the X-M1, labeled `reported`, with no durable claim about this body; the serial and USB_ID strings printed in the manual are sample data; no procedure was executed.

<a id="s32"></a>

- **[S32] Mode-7 controller identity cross-checks:** `fffw` X-E2 4.11 [`bootrom.h`](https://github.com/tiredboffin/fffw/blob/master/ffun/ports/xe2/0411/bootrom.h) and [commit `7f313c704dee7df98a5ea19abe3bf8113e6ead90`](https://github.com/tiredboffin/fffw/commit/7f313c704dee7df98a5ea19abe3bf8113e6ead90), plus Linux [`milbeaut_usio.c`](https://github.com/torvalds/linux/blob/master/drivers/tty/serial/milbeaut_usio.c) and [`sdhci-milbeaut.c`](https://github.com/torvalds/linux/blob/master/drivers/mmc/host/sdhci-milbeaut.c), read 2026-10-12. The upstream enum calls value 7 `BOOT_DEV56` without naming a controller. The later Milbeaut UART and SDHCI drivers expose register layouts incompatible with the captured ROM's `0xfff56000` block. Scope: X-E2 sibling naming and later-generation Milbeaut controller sources only; none maps FF4227 pins, framing or a physical connector.

  **Provenance addendum, 2026-10-26:** the [`fffw` header at the recorded introducing commit](https://github.com/tiredboffin/fffw/blob/7f313c704dee7df98a5ea19abe3bf8113e6ead90/ffun/ports/xe2/0411/bootrom.h) preserves `BOOT_DEV56 = 7`. Current reproducibility references are Linux v6.18 [`milbeaut_usio.c`](https://github.com/torvalds/linux/blob/v6.18/drivers/tty/serial/milbeaut_usio.c) and [`sdhci-milbeaut.c`](https://github.com/torvalds/linux/blob/v6.18/drivers/mmc/host/sdhci-milbeaut.c), checked for the compact `0x00`-`0x0e` USIO layout and the SDHCI bridge's `0x200`-range registers. These versioned links do not identify the Linux `master` bytes read on 2026-10-12; those original revisions were not recorded. See the [append-only mode-7 provenance correction and independently supplied ROM procedure](../reports/recovery-hardware-routes.md#mode-7-provenance-correction-and-offline-reproduction). No replacement historical DAT identity is inferred from the current firmware pin.

<a id="s33"></a>

- **[S33] Expert-trajectory-regularized autofocus, CVPR 2025:** Shouhang Zhu et al., [paper and supplementary material](https://openaccess.thecvf.com/content/CVPR2025/html/Zhu_Stabilizing_and_Accelerating_Autofocus_with_Expert_Trajectory_Regularized_Deep_Reinforcement_CVPR_2025_paper.html), pp. 26440-26450, inspected 2026-10-25. The authors propose relative lens-movement prediction and expert-trajectory regularization to reduce hunting in multi-step learned AF. Reproduction lead only: no implementation, weights or reuse license was established by this inspection, and published prediction/hunting results do not measure X-M1 latency or target cost.

<a id="s34"></a>

- **[S34] Fujifilm X-T3 4.0 AF update:** [official announcement](https://www.fujifilm-x.com/en-us/news/fujifilm-releases-free-firmware-update-upgrading-the-af-performance-of-the-fujifilm-x-t3/), published 2020-10-15, read 2026-10-25. Vendor-reported algorithm changes include movement prediction, face/eye tracking, focus limiter and AF speed; the low-light result explicitly uses phase detection and XF50mmF1.0. It demonstrates a source of policy hypotheses, not portability to X-M1. Donor firmware comparison needs exact inputs, applicable rights/provenance review and independent target binding; no binaries, addresses or calibration tables are transplanted.

<a id="s35"></a>

- **[S35] x70view exact-source provenance:** [original repository](https://github.com/justjake/x70view), preserved runtime and matching-toolchain revisions in the [source manifest](../manifests/sources.json), the [bounded public recovery findings and retained dependency comparison](../third_party/README.md#exact-source-recovery-boundary), and the [exact 220-file source patch and integrity manifest](../third_party/source-recovery/README.md), inspected 2026-10-26. Original commit/tree objects survive privately. A matching public immutable archive remains unavailable, but the selected-source artifact verifies the named original blobs and restores the public bytes; 23 retained paths remain unmatched. This artifact does not replace either original pin or change the separately recorded redistribution terms.

