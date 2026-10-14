# Transport source and effect audit

Evidence here is source-inspected protocol/dependency paths plus synthetic host policy tests. Later X-M1 observations are scoped in the evidence ledger, including S02 identity (E-S02-IDENTITY-QUERY) and the stopped S03 page-zero reply (E-S03-READ-ECHO); they do not establish every upstream command or service effect. The upstream CLI is unsuitable as the operator command; use [`//transport:camera`](../transport/README.md).

| Source | Exact scope |
| --- | --- |
| [fffw](https://github.com/tiredboffin/fffw/tree/bedc091e0b54a1a34aaf6929dd08e1db36d13b08) | Commit `bedc091e0b54a1a34aaf6929dd08e1db36d13b08`; ff80 parser/imports, transport, identity and read/config/debug/experimental effects below |
| [python-libusb1](https://github.com/vpelletier/python-libusb1/tree/292143c8f4465fdcb2c35ed40cdd7e4dd8d031e1) | Commit `292143c8f4465fdcb2c35ed40cdd7e4dd8d031e1`; selected wrapper/constants/import/cleanup paths, an additional audit input rather than an upstream fffw lock |

This audit covers SHA-256 `8d714d220131178a903f23f30f6395f578900cead32c431c892be2a0ae192569` for `ff80/fftlib.py` and `9ee0f8243ebf2a4e00f98c6fdec718a3ed13e6dc2683834afe2805cbe6fb6ebc` for `ff80/ffjlib.py`; no code reads those files. Full Git inventories are regenerable from the pinned commits using [the inventory tool](review/README.md); inventory coverage does not imply every blob was audited. No upstream camera program was executed for this audit.

fffw carries an MIT notice; python-libusb1 declares LGPL-2.1-or-later. Original project software uses AGPL-3.0-only; preserve upstream notices and obligations under [the licensing review](../LICENSING.md).

The following protocol/effect details refer to the pinned upstream code. Strict local reply/model/budget rules are host policies tested with synthetic fixtures, not established X-M1 response contracts.

## Imports, dependencies, and entry paths

Source references throughout use line numbers at the exact commits above. Links into [fffw `ff80.py`](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/ff80/ff80.py), [`fftlib.py`](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/ff80/fftlib.py), [`ffjlib.py`](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/ff80/ffjlib.py), and [`cmd.py`](https://github.com/tiredboffin/fffw/blob/bedc091e0b54a1a34aaf6929dd08e1db36d13b08/ff80/ff80_cmd/cmd.py) are immutable source references.

- `ff80/__main__.py` imports `ff80` then invokes `main()` when executed as the directory program. `ff80.py` imports external `usb1`, Python stdlib `argparse`/`sys`, local `fftlib`/`ffjlib`, and **all** `ff80_cmd` exports. Its own hardware path is under `main()` and the `__main__` guard (157–193).
- `ff80_cmd/__init__.py` eagerly imports `.cmd`, then attempts `.cmd_exp`, swallowing `ImportError`. `cmd_exp.py` imports stdlib `time`/`struct` and `.cmd_data:key2code`, defines `model=None` and functions. Experimental dispatch is therefore available by default at this pin. No top-level USB calls were found in these imported local modules. Python import-path shadowing or a substituted installed dependency remains outside this source conclusion.
- `fftlib.py` imports `usb1` and `struct`; `ffjlib.py` imports `struct`. Constructors store handles/configuration and `ftl` inspects descriptors; they are not ordinary offline objects. Command-local imports include `hashlib`, `random.randbytes`, `time`, and `struct`; file I/O occurs inside commands.
- `ff80/learn-xff80.py` **opens a USB context/device at module top level**, claims interface 0 and sends session/ping operations. It then references missing `jig.print_info()`. Its `finally` closes a session but does not make import safe. Exclude this module from imports, discovery plugins and default tests.
- No `requirements.txt`, Python packaging lock or explicit `usb1` version constraint is tracked for `ff80`. The chosen dependency pin is the audit's additional input; it is not proof of what a future environment resolves. `devenv.nix` includes native `libusb1`, but no audited hardware Python runtime has been prepared.
- In the chosen dependency, `usb1/__init__.py:61–63` imports `_libusb1` and `_version`, calling `_version.get_versions()` immediately. A source-checkout import can run Git subprocesses through `_version.py:82–129,253–384,640–674` (`Popen` uses argv, not a shell; reads version/branch/date information). `_libusb1` declares ctypes types/constants; native library loading is deferred until `loadLibrary`, and `USBContext.__enter__` calls `open()` (`__init__.py:2255–2319`). `open()` loads native libusb and initializes a context with discovery enabled by default. The compatibility `usb1/libusb1.py` explicitly calls `loadLibrary()` on import. These source observations do not certify imports of arbitrary wheels or native libraries as camera-free.
- `fffw:ffun/go.mod` declares Go 1.23.10, `gopkg.in/yaml.v3 v3.0.1`; `go.sum` also records the check.v1 test dependency and exact module checksums. `ffun/Makefile` can invoke zipapp, Go builds, `go mod tidy`, `go generate`, cleanup programs and deletion. `generate.go` invokes `tools/genglue`. None was run. Go imports refer to absent `internal/cli`, `internal/pycheck`, `internal/fflz` implementations, and `py/ffundat/ffundat.py` is a stub referencing unimported `sys`. `ffun/README.md` explicitly describes unfinished translation. No published X-M1 manifest was found in the tracked inventory. Detailed extraction/build validation belongs to B-01; the YAML dependency is not acquired or relied on here.

## Selection, claiming, and cleanup

The upstream call chain is `__main__.py → ff80.main → USBContext → openByVendorIDAndProductID → ftl → jig → claimInterface(0) → open_session → cmd_info → get_device_info → close_session`.

`ff80.py:11–12,160–171` selects VID `0x04cb`, PID `0xff80`, using `skip_on_error=True`. The pinned dependency's `usb1/__init__.py:2421–2465` returns the **first** matching device and opens it. This does not enforce one candidate, physical path, private camera identity, model, firmware, or shared lease. Descriptor-access errors while enumerating can be skipped; failure to open the selected match is not a search for another authorized camera.

`fftlib.py:24–51` selects the first configuration, first interface and first alternate setting, asserts exactly three endpoints, then assigns each bulk IN/OUT endpoint found (the last of each direction wins). It compares all endpoint attributes with `TRANSFER_TYPE_BULK=2` rather than masking type bits. It does not pin numeric endpoint addresses, configuration number, interface number or alternate-setting number. It claims interface 0 separately; no call sets configuration/alternate setting, detaches a kernel driver, resets the device or clears endpoint halt. The misspelled `iterConfiguations()` is supported by the dependency's explicit compatibility alias (`usb1/__init__.py:1941–1942`), so it is not a blocker for this selected dependency.

The dependency claims before entering the `with` block and releases on its exit (`usb1/__init__.py:1055–1065,1212–1234`). Context exit finalizes related handles/devices and calls native `libusb_exit` (`2255–2259,2320–2380`). Host handle cleanup is different from closing the camera protocol session or verifying stock state.

`ff80.py:172–190` sends C4 after a normally returning command. USB access/timeout/I/O/disconnect handlers only print; they do not explicitly close the protocol session. Assertions, decoding/packing errors, `KeyboardInterrupt` and other exceptions likewise lack a protocol `finally`. The `jig_exception` handler tries C4 **after** unwinding/releasing interface 0, without a guard that C0 succeeded or a valid `sid` exists. A second close failure can escape. Failure to claim/open a context is outside part of the handler scope. Some error paths can return a successful process status despite printing errors. None verifies camera state restoration; the timeout message suggesting battery removal is an upstream string, not an independently reviewed recovery procedure.

## Exact first identity wire flow

[The 1.50 firmware map](service-query-and-jig-reset.md) separately locates these handlers and their RAM effects. It binds C4 to protocol session close, not End Setting or jig exit. The exact command branch does not select a settings writer, but a state-dependent vendor-service disconnect path reaches full settings reset and guarded persistence. Initialization validity and other lifecycle effects are separate paths. The installed body is 1.00; this is not a whole-session no-write or restoration guarantee.

All listed requests use synchronous `controlWrite` with `bmRequestType=0x43` (vendor `0x40` OR recipient-other `0x03`, OUT), `bRequest=0`, `wValue=0`, `wIndex=0`, and **12 data bytes**. The setup packet is `43 00 00 00 00 00 0c 00`; the following table describes its data stage. The dependency forces OUT direction (`usb1/__init__.py:1386–1407`); constants are `_libusb1.py:926,949`. Bulk endpoint numbers must come from separately reviewed descriptors; no numeric address is inferred for X-M1.

`S_hi S_lo` below is the big-endian encoding of the 16-bit session integer. `fftlib.ms` is 5000 ms per transfer. There is no retry or overall experiment deadline upstream.

| Stage | Exact control data | Following transfer | Source |
| --- | --- | --- | --- |
| Open C0 | `c0 00 00 00 00 00 00 00 00 08 00 00` | Bulk IN requested 8 bytes | `fftlib.py:53–60` |
| Identity command D0 | `d0 00 00 00 00 00 00 00 00 20 S_hi S_lo` | Bulk OUT exactly 32 bytes, below | `fftlib.py:85–94`; `ffjlib.py:40–46` |
| Identity result D1 | `d1 00 00 00 00 00 00 00 00 80 S_hi S_lo` | Bulk IN requested 128 bytes | `fftlib.py:96–105`; `ffjlib.py:47` |
| Close C4 | `c4 00 00 00 00 00 00 00 00 02 S_hi S_lo` | Bulk IN requested 2 bytes | `fftlib.py:73–80` |

The 12-byte general control frame has opcode at byte 0, byte 1 zero, bytes 2–5 zero for this flow, big-endian bulk length/parameter at bytes 6–9, session at bytes 10–11. C0 starts with a zero session. The identity **bulk** payload for product-name tag 4 is:

```text
00 00 15 00 00 00 00 00 00 00 00 00 00 00 00 00
04 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
```

Bulk command offset 0–3 is little-endian opcode `0x00150000`; 4–15 are zero, 16–19 little-endian tag `0x00000004`, 20–31 zero. All control writes assert returned length 12; D0 bulk write asserts returned length 32. Assertions are removable with optimized Python and must become explicit failures in a broker.

C0 reply parsing uses bytes 0–1 as little-endian session ID and **last four** bytes as big-endian maximum packet size; bytes 2–3 have no interpretation/check. It does not validate exact length 8, session validity, or max-packet bounds. A 6-byte reply could still parse. C4 consumes a requested 2-byte reply without checking actual length or contents and leaves the local `sid` populated. No source-backed C4 acknowledgement value or session idempotence is established.

Identity replies are requested at 128 bytes, not proved to be exactly 128 on hardware. At least the first 16 must exist for upstream `<4I` unpack: opcode, `errcode1`, `errcode2`, unknown `u1`, each little endian. Upstream asserts opcode `0x00150000`. Success checks only `errcode1==0`, ignoring `errcode2` and `u1`. Failure `(9,3)` returns empty text; other failures raise `jig_exception`. It does not verify a returned tag. Text starts at byte 16; upstream finds the first three consecutive zero bytes then slices through one byte beyond the match before UTF-16LE decoding. It does not bound/validate aligned termination. Missing triple zeros yields empty text, and non-ASCII or malformed terminators can yield odd-length decode failure or truncate a code unit. No identity/model match is enforced by `cmd_info`.

For deterministic offline testing, a **synthetic convention**, not a captured response, is:

```text
C0 reply: 34 12 00 00 00 00 01 00  (session 0x1234, max packet 256)
Identity reply (128 bytes):
00 00 15 00 00 00 00 00 00 00 00 00 00 00 00 00
58 00 2d 00 4d 00 31 00 00 00 [102 further zero bytes]
C4 reply: 34 12  (chosen opaque two bytes; no source acknowledgement semantics)
```

The successful text is exact `X-M1`. A-02 may use strict local rules for reply lengths, status combinations, UTF-16LE alignment/termination/padding and exact model matching; those must be labelled **broker policy tested with synthetic fixtures**, not observed camera contracts. The open middle bytes/close contents are undocumented. Fail closed on anomalies and do not normalize an unexpected model into X-M1.

Upstream `cmd_info` (`cmd.py:62–67`) sends seven identity tags in order `1,2,4,8,0x10,0x20,0x40`: vendor, manufacturer, product, “FW Version?”, device type, serial, framework. Only tag 4 is necessary for the minimal initial model check. Tag 8's format and association with installed body firmware are source questions, not proved; ordinary camera controls remain the baseline. Serial replies must remain private if later authorized. `info_tags()` also advertises `0xf0002` “FW Reset Download Status?”, explicitly describing a status mutation: exclude it even though it uses `get_device_info`.

## Read/config/debug packet and effect matrix

This table classifies source-intended effects; undocumented firmware may have more effects. H1 entries are candidates for future audit/session scope, not authorized camera actions. Unknown address validity and camera-side semantics remain blockers.

The generic `_mem_op` (`ffjlib.py:19–37`) constructs `36+data_length` bytes: LE opcode at 0–3, zero 4–15, LE function at 16–19, 16 parameter bytes at 20–35, optional data at 36 onward. D0 frames that size; D1 requests `36+read_size`. Returned first 20 are `<5I` opcode/error1/error2/zero/function; nonzero error1 raises; opcode/zero/function are asserted, error2 ignored on success. The remainder starts at 20, usually echoed parameters then data. Short lengths/status/parameter checks are inconsistent and predominantly assertions.

| Operation | Exact source path/packet or effect | Classification and limitations |
| --- | --- | --- |
| C0/D0(identity tag4)/D1/C4 | Flow above | H1 preparation candidate; protocol session state changes, no config/memory writes in this source chain; hardware effect/idempotence unknown |
| `info` seven tags | `cmd.py:62–67`; opcode `0x150000` | Identity observation candidate, private serial response; restrict initial allowlist to tag4 |
| `get_device_info(0xf0002)` | `ffjlib.py:68–69`, same identity opcode | Status-reset lead; unclassified state/persistence scope, exclude until separately established |
| `ping` | `cmd.py:69–71`, C2 then E8 | C2 control length2; E8 raw parameter/read32 (`fftlib.py:63–83,107–119`), despite comment claiming4. Session/readiness semantics unknown; exclude from minimal flow |
| Config size/read | `ffjlib.py:72–110`, opcode `0x1a0002`, functions1/2 | H1 candidate only after region/firmware evidence; no debug write in this path. Params LE `mb!=0,page,offset,length`; default mb1 described as RAM copy, mb0 labelled ROM; not proved persistent backup |
| Config write | `ffjlib.py:134–156`; `cmd.py:146–178`; function3, appended bytes | H2 if verified bounded RAM-only offsets; H3/unclassified for persistence/control offsets. Requires no automatic debug toggle here; comment says debug must be enabled |
| Debug enable/restore | `ffjlib.py:171–179` | Reads config byte offset `0xf7`, writes full byte to1 only if different, returns old byte; restore reads again and conditionally writes old. No readback after write. Source calls it USB debug; bit mask/address semantics unobserved on X-M1 |
| RAM read/dump | `cmd.py:278–310,345–383`; `ffjlib.py:193–209`; opcode `0x200001`, function2 | H2 wrappers because of debug byte writes, even though payload read. Params first8 zero, address8–11, requested chunk12–15 LE. Unknown MMIO reads may affect state. `ram read` parser defaults4; `ram dump` defaults0→unbounded. No memory-region allowlist |
| RAM write | `cmd.py:312–343`; `ffjlib.py:223–232`; function3 | H2 only with exact write set/owned RAM; MMIO/control/persistent targets unclassified. Debug toggle, appended data, no readback and no protected region bounds |
| Syslog read/output | `cmd.py:2–60`; `ffjlib.py:181–191`; debug function1 | H2 debug toggle; params first4 requested size LE. Log response has LE status/length first8 then bytes. Whether reading consumes logs is unobserved; output mode loops indefinitely |
| Boot-ROM command | `cmd.py:73–101` | H2 debug toggle/read; model allowlist includes X-M1. Its address `0x80000000`, `+0x3ff0` signature `JASM`, 0x4000 size and SHA1 `bed698ed1d91b5c29ce7c5d650156e6100c32cce` were upstream leads until the reviewed `probe-bootrom` capture matched that digest and marker on this body (E-BOOTROM-PROBE-150), so the lead's X-M1 entry is hardware-observed for these bytes; recovery proof still needs the ROM's boot paths |
| Config save | `cmd.py:256–266`, `app80_request(0x22,[0xff])`, wait5s/E8/read mailbox | H3 intended RAM→EEP persistence. Mailbox writes below. Parser exposes `load`, but dispatcher does not implement it |
| RTC “read” | `cmd.py:385–405`, sixteen `app80_request(0x06,[1,offset])` | H2/unclassified hardware operation: command-mailbox writes, not a dedicated pure memory observation. Parser exposes write/reset but dispatch implements only read |
| app80 mailbox | `ffjlib.py:158–169` | Config function3 first writes32 param bytes at0x81, then33 command+param bytes at0x80, reads33 at0x80. Writes can trigger hardware/persistence; classify enclosing command, no generic allowance |
| XE2 GPIO/ADC reads | `cmd_exp.py:9–22,66–122`, mailbox op1/2 loops | H2/unclassified; model warning is not enforcement and X-M1 absent from tested names. Model-specific labels are only X-E2 leads. All other parsed XE2 operations return unimplemented at this pin |
| Debug key/cmdline | `ffjlib.py:234–279`, debug functions5/6/7 | H2/unclassified command/state effects; arbitrary textual dispatch, exclude. Not reachable through implemented XE2 CLI actions at this pin |
| Timeouts/unknown op | `ffjlib.py:281–310`; `_mem_op(0x580000,0)` then E1; opcode `0x1a0001` | Unclassified observation leads. Timeout map is not applied to `ftl.ms`. Not part of first identity |
| Diagnostic data | `cmd.py:407–416`; `ffjlib.py:471–486`, opcode `0x450006` with ID at16 | Unclassified camera-side effects; camera-provided size drives E1/allocation without cap, exclude |
| Dummy/echo | `cmd.py:103–132`; `ffjlib.py:315–372` | Dummy opcode0x550001/op0 reads64KiB; echo opcode0x760001 sends random64KiB using E0, E8, then0x550001/op0x100. Test state/data writes and unknown semantics; exclude |
| Firmware upload | `ffjlib.py:374–405` | Prints warning then **returns before I/O** at this pin. Unreachable sketch uses opcode0x660002/E0, no complete validation/retry/completion contract. Intended H3; parser XE2 firmware action currently unimplemented. Never enable by removing the return |
| Boot examples | `boot/xe2-start.S`, `boot/xe2-led0-main.c` | X-E2 code/register-write leads: stack0xfff00f00, register0xfff88084, GPIO0xfff893c0. No X-M1 register/ABI/entry claim; exclude from execution |

## Bounds, errors, and restoration findings

The minimal identity flow has **eight** transfers: C0 control+read, D0 control+write, D1 control+read and C4 control+read. With 5000 ms per transfer, even this fixed sequence needs an independent aggregate deadline and finite cleanup budget. Upstream has no automatic retries. The selected dependency returns short bulk bytes/counts, and a timeout may contain `transferred`/`received` partial data (`usb1/__init__.py:1431–1495`); upstream discards these details in the identity CLI. Control/bulk write API results are byte counts, not camera reply payloads or camera status fields. A lost reply does not prove a request failed to reach the camera. Do not add retries without evidence of idempotence.

`read_data` E1 (`fftlib.py:122–140`) sends one length-framed request then bulk reads remaining bytes; zero-byte success causes an infinite zero-progress loop and repeatedly renews the transfer timeout. No aggregate cap exists. `write_data` E0 returns a count without demanding a full write. Camera-provided sizes (diagnostics/timeouts) can drive allocation/read length without host caps. `max_pkt_size` is not sanity checked.

`debug_read_ram` places `min(remaining,max_pkt_size)` in the parameters but asks D1 for `36+remaining`, not `36+chunk`; response data is accumulated without checking it fits the declared chunk or remaining budget. A reply containing just the echoed parameters passes their assertion but makes zero progress forever. Addresses use `to_bytes(4)` without preflight range/overflow/region checks: invalid starting values or an increment beyond 32 bits can fail after earlier requests. Per-byte/word helpers unpack exact sizes after I/O rather than making the request itself safe.

Config reads check parameter echo but not exact data length; CLI truncates to advertised size rather than requiring the requested region. Negative offsets/lengths are not rejected at parser entry. Config size reply parsing requests48 total bytes but only interprets16 bytes after20 (36 interpreted); the extra expected response bytes have no documented meaning. The legacy `dump_config_data` calls missing `get_config_data_info()` (`ffjlib.py:438`), while the CLI uses another implementation.

No memory/bootrom/simple-syslog wrapper uses `finally` around debug changes. Any failed read, file write, decode, interrupt or disconnect can skip restoration. `_dump_syslog` restores on `KeyboardInterrupt` and normal completion only; other exceptions skip it, and enabling occurs before its `try`. RAM output mode preserves an already nonzero debug byte; direct RAM read and bootrom overwrite any value unequal to1 before restoring. None verifies restoration by a subsequent read. RAM dump reads full max-packet chunks even when the remaining finite requested length is smaller, so its final chunk can exceed the requested scope. Syslog drains/polls indefinitely; several experimental loops have no end condition. Host death/disconnect cannot be solved by a `finally` alone.

## Native runtime and kernel effects

The supported native path was source-inspected and its installed runtime identified statically; no real libusb load/init/list/open occurred. [Selected native source hashes](../third_party/native-usb-source-inventory.json) retain exact inspected files. Future session approvals must bind actual runtime/dependencies, running kernel release and a concrete host/device profile; these source conclusions do not authorize live execution.

| Component | Reviewed identity |
| --- | --- |
| libusb 1.0.30 | Commit `87a55632db62c9bdc58cd31d3ccfa673f1bb017f`; Linux HAVE_LIBUDEV build; release archive SHA-256 `2ae28adb0bb9558c86135c4e1c11b320b0805461e207a64a6e520a114094bf07` |
| systemd/libudev 261.2 | Commit `4925d9f07fc697efccd98a93046ff535b8832445`; archive SHA-256 `ed1059ff964f5df35b6056434cc17cc83f86dc913f10489948a0b19b6081c5ec` |
| Reviewed Linux profile | Release `7.2.3-ogc3.1.fc44.x86_64`; OGC source tag commit `151eb3adafae8820f349a4454c1f1e8d5fc991a4`; packaging `5fc47f707b518566b41258942a0a5a7a04d532f7` |

The inspected libusb runtime SHA-256 was `d7665d10c8ca56407874773bedb5a28c31f4bd4bdbf9fafd4e48e1e3fa876478`. ELF dependencies were libudev.so.1, libpthread.so.0 and libc.so.6, with systemd-minimal-libs 261.2 and glibc 2.42-84 RUNPATHs. Source/static initializer inspection found libc-symbol resolution and compiler initialization, without device-opening/reset calls. Invoked libudev enumerate/device/monitor wrappers allocate metadata, read sysfs and `/run/udev`, and receive filtered netlink events. Unrelated device setters/openers/service APIs are outside this invoked path. The camera command opens the locked Nix runtime by a path fixed at build time; loader environment substitution and installed-package trust remain assumptions, and nothing checks the loader environment at run time.

libusb initialization scans metadata and starts a hotplug/event thread. The inspected HAVE_LIBUDEV path requires a nonnull sysfs directory before cached descriptor initialization; disappearing descriptors fail rather than falling back to opening USBFS nodes. The alternate discovery branch can open unselected nodes, resume devices and issue GET_CONFIGURATION, so it is excluded. Stable mounts/loader inputs are trusted; the precheck is not atomic hostile-host isolation.

Selected-node open uses O_RDWR and may autoresume the body. Claim changes host ownership. The supported profile forbids automatic detach and admits only one interface/alternate zero. Release follows `releaseintf → usb_driver_release_interface → usb_unbind_interface`: alternate zero skips SET_INTERFACE while disabling/re-enabling host endpoints; a nonzero alternate can set interface zero and is excluded. USBFS close may implicitly release a still-owned interface, cancel URBs and autosuspend even after explicit-release failure. libusb synchronous cancellation/event waiting and filesystem/journal work can stall beyond finite application budgets.

The reviewed [driver.c](https://github.com/OpenGamingCollective/linux/blob/151eb3adafae8820f349a4454c1f1e8d5fc991a4/drivers/usb/core/driver.c) and [devio.c](https://github.com/OpenGamingCollective/linux/blob/151eb3adafae8820f349a4454c1f1e8d5fc991a4/drivers/usb/core/devio.c) match the inspected stable 7.2.3 files. The packaging trace and installed RPM checks establish correspondence under a trusted installed-package boundary; the RPM was unsigned and there was no independent kernel binary rebuild or whole-kernel certification. Revalidate after host changes.

Critically, [hub.c](https://github.com/OpenGamingCollective/linux/blob/151eb3adafae8820f349a4454c1f1e8d5fc991a4/drivers/usb/core/hub.c) permits reset-resume through USB_QUIRK_RESET_RESUME; port resume can reset/verify, recover under persistence and restore configuration. Ordinary resume can send status/remote-wakeup requests. The reviewed configuration enabled PM, default persistence and a two-second autosuspend delay; actual body state/quirks are unobserved. **No explicit shim reset API does not establish a reset-free kernel path.** Nothing in the command binds the running kernel release or proves PM/quirk exclusion. A concrete profile excluding these branches remains unestablished. Eight explicit service transfers are not total bus traffic, and native cleanup is not stock restoration.

### First-session PM profile source trace

Source inspection and independent source review on 2026-10-03 at the same OGC Linux commit `151eb3adafae8820f349a4454c1f1e8d5fc991a4` confirmed the PM branches and cached getters below; no USB enumeration, sysfs device observation or runtime load. This trace supports a proposed manual PM check, not an assertion that resets are excluded on the actual host. The independent review did not recertify the broader libusb/libudev or installed-package correspondence claims; the installed-package trust boundary above still applies.

| Question | Exact source path and result |
| --- | --- |
| New USB device default | `hub.c:2652–2676`, `usb_new_device`, marks runtime PM active, enables it and calls `usb_disable_autosuspend`. Hubs can enable autosuspend during binding. `driver.c:1708–1712` calls `pm_runtime_forbid`. This is a USB initialization default; it is not proof that later policy or a driver leaves this camera at `on`. |
| Meaning of `on` | [drivers/base/power/runtime.c](https://github.com/OpenGamingCollective/linux/blob/151eb3adafae8820f349a4454c1f1e8d5fc991a4/drivers/base/power/runtime.c):1664–1677 sets `runtime_auto=false`, holds a usage reference and calls `rpm_resume`. Its `rpm_resume`:820–823 returns when already `RPM_ACTIVE`, before the device resume callback. A **write** of `on` to an already suspended device can resume it and reach USB effects; no such write is included in the proposed profile. |
| USB open and close | `devio.c:1034–1058`, `usbdev_open`, locks the selected device and calls `usb_autoresume_device`; `driver.c:1761–1770` uses `pm_runtime_resume_and_get`. Runtime PM active throughout avoids a runtime resume callback. `usb_autosuspend_device`:1729–1739 drops its reference; the forbid reference remains if policy has not changed. The comment at `driver.c:1746–1750` explicitly says this does **not** prevent PM-core suspend events. |
| Reset branches | `driver.c:1553–1556` resumes the USB device when `USB_STATE_SUSPENDED` **or** `reset_resume` is set. `hub.c:3667–3694`, `finish_port_resume`, resets/verifies when that flag is set and retries a failed normal status check with reset-resume under persistence. An active-state snapshot does not certify this internal flag is clear or prevent later controller/system events. |
| Quirk selection | [drivers/usb/core/quirks.c](https://github.com/OpenGamingCollective/linux/blob/151eb3adafae8820f349a4454c1f1e8d5fc991a4/drivers/usb/core/quirks.c) has no `04cb` entry in its static device/interface lists. `usb_detect_quirks`:770–796 combines static and host-specific quirks and XORs dynamic VID/PID quirks from `usbcore.quirks`. Absence from the static list is insufficient. [include/linux/usb/quirks.h](https://github.com/OpenGamingCollective/linux/blob/151eb3adafae8820f349a4454c1f1e8d5fc991a4/include/linux/usb/quirks.h):15 defines `USB_QUIRK_RESET_RESUME` as `BIT(1)`, mask `0x2`. |
| Cached PM observations | [drivers/base/power/sysfs.c](https://github.com/OpenGamingCollective/linux/blob/151eb3adafae8820f349a4454c1f1e8d5fc991a4/drivers/base/power/sysfs.c):100–105, 149–179 and 181–188 read `runtime_auto`, PM status/error/disable fields and autosuspend delay. USB `sysfs.c:252–270, 315–320` reads `quirks`, `avoid_reset_quirk` and `persist`. These show functions issue no USB transfer or device resume. Their corresponding stores are excluded. `avoid_reset_quirk` concerns `USB_QUIRK_RESET`, not the reset-resume bit. |

The proposed first-session checks require `power/control=on`, `power/runtime_status=active` and `(quirks & 0x2)==0` at the exact reviewed path immediately before the run. Record autosuspend delay, persistence and avoid-reset values as context. Missing/unreadable attributes, other status values, uncertain client ownership, a policy rewrite, host sleep or an unexpected reset/disconnect stop the session; do not write a PM setting to make the check pass.

Continuity depends on reviewed host policy and the operator's sleep-inhibition procedure. A manual observation holds no lock that makes PM policy immutable through opening, and the camera command does not read these leaves. Kernel-log observation is a detector after an event and may miss effects. The remaining check-to-open race and conditional kernel effects stay unexcluded. Normal USB attachment also permits kernel enumeration/configuration and reads of strings. Owner-reported plugged-in wakefulness for 15–20 minutes does not establish runtime PM state or wakefulness in service mode.

### Cached observation

The selected `idVendor`, `idProduct`, `busnum`, `devpath`, `bConfigurationValue` and `descriptors` reads were traced in [USB sysfs.c](https://github.com/OpenGamingCollective/linux/blob/151eb3adafae8820f349a4454c1f1e8d5fc991a4/drivers/usb/core/sysfs.c). They use cached fields/configuration bytes without USB transfers, device-node open or body resume; configuration reads take a mutex and may block. Attribute stores and string descriptors are excluded. [fs/sysfs/file.c](https://github.com/OpenGamingCollective/linux/blob/151eb3adafae8820f349a4454c1f1e8d5fc991a4/fs/sysfs/file.c) declares PAGE_SIZE capacity, while binary descriptors declare 18+65535. Those sizes are virtual capacities: validate regular leaves and cap actual returned bytes rather than treating st_size as observed content length.

No command in this repository reads sysfs; the traced reads describe what a cached observation could cost, not a delivered collector. [Transport contracts](../transport/README.md) and [validation rules](validation.md) own the implementation, tests and operator steps.
