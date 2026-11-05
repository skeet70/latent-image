# FujiMod speed survey

## Scope

This survey measures the current FujiMod X-M1 RAM-push path and identifies follow-up opportunities. It is one run per scenario on a contended host, not a statistical comparison or evidence of a qualified speedup. The source pin remained `3d90035d7d8d651e126f89f94f30765506e6f53f`; no runtime patch or acceptance gate changed. These are host-emulator timings, not physical-camera timings. Follow-up work is tracked in [#158](https://github.com/skeet70/latent-image/issues/158).

## Measurements and host load

The host was a Ryzen 7 7800X3D running Linux x86_64. `/usr/bin/time -v` recorded elapsed wall time, process CPU time and the percentage of a CPU allocated to each command. The runs were contended, single observations. System-wide load averages and a sample of other processes were not captured, so the CPU percentage is the available host-load measure, not a causal attribution of off-CPU time.

| Scenario | Guest execution | Emulator process | Host CPU share |
| --- | --- | --- | ---: |
| Cold boot to attach | 397,692,000 aggregate entered instructions; 80.166 s boot; 4.961 million instructions/s during boot | 104.20 s elapsed; 22.50 CPU s (20.66 user, 1.84 system); 20.584 s checkpoint save; 3.817 million instructions/s over elapsed process time | 21% |
| Restore only | Zero boot instructions; 3.588 s restore | 6.27 s elapsed; 2.53 CPU s (1.84 user, 0.69 system); zero guest instructions/s | 40% |
| Restore and resume | 15,000,000 entries, from 397,692,000 through 412,692,000; 1.431 s guest advance; 10.485 million instructions/s during advance | 8.93 s elapsed including restore/setup; 3.76 CPU s (2.67 user, 1.09 system); 1.680 million instructions/s over elapsed process time | 42% |

The cold and restore-only process wall times have a ratio of 16.6:1, but the restored run skips boot and executes no guest instructions. This ratio is a harness opportunity, not a like-for-like speedup. The available checkpoint was selected as the nearest compatible checkpoint; older shared checkpoints with different bridge identities were not valid substitutes.

An idle observation covered a 15,000,000-entry resumed window. The carrier range and its alias accounted for 1,048 entered instructions across both cores, or 0.00699% of entries. This excludes other polling loops and idle periods outside that continuation, and counts entries rather than retired instructions. It does not establish the amount of idle work in a whole boot.

## Profile breakdown

`perf record -e cycles:u -F 99 -g` captured 2,175 samples with zero lost, representing about 75.8 billion user-cycle events. The table uses disjoint, period-weighted stack attribution. Percentages are not wall-time fractions. Frame-pointer unwinding through JIT code limits attribution, and the Haskell launcher is outside the profiled child.

| Category | Sampled user cycles |
| --- | ---: |
| Hook callbacks | 45.08% |
| Local observers | 15.97% |
| Unicorn runtime and translation | 18.74% |
| MMIO | 10.20% |
| Host, setup and unresolved | 5.87% |
| Translated JIT | 3.00% |
| Checkpoint work | 1.15% |

The largest named leaves were `Memory::flush_pending` at 10.21%, `Memory::apply_watches` at 5.70%, `DefaultHasher::write` at 7.87%, and `HashMap::insert` at 2.81%. R4F block hashing and entry work together accounted for about 16% in the survey's attribution. That is profiling evidence for the guard opportunity, not a prediction that all of it can be removed.

The pinned engine registers all-range block, code and write hooks, while translation and watch filters gate callbacks. Range-wide registration alone does not mean every instruction runs every hook. MMIO hooks are range-scoped. The X-M1 adapter's R4F observer does run for every block and vetoes loop acceleration; the admission and refusal checks remain required. Nine default trace sites remain after attach, while capture and modal watches are opt-in. See the [pinned engine source](https://github.com/skeet70/fujimod/blob/3d90035d7d8d651e126f89f94f30765506e6f53f/crates/emulator-cpu-unicorn/src/rprofile/core.rs#L239-L260) and [X-M1 R4F observer](../emulation/fujimod-xm1/src/r4f.rs).

## Pin versus upstream

The project's [FujiMod pin](../build-support/fujimod.nix) is fork commit `3d90035d7d8d651e126f89f94f30765506e6f53f`, dated 2026-10-29. The fork's main head is `90bb57ba4eebff11c62a85ec02b55ffeb33293de`, dated 2026-10-25. Official upstream main is [justjake/FujiMod `4661bf974b7213137d6d7b1a3a572612c97b83d5`](https://github.com/justjake/FujiMod/commit/4661bf974b7213137d6d7b1a3a572612c97b83d5), dated 2026-11-04. Relative to the shared base `9bb8bd90964fd4906a7d126b78a91ed30bb44e17`, official upstream has 801 commits absent from the pin and the pin has 13 absent from official upstream; fork main has 272 commits absent from the pin and the pin has 18 absent from fork main. The fork head is not a forward upgrade of the pin.

Notable upstream additions include translated ARM exclusives and identity reads ([`8db7df3d`](https://github.com/justjake/FujiMod/commit/8db7df3da27b6e25a0ba997a755d3e4d4a5f7d33)), translation-cache sizing, loop-proof caching and polling accelerators, a Thumb block-entry fast path ([`28a337ee`](https://github.com/justjake/FujiMod/commit/28a337ee09b07d3c55bdbf896585520530ce19fc)), newer counters and profiling, interactive MMIO policies, portable snapshots ([`72b44dfa`](https://github.com/justjake/FujiMod/commit/72b44dfa0ae186003f34ff0b064f8e8165e82f38)), and synchronized guest-time advancement/device access. The latter changes timing semantics, not host-thread parallelism. Upstream also advances its vendored Unicorn from `ba96845e5c8dcb650a66b07d6d6f0e48ca03440a` to `a2dfdfa01546f57f45e6afec2384ef34b4de9044`.

The local context-alignment patch is superseded by upstream's alignment work. The timer-upcount patch applies cleanly. Attribution, composite CPU/watch, classified-watch, byte-exclusives and Wi-Fi patches need rebasing. These were independent patch-compatibility checks against official main, not a test of the sequential patch stack, vendor submodule or X-M1 qualification. Preserve the local YIELD, access-refusal and JPEG fixes when rebasing.

Upstream reports translated exclusives reducing a benchmark from 21.6 to 15.0 CPU seconds, or 1.44x there. That is an upstream result, not a measured X-M1 speedup. Any source-pin change must reapply our patches and pass the full `verify-candidate` matrix before a gate changes, as required by the [runtime direction](../docs/principles.md#implementation-and-build).

## Ranked opportunities

1. **Reuse compatible checkpoints.** The observed cold and restore-only process times differ by 16.6:1, with the caveat that restore-only performs no boot. Save and reuse a checkpoint only when its bridge identity and model inputs match. Key saved checkpoints by composed DAT and model version so every harness and observer begins from the state it actually assumes.
2. **Replace the per-block hash with a one-time R4F guard decision.** The R4F hashing/entry path accounts for about 16% of sampled cycles. Removing that sampled cost alone would cap the projected ratio at about 1.19x, an inference, before considering any loop-acceleration benefit. Preserve the R4F admission and refusal controls; do not remove the guard to improve timing.
3. **Rebase onto upstream translated exclusives.** Upstream reports a 1.44x result on its own workload, not X-M1. Move to upstream `4661bf97`, reapply the local patches, and rerun the full `verify-candidate` matrix before treating it as an improvement.
4. **Reduce coherence and block-callback work.** `flush_pending` and `apply_watches` account for 15.91% of sampled cycles together. If half their cost disappeared, the projected ratio would be about 1.09x, an inference rather than an observed result.
5. **Control and record host contention.** The cold process spent 104.20 s elapsed against 22.50 CPU s, a 4.6:1 ratio. The difference includes any waiting and I/O as well as contention; it is not an emulator speedup. Record host load alongside each before/after measurement to distinguish runtime changes from host scheduling.

## Reproduction inputs

The original `measure.sh` and `commands.json` were small but tied to one old absolute worktree, an installed adapter package and owner-supplied local X-M1 1.50 ROM, DAT and adjustment-NAND files. Their exact local paths and those input files are not part of the public reproduction. The public inputs are the run configuration `emulation/fujimod-xm1/data/xm1-150.json`, the matching local X-M1 ROM/DAT/NAND inputs, scenario JSONL streams for cold boot, resumed continuation and idle observation, and the saved checkpoint set used by that same bridge identity. Supply those inputs locally and keep output directories separate by scenario.

Run through `//emulation:emulator-slot` with `XM1_EMULATOR_SLOTS=1` and Bazel `--jobs=2`. Use `time -v` for each process to capture elapsed wall time, user/system CPU time and CPU percentage. Wrap the cold scenario with `perf record -e cycles:u -F 99 -g` to collect the sampled profile. Run restore-only and resumed cases against the same matching checkpoint and model inputs; do not compare a checkpoint from a different bridge identity. The measurements above report guest entry counts separately from wall-time throughput because WFI advances guest ticks without entering instructions.
