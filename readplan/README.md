# Bounded read plan generator and window comparison

Host-only tools for the `read.config-data.v1` operation. `read-plan` writes one [read plan](../schemas/read-plan.schema.json) from the private 1.50 decoded images, and `compare-windows` compares the windows a session captured against the firmware fallback adjustment image. Neither tool loads libusb, enumerates USB or opens a device, and the tests use synthetic images only.

```sh
devenv shell -- bazel build --lockfile_mode=error --repository_disable_download //readplan:read-plan //readplan:compare-windows
devenv shell -- bazel test --lockfile_mode=error --repository_disable_download --nocache_test_results --test_output=errors //readplan/...
```

## Channel

Every plan names opcode `0x1a0002` function 2, `mb` 1 and length 256. The firmware applies no length check on that opcode, so the 256-byte window is the host's bound; windows are 256-aligned, strictly increasing and end below `0x420000`, and the plan schema, the generator and the comparison parser each refuse anything else. Function 3 writes the adjustment cache and no tool here can name it. The oracle window `0x000100` is in every plan, with the product string at byte `0x5c`, adjustment offset `0x15c`, bound as `product-string`; the read command executes windows in offset order, so the oracle is index 0 in `rc0-bound` and index 1 in the scopes that also read window `0x000000`.

## Scopes

| Scope | Selection | Table windows | Windows | Read bytes | Transfers |
| --- | --- | ---: | ---: | ---: | ---: |
| `rc0-bound` | Row-0 entries of the nine develop-stage tables, resolved from the images | 11 | 14 | 3584 | 64 |
| `rc0-extent` | Every entry inside each table's extent, resolved from the images | 17 | 19 | 4864 | 84 |
| `adjustment-42000` | `0x000000`-`0x041fff` (1056 windows) and `0x400000`-`0x40fcff` (253 windows), no image read | 0 | 1309 | 335104 | 5244 |
| `gate-byte` | `0x0000f7`, the byte `0x00647bd4` tests before the debug text callback, no image read | 0 | 4 | 1024 | 24 |

Every scope adds `0x000100` (oracle), `0x000200` (signature `+0x202`) and `0x00c000` (flag `+0xc043`, an immediate in `0x00d5eed0` rather than a table entry). Transfers count C0 open 2, tag-4 identity 4, 4 per window and C4 close 2. The nine tables, their row-0 counts and their extents are compiled into [`FujiAF/AdjustmentTables.hs`](FujiAF/AdjustmentTables.hs), with provenance in the private `adj-windows/findings.md` research note. The public generator replaces the historical `windows.c lower`/`upper` helpers; [adjustment coverage findings](../reports/adjustment-and-service-interfaces.md#adjustment-read-coverage-and-update-stamps) retain the version-bound scope and update-stamp interpretation.

## Generating a plan

```sh
mkdir -m 700 -p "$PWD/private-data/read-plans"
L="$PWD/private-data/analysis/x-m1-1.50/lz"
bazel-bin/readplan/read-plan rc0-bound read-rc0-bound-v1 "$PWD/private-data/read-plans/rc0-bound.json" "$L/id4-0061c400.bin@0x0061c400" "$L/id5-00bf1a00.bin@0x00bf1a00"
bazel-bin/readplan/read-plan rc0-extent read-rc0-extent-v1 "$PWD/private-data/read-plans/rc0-extent.json" "$L/id4-0061c400.bin@0x0061c400" "$L/id5-00bf1a00.bin@0x00bf1a00"
bazel-bin/readplan/read-plan adjustment-42000 read-adjustment-42000-v1 "$PWD/private-data/read-plans/adjustment-42000.json"
bazel-bin/readplan/read-plan gate-byte read-gate-byte-v1 "$PWD/private-data/read-plans/gate-byte.json"
```

The arguments are `SCOPE PLAN_ID NEW_PRIVATE_PLAN_JSON [IMAGE@LOAD_ADDRESS ...]`. The rc0 scopes need the decoded images and the fixed-range scopes refuse them. Standard output lists each table's resolved offset range, the table-window count, the window, byte and transfer totals, the oracle index, every image path with its SHA-256, the generator's SHA-256 and the plan's SHA-256.

Provenance in the plan: `firmware_basis` names the scope, the table extent and every image by load address and SHA-256, and a basis longer than the schema's 256 characters is refused rather than shortened; `generated_by.tool` names the scope and `generated_by.sha256` is the SHA-256 of the generator executable, which carries the compiled table list; each window's `purpose` names the fixed reason or the tables whose entries it holds.

## Comparing captured windows

```sh
bazel-bin/readplan/compare-windows "$PWD/private-data/read-plans/rc0-extent.json" /ABS/PRIVATE-SESSION/windows "$PWD/private-data/analysis/x-m1-1.50/payload.bin@0xe0048" /ABS/PRIVATE/NEW-REPORT.txt "$L/id4-0061c400.bin@0x0061c400" "$L/id5-00bf1a00.bin@0x00bf1a00"
```

The arguments are `PLAN_JSON PRIVATE_CAPTURE_DIRECTORY PAYLOAD@FALLBACK_OFFSET NEW_PRIVATE_REPORT IMAGE@LOAD_ADDRESS ...`. The capture directory is the `windows` directory `read-config` writes, one `OFFSET.bin` of 256 bytes per window, named by six hexadecimal digits. The fallback image starts at payload offset `0xe0048` and is compared over the `0x42000` bytes `0x0069f204` loads from it.

Each window is reported as `match`, `differ` with the count and range of differing offsets, or `not-comparable` with the reason: no capture file, a capture that is not a 256-byte regular file, or a window past the fallback load. The summary names the develop codes and monochrome filter indices whose row-0 entries resolve to a differing byte, the unindexed data tables with a differing entry, and the count of differing bytes no row-0 entry resolves to. The report is written to the new file and printed.

## Private paths

Both tools take absolute paths only. Every input and output must resolve, with symlinks followed, outside any Git working tree or under the ignored `private-data/` directory of the innermost working tree that contains it. Outputs are new files created with mode 0600 inside an existing directory owned by the caller with no group or other access, and the capture directory must meet the same directory rule.

## Limits

The table addresses, the offsets they resolve to and the fixed windows are derived from the 1.50 images; whether installed 1.00 uses the same layout was the question the oracle window and the signature window tested when the S03 1.00 capture ran (E-S03-FULL-CAPTURE). Later instruction-level analysis bounded the runtime index paths through `0x00d671b4`, `0x00d60ea8`, `0x00d618a4`, `0x00d69df0` and `0x00d6a11c`; [the runtime-selector correction](../reports/adjustment-and-service-interfaces.md#runtime-selector-bounds) records the row terms and setter paths. The original scope names and output stay unchanged: `rc0-bound` is row-zero coverage and lacks runtime-required windows `0x00c700` and `0x00ca00`; `rc0-extent` includes entries the stage functions may never index. The complete `adjustment-42000` capture includes the required windows. Develop-code attribution in the comparison tool covers row-zero entries only. A plan runs through `//transport:camera read-config` under [the camera-run procedure](../spec/07-governance-and-handoff.md#31-procedure).
