# Host validation and review boundaries

This inventory describes the checks the code retains after the camera-run cutover that replaced the preparation/lease adapter with `//transport:camera`. The cutover removed every manifest, grant, authorization, review, scope, source-pin, lease, clearance, draft and cached-collection path together with their schemas and tests, so the earlier run counts and the claims that depended on those paths no longer describe this tree. Rerun the suite before quoting a result:

```sh
devenv shell -- bazel test --lockfile_mode=error --repository_disable_download --nocache_test_results --test_output=errors //...
```

Wire bytes did not change. The three operations emit the same C0/D0/D1/C4 frames, the same 32-byte tag-4 request, the same 36-byte function-2 request frames and the same 46-byte `POWER OFF` request as the runs recorded in the evidence ledger; what changed is how a run is authorized, where it writes and what the host checks before it starts.

| Boundary | Checks retained in code |
| --- | --- |
| Host setup/contracts | Help and unsupported-command rejection; declared Unicorn linking/API; schemas, draft manifests and evidence labels |
| Camera CLI | Unknown command, missing and extra argument refusal; relative and existing output directories; a missing default location without `BUILD_WORKSPACE_DIRECTORY`; a malformed plan stopping `read-config` and `rehearse read-config` before anything is created; no libusb or libudev in the linked binary |
| Device selection | Zero eligible devices, two eligible devices, a foreign vendor, an oversized inventory and each unsupported descriptor profile refuse before a handle is opened; endpoint numbers come from the live descriptors |
| Identity operation | The exact eight-transfer plan over an injected `UsbApi`; opaque C0 bytes carried uninterpreted; short, wrong-opcode, nonzero-status, wrong-product, partial, lost and capacity-refused replies stop with at most one close; a late transfer stops; outgoing bytes durable before dispatch and the raw reply journaled; cleanup once, and a failed release reported as uncertain |
| Bounded read | Emitted-byte assertions for opcode `0x1a0002` function 2, `mb` 1 and length 256 across all 16,896 admissible offsets, excluding function 3 and opcode `0x200001`; saved S03 C0 bytes through the synthetic pipeline with full 292-byte D1 replies; oracle mismatch, both error pairs, reserved/function/mailbox/page/offset/length mismatch, short and lost D1, short and oversized C0 and lost-C4 stops; no guessed close without a session identifier, cleanup once and preserved earlier windows; post-intent deadline refresh and remaining transfer-budget coverage; plan invariants and the journal ceiling |
| End Setting | The literal six-transfer plan and its builder over every session identifier; every one-byte opcode, reserved-word and function mutation of the reply refused; both error pairs and every unsupported pair; short, oversized, count-mismatched, failed, transfer-late and aggregate-late transfers; host cleanup once with its own budget after an expired camera deadline, and no C4 |
| Read-plan documents | The closed plan document parser over version, key set, channel, window shape and generator binding, and the window invariants the schema cannot express |
| Read-plan generation | Scope selection over the nine develop-stage offset tables, window invariants, refusal of a non-aligned or oversized window, provenance binding, and the comparison tool's match, differ and not-comparable outcomes, all on synthetic images |
| Native runner | 31 hand-specified synthetic cases on the Cortex-R5F profile, including rejection of R5F-only A32 divide and the PMSAv7 execute-never boundary; 29 JSON refusals, 14 independent C wire refusals; logical region/permission/fetch/writeback bounds and preserved state/diagnostics |
| Native shim | Synthetic root-hub, extra-endpoint, kernel-driver and partial-result probes plus the S01 jig descriptor layout, with every provider entry pointer local and the real loader never invoked |
| AF model | 21 constructed closed-loop scenarios, integer/clock/budget boundaries, observation-command association, cancellation/fallback and malformed bridge/input cases |
| General host tools | Strict JSON, bounded file/process access, independently declared SHA vectors, Git tree/source checks, framing guards and export/output refusals |

Every camera-path test injects a `UsbApi`; none builds the native provider, enumerates a bus or opens a device. The [fixture provenance](../emulation/fixtures/PROVENANCE.md) and [AF fixture definitions](../af/fixtures/README.md) retain the independently specified expectations. Checked-in fixtures are synthetic, with no stock firmware copied into them.

## Rules derived from review

These remain implemented and tested, regardless of the original finding IDs:

- Reject decoded duplicate keys, malformed UTF-8/surrogates, nonfinite JSON and excessive nesting. Numeric tokens and absolute exponents are bounded at 128; integer profiles reject decimal/exponent spelling.
- Clean process groups, stdin writers, readers and owned descriptors on successful exit as well as timeout/cap/failure. Preserve bounded output prefixes and reap children. The AF bridge follows the same rule.
- Record late/partial USB outcomes before refusing them. An unknown session identifier never permits a guessed close; eight explicit service dispatches do not imply eight total bus transactions.
- Write and fsync outgoing bytes before dispatch, and raw replies before parsing, into a private bounded regular journal.
- Reject aliased/existing/dangling-link outputs and special files; check both paths and opened descriptors, with bounded reads and replacement-race tests.
- Treat a successful reply, a released interface and a closed handle as host facts only; none of them observes camera state.

## Reproduction and limits

[Host-tool commands](review/README.md) reproduce source/export/framing and caller probes. The preserved public DAT's complete export/framing metadata and upstream Git inventories were checked through the pinned environment; those format-specific claims are in [firmware findings](firmware-extraction.md). Static native metadata uses `pkg-config`, hashes and `readelf`, without loading libusb.

Earlier runs used populated pinned Nix/Bazel dependencies with Bazel downloads disabled. They do not independently rebuild the entire toolchain or prove empty-cache acquisition. Filesystem/kernel/event-loop progress, trusted installed tools/loader/Nix store and cooperating-account assumptions remain; finite application deadlines are not hard real-time cancellation guarantees.

The [native effect audit](source-and-transport-audit.md#native-runtime-and-kernel-effects) retains the unresolved PM/reset/reconfiguration condition. Exact future firmware/camera/session artifacts still need hashes and durable private records; superseded host run diaries and full source-file hash lists can be recovered from Git.
