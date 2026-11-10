# Bazel and Nix integration

Bazel owns the project build graph and host tests. The project structure is Haskell/Rust, with C limited to technically necessary boundaries. The preserved viewer's Rust extension currently builds through the locked Nix package. Nix/devenv provides pinned compilers, libraries and validation tools. Supported build hosts are Linux x86_64 and macOS on Apple silicon with Nix, and Windows through WSL 2 with the Linux instructions ([macOS host](#macos-host)); no host platform is a firmware target.

## Pinned inputs

- `.bazelversion` selects Bazel 7.6.0, matching the `bazel_7` package in `devenv.nix`.
- `MODULE.bazel` selects Haskell rules 1.0 and Nixpkgs rules 0.14.0, plus C/shell/platform support. `MODULE.bazel.lock` records resolved module metadata and extension inputs. The Nix C/POSIX modules come from integrity-checked subdirectories of the upstream release archive.
- `nixpkgs.nix` reads the `nixpkgs-src` revision and NAR hash directly from `devenv.lock`. It imports that source with no user overlays through the Nixpkgs rules. `ghc.nix` selects GHC 9.10.3 with Aeson 2.2.4.1, Attoparsec 0.14.4 and cryptohash-sha256 0.11.102.1 from that exact package set; both devenv and Bazel use it. `//host:cryptohash-sha256` is visible to `//emulation`, whose native case suites hash in process, and `//matching`. `repositories.bzl` also declares GCC, Unicorn, jq, and check-jsonschema repositories from this input.
- `fuji-usb.nix` (`lab_fuji_usb`) builds [the transport's USB provider](../transport/README.md#native-provider), `transport/usb`, as a static library and runs its unit tests.
- `exr2-gcc.nix` (`lab_exr2_gcc`) builds the EXR II matching compilers, GNU GCC 4.6.4 and 4.4.7 for `arm-linux-gnueabi` with binutils 2.24, from GNU release archives with the same nixpkgs, following x70view's Linux recipe and, on macOS, its macOS recipe with x70view's three host patches in `exr2-gcc/`. `exr2-gcc/host-qsort.c` is x70view's stable host `qsort`, linked into binutils and GCC 4.4.7. The layout is `gcc-4.6.4/`, `gcc-4.4.7/` and `binutils/`, used by [`//matching:pilot`](../matching/README.md).
- `recorder-python.nix` declares the embedded CPython runtime (`lab_recorder_python`): the same Python with NumPy and Unicorn as the viewer's devenv Python, its headers and a header carrying the declared environment, base interpreter, ABI and version. `//emulation:embedded-python-lib` loads the declared base interpreter's `libpython` at run time and calls its C API from Haskell; the cardless RAM-push bridge refuses to start when the loaded interpreter differs from the declaration.
- `ram-push-toolchain-paths.nix` records the RAM-push relink tools' resolved store paths in `store-path` files (`lab_ram_push_toolchain_paths`). `//host/firmware:ram-push-toolchain-module` and `//emulation:fujimod-paths-module` read those files instead of resolving tool binaries, so a rebuild at a new store path changes their action keys and reruns the generated source.
- `rules-haskell-nix.patch` changes two Nix integration dependencies in the released Haskell module from development-only to ordinary dependencies. This gives the non-root module's Nix macros the repository visibility they need. It changes no compiler or runtime code.

Bazel 7 is deliberate: the released Haskell/POSIX integration still uses APIs removed in Bazel 8. Upgrade the rules and Bazel together after validating the Nix toolchains, Haskell binary, C library discovery, and tests. Do not replace the pinned toolchains with compilers selected implicitly from the host `PATH`.

## Commands

```sh
devenv shell -- host-build build //...
devenv shell -- host-build test //...
devenv shell -- host-build run //transport:camera -- --help
devenv shell -- host-build run //emulation:unicorn-info
```

`host-build [--output_base=PATH] COMMAND [ARGUMENT...]` (`//emulation:host-build`, which devenv also puts on PATH through `host-build.nix`) is the supported way to run a Bazel build, test or run. It waits for the host build lock, a POSIX record lock on `~/.cache/xm1-host-build/build.lock` shared by every checkout, so one admitted command runs at a time on the host; the lock is released when the process exits, however it exits, and the next holder stops any scope the previous one left running. It then runs `bazel --batch` in a `systemd-run --user --scope` with `MemoryMax=6G`, no swap and `OOMPolicy=stop`, adding `--jobs=4 --local_resources=memory=3072` before the caller's options. Batch mode keeps Bazel's JVM and actions inside the scope and leaves no server behind. In a worktree at `latent-image-worktrees/NAME` the output base is `~/.cache/bazel/latent-image-NAME`, in the main checkout Bazel's default; `--output_base` overrides it. `run` builds with `--script_path`, releases the lock and then executes the target, so an emulator target takes its slot after the build. `host-build` refuses to start inside an emulator slot or inside another admitted command, so a slot holder never waits for the build lock. A command example in this repository that calls `bazel build`, `test` or `run`, with or without `--batch`, `--output_base` and `--jobs`, runs as `host-build` with the same command, targets and options.

The last stderr line is `host-build verdict` followed by a JSON object. `verdict` is `passed`, `failed` (Bazel's exit status is kept), `interrupted` (Bazel's exit 8), `resource-failure` or `unknown`. `resource_failure` is `oom-kill` when the scope's systemd `Result` is `oom-kill`, read before systemd collects the unit, or `nix-daemon-oom-kill` when a failed command coincides with a rise in `nix-daemon.service`'s `memory.events` `oom_kill`; a resource failure exits 75 and is not a firmware or emulator discrepancy. An unreadable unit status is `unit_result` `unknown`, never success. The object also carries the lock wait, wall time, the scope's CPU time and memory peak and the Nix daemon's CPU time during the command, which includes any concurrent Nix builds outside admission.

Two resource classes share the host's 30.4 GiB (`Emulation.HostAdmission`): `build`, one admitted command at 6 GiB, and `emulator`, 2 slots of at most 8 GiB each. The worst case, 22 GiB, leaves 8.4 GiB for the OS, interactive work and Nix daemon builders. Nix builds, including the FujiMod Rust and Unicorn compilation rules_nixpkgs requests during a Bazel fetch, run in `nix-daemon.service`, outside the scope and every class, and are not memory-capped: the build lock serializes the admitted ones and `repositories.bzl` limits each to one Nix job and two cores. Capping them takes a privileged `nix-daemon.service` drop-in (`MemoryMax`, `MemorySwapMax=0`, `OOMPolicy=continue`), which is not applied.

The initial acquisition can download pinned Bazel modules and Nix store paths. After acquisition, check that the module lock is current and rerun tests without Bazel repository downloads:

```sh
devenv shell -- host-build test --lockfile_mode=error --repository_disable_download --nocache_test_results //...
```

Concurrent agents each use their own output base; `.bazelrc` points every base at the same local disk cache, `~/.cache/bazel-disk/latent-image`, so an action built in one base is reused by the others. The cache holds build outputs of private-data inputs and stays on this machine. `.bazelrc` caps a Bazel JVM's heap at 2 GiB and stops a server, which only a plain `bazel` command outside `host-build` starts, after 10 idle minutes.

Bazel 7.6's native disk-cache garbage collector keeps this cache within 20 GiB and expires entries older than 14 days after one idle minute. These bounds apply to Bazel's action cache, not output bases, Cargo targets or research artifacts. An active build can temporarily exceed the cache bound.

Concurrent agents also work in separate git worktrees, so an edit in one never invalidates another agent's build or emulator run. `build-support/agent-worktree.sh NAME [BASE]` creates `../latent-image-worktrees/NAME` on branch `agent/NAME`, symlinks the checkout's ignored `private-data/` into it and prints the output base to use. The agent commits on its `agent/NAME` branch; the integration owner merges it. Once the branch is merged into the main work branch, the integration owner removes the worktree (`git worktree remove ../latent-image-worktrees/NAME`), deletes the branch and deletes `~/.cache/bazel/latent-image-NAME`.

```sh
devenv shell -- host-build test --jobs=2 --local_test_jobs=1 //emulation:native-sdk-tests //emulation:native-runner-tests
```

Research output is compacted under the [evidence ledger rules](../docs/principles.md#evidence-ledger). A deletion plan names the current conclusions and failure classes preserved, the retained evidence and the exact redundant files to remove. Record path hashes and preservation checks under `private-data/scratch/reclaim/`, and recheck active runs, current baselines and unmerged work before applying it.

Build cleanup is restricted to positively identified compiler outputs under Cargo's guards; open or mapped candidates are retained. Source, application databases, generated imports, reports and input artifacts are never deletion targets, even inside a build directory.

`//build-support:storage-maintenance` provides explicit Cargo cache inventory, plan and apply operations. Inventory reads metadata, Cargo provenance and compiler headers without hashing application contents. Planning queues on Cargo's BSD `flock` guards and hashes only positively identified compiler artifacts and their success fingerprints. Apply queues on the guards, rechecks ownership, identities and approved contents, and skips missing, changed, open or mapped candidates. New files are outside the plan's deletion scope. Source trees and Git metadata, databases, imports, reports, firmware inputs, top-level executables, examples, unknown files and hardlinked files are retained. Test executables require an exact unit-hash match to a validated Cargo test fingerprint and an ELF header; an executable-looking filename alone is insufficient. Only success fingerprints associated with an artifact actually selected for removal are invalidated.

Content hashing advances SHA-256 state strictly with input chunks no larger than 1 MiB, sized down for small files. Snapshot paths and digests use packed text; metadata still scales with file count. Filenames that cannot be represented as Unicode are refused without publishing a plan. Add `+RTS -M2G -RTS` to bound the Haskell heap; the host-build scope separately limits total memory. Agents and applications may remain running. Cargo writes to affected profiles wait behind cleanup, and cleanup waits behind an existing Cargo writer.

```sh
target="$(realpath /path/to/cargo/target)"
plan="$PWD/private-data/scratch/reclaim/cargo-plan.json"
mkdir -p "$(dirname "$plan")"
devenv shell -- host-build run --jobs=2 //build-support:storage-maintenance -- inventory --cargo-target "$target" | jq
devenv shell -- host-build run --jobs=2 //build-support:storage-maintenance -- plan --cargo-target "$target" --output "$plan" +RTS -M2G -RTS
jq '[.bindings[] | .root as $root | .entries[] | select(.disposition != "Retain") | {root: $root, path, size, disposition}]' "$plan"
# Review the selected files; no application stop or manual agent pause is required.
devenv shell -- host-build run --jobs=2 //build-support:storage-maintenance -- apply --plan "$plan" +RTS -M2G -RTS
```

Repeat `--cargo-target` for additional explicit roots. Roots require Cargo markers, profile locks, `deps` and `.fingerprint` structures; selected compiler files require validated fingerprints. Symlinked roots, protected root paths and overlapping roots are refused. Inventory and manual plans refuse symlinks in scanned paths; automatic retention keeps unrelated symlinks without following them. Compiler discovery treats directories named `source`, `.git` or `private-data` as opaque retained trees. Their contents are not inspected for compiler provenance or hashed; whole-root retention sizing still walks their metadata. No command recursively deletes a target, worktree or research directory. `private-data`, `.git` and the Nix store cannot be managed roots.

Inventory reports `activeUsers` for context, not an idle prerequisite. Apply checks executables, mappings and open descriptors against individual deletion candidates before and after content validation. Working directories and `CARGO_TARGET_DIR` alone do not block cleanup. Ordinary invocations inspect the target owner's processes; privileged invocations inspect all visible userspace processes. Inaccessible processes still make apply refuse before removing files. Request an explicitly authorized privileged host inspection when ordinary permissions are insufficient. Do not run Bazel as root or hide existing consumers in a PID namespace to bypass inspection. The isolated tests create every root and consumer they manage.

Planning and apply acquire discovered `.cargo-lock`, `.cargo-build-lock` and `.cargo-artifact-lock` guards exclusively in sorted order. They wait for held guards rather than require a manual producer pause. If a profile's guards changed since planning, its candidates are retained. Lock-interoperability tests keep a writer's guard outside the cleanup probe's PID namespace and verify that cleanup queues until release. These namespace probes manage fresh synthetic fixtures only. Maintenance is for host-local target trees whose compiler writers honor Cargo's guards, not remote/shared caches or uncoordinated direct compiler writes.

Do not wrap cleanup in a project-wide source-generation lock or stop local services. Source capture and application-state writes may continue in retained paths; their changes do not invalidate approved compiler candidates. A rebuilt or otherwise changed candidate is skipped, and a later plan can include its new version.

Schema-3 plans and audits bind mtimes and coherent retention units rather than require a whole-tree snapshot. Apply writes `PLAN.audit.json` with its phase, exact removals, skipped paths and reasons, `deletionScopeVerified`, and measured available-space changes. `Complete` with `deletionScopeVerified=true` establishes that removals stayed within the revalidated compiler allowlist; it does not claim a byte-for-byte snapshot of concurrently changing protected data. `Applying` or `Interrupted` is not a completed cleanup. Inspect the audit before a retry. Schema-1/2 plans are refused; regenerate them against the current tree. Completed historical audits are unchanged. `recognizedBytes` is a logical size, not a promised physical saving on reflinked filesystems, and the measured filesystem delta includes concurrent activity.

The immediate availability delta is not a final reclamation measurement. Recheck `df` after cleanup; filesystems can defer reclamation, and concurrent activity can change the later result.

The shared Bazel action cache keeps its `.bazelrc` 20 GiB/14-day policy. `host-build` runs in batch mode, which does not execute Bazel's idle GC tasks. The checked-in [user service](systemd/latent-image-cache-gc.service) and [daily timer](systemd/latent-image-cache-gc.timer) start a separate non-batch server in `~/.cache/bazel/latent-image-gc`, run `build --nobuild //build-support:host`, and retain the service through the server's idle window. The service is capped at 2 GiB with no swap. No separate collector is built.

The units assume this checkout is at `~/Code/latent-image` and devenv is installed at `~/.nix-profile/bin/devenv`. Override `WorkingDirectory` or `ExecStart` with a user drop-in for other locations. Install from the checkout root:

```sh
mkdir -p "$HOME/.config/systemd/user"
ln -s "$PWD/build-support/systemd/latent-image-cache-gc.service" "$HOME/.config/systemd/user/"
ln -s "$PWD/build-support/systemd/latent-image-cache-gc.timer" "$HOME/.config/systemd/user/"
systemctl --user daemon-reload
systemctl --user enable --now latent-image-cache-gc.timer
systemctl --user start latent-image-cache-gc.service
```

Inspect `systemctl --user status latent-image-cache-gc.timer` for the next run and `journalctl --user -u latent-image-cache-gc.service` for failures. This collects only Bazel's configured disk cache; it does not prune output bases, Cargo targets, Nix staging or research data. The policy is not a hard filesystem quota, and the cache can exceed its target between collections. A separate, disabled-by-default timer applies a reviewed selective retention policy to explicit Cargo roots; [selective Cargo retention](#selective-cargo-retention) covers its policy, privilege and installation.

### Selective Cargo retention

`//build-support:storage-maintenance` can also apply a reviewed retention policy to explicit Cargo targets, deleting whole fingerprint-owned units oldest first until a root returns under its target. This is not the Bazel cache collector above and not the ordinary inventory/plan/apply workflow: it never runs Bazel or `host-build`, never discovers roots from the filesystem, and does not broaden the ordinary binary/example deletion classes. A disabled-by-default [user service](systemd/latent-image-cargo-retention.service) and [six-hour timer](systemd/latent-image-cargo-retention.timer) run it after explicit policy approval.

`storage-maintenance retain --policy FILE` is a read-only metadata preview. `storage-maintenance retain --policy FILE --apply --approved-policy-sha256 HEX +RTS -M2G -RTS` deletes only when `HEX` is the SHA-256 of the policy bytes; a missing or different digest refuses before deletion. Below a root's budget the run hashes no artifacts, deletes nothing and writes no plan or audit. Artifact mtime is the age, not last use and not LRU. Above budget, the tool derives coherent units owned by one fingerprint and removes only complete eligible units, oldest first, until the projected lower target; a unit whose eligible member or control changes, becomes protected or becomes in use is retained whole. Recent, protected and in-use content can keep a root above target: the report says `TargetUnmet` and the tool does not escalate or loop. A contended Cargo guard returns `DeferredBusy` with no deletion. Shared ambiguous incremental state is outside the automatic artifact classes.

The reviewed policy fixes each explicit canonical `cargoTarget`, its `budgetBytes`, `targetBytes` and `minimumAgeSeconds` (`0 <= targetBytes < budgetBytes`), a nonempty `artifactClasses` subset of `LibraryUnits`, `CargoTestUnits` and `BuildScriptUnits`, and an `auditDirectory` outside every managed root. Production roots, classes, budgets, minimum ages and the timer interval are pending explicit owner approval; take them from the reviewed policy file and do not treat this document as approval.

Before a unit's first unlink, the tool checks its live bindings and parent deletion permissions under the managed root owner's credentials. Artifact and success-marker unlinks use those credentials; process inspection and auditing remain privileged. Compiler writers must honor Cargo's guards; shared filesystems and uncoordinated writers are unsupported. Unlinks are not transactional: an unexpected filesystem failure can leave partial approved removals and an interrupted checkpoint. The root-owned audit directory retains that checkpoint for review and one last-complete audit, not an unbounded per-timer history.

#### Installation and approval

Install the binary, policy, environment file and sudoers rule separately from enabling the timer. The service invokes the root-owned installed copy, never `bazel-bin`, and the installed copy's actual Nix loader and library store paths need a root-owned GC root; a copied ELF alone is not pinned.

The policy format is:

```json
{
  "policyVersion": 1,
  "roots": [{
    "cargoTarget": "/absolute/reviewed/target",
    "budgetBytes": 107374182400,
    "targetBytes": 85899345920,
    "minimumAgeSeconds": 86400
  }],
  "artifactClasses": ["LibraryUnits", "CargoTestUnits", "BuildScriptUnits"],
  "auditDirectory": "/var/lib/latent-image/cargo-retention"
}
```

Measure each root's protected working set before choosing budgets; these example values are not approval. `minimumAgeSeconds` must be positive. Run the metadata preview first. Approval of an earlier exact plan does not authorize this policy's future selections.

Build once with `devenv shell -- host-build build --jobs=2 //build-support:storage-maintenance //build-support/retention-qualification:normal-build-measure`. After owner confirmation, an administrator installs the reviewed executable at `/usr/local/libexec/latent-image/storage-maintenance` and the reviewed production policy at `/etc/latent-image/cargo-retention.json`. The directories and both files must be root-owned; use mode 0755 for the executable and 0644 for the policy. Do not point a privileged rule at a user-writable Bazel output, wrapper, policy or environment file.

Inspect `ldd` on the exact executable to be installed. Record each distinct loader/library `/nix/store/<basename>` and retain that list with the installation inputs. Pin those actual runtime roots with a root-owned prefix, for example `sudo -- /nix/var/nix/profiles/default/bin/nix-store --realise --add-root /var/lib/latent-image/cargo-retention-runtime/runtime --indirect ROOT1 ROOT2`. Multiple roots create `runtime`, `runtime-2` and subsequent links. Use the absolute administrator-owned Nix executable for the host. Do not retain an entire compiler or development shell just to keep these libraries alive. Refresh the reviewed runtime list when replacing the executable.

After explicit policy approval, compute its SHA-256 and create the root-owned mode-0644 `/etc/latent-image/cargo-retention.env` containing `CARGO_RETENTION_POLICY_SHA256=APPROVED_SHA256`. Use `sudo visudo -f /etc/sudoers.d/latent-image-cargo-retention` to install the following exact-command rule, replacing `OWNER` and `APPROVED_SHA256` with the owning account and literal 64-digit approved digest. No wildcard arguments:

```sudoers
Cmnd_Alias LATENT_IMAGE_CARGO_RETENTION = /usr/local/libexec/latent-image/storage-maintenance retain --policy /etc/latent-image/cargo-retention.json --apply --approved-policy-sha256 APPROVED_SHA256 +RTS -M2G -RTS
Defaults!LATENT_IMAGE_CARGO_RETENTION env_reset, secure_path="/usr/sbin:/usr/bin:/sbin:/bin"
OWNER ALL=(root) NOPASSWD: NOSETENV: LATENT_IMAGE_CARGO_RETENTION
```

A policy-byte change needs a new owner approval, environment digest and exact-command grant. Keep the timer disabled through qualification.

#### Normal-build qualification

Use a separately reviewed single-root policy at `/etc/latent-image/cargo-retention-qualification.json` for a supervised trial. Scope the exact privileged rule to that policy's path and digest, not the production two-root policy. The captured build starts a new session: do not rely on a tty-scoped `sudo -v` ticket. An explicitly approved fixed-command grant permits the qualifier's noninteractive invocation; remove the trial grant after the comparison.

The qualifier requires the ordinary Looklab source-capture wrapper and exactly its `CARGO_TARGET_DIR` in the trial policy. It verifies the schema-2 captured-input byte generation around each phase. The normal build includes all three WASM preparations and `cargo test --locked --workspace --features looklab-fit/synth --no-run --jobs 2`, not a narrow crate substitute. A failed prime attempts no deletion.

```sh
looklab=/absolute/reviewed/looklab
qualifier="$(realpath bazel-bin/build-support/retention-qualification/normal-build-measure)"
report="$PWD/private-data/scratch/reclaim/cargo-retention-qualification.json"
trial=/etc/latent-image/cargo-retention-qualification.json
approved_trial="$(sha256sum "$trial" | cut -d ' ' -f 1)"
env CARGO_BUILD_JOBS=2 systemd-run --user --scope --quiet \
  --unit=latent-image-retention-qualification -p MemoryMax=6G -p MemorySwapMax=0 \
  nix develop "$looklab" -c "$looklab/tools/build.sh" --origin "$looklab" --lane native --target-root "$looklab/target" -- \
  "$qualifier" qualify /usr/local/libexec/latent-image/storage-maintenance \
  "$trial" "$approved_trial" "$report"
```

Review filesystem availability before/immediately after retention and after the next normal build; the measurements include concurrent activity and deferred filesystem accounting. Wall time includes WASM preparation; `nativeFreshEvents` and `nativeCompiledEvents` describe only native Cargo messages. Acceptance requires actual eviction, no recreated removed path, zero native recompilation and a successful repeat with no further deletion. A below-budget no-op or busy repeat is not qualification. Do not manufacture ages on live artifacts. This matched comparison uses the normal source guard; the periodic retention service does not acquire a project-wide source-generation lock.

#### Activation

Only after successful live build-cost review and explicit standing approval, install the user units and enable the timer:

```sh
install -Dm0644 build-support/systemd/latent-image-cargo-retention.service "$HOME/.config/systemd/user/latent-image-cargo-retention.service"
install -Dm0644 build-support/systemd/latent-image-cargo-retention.timer "$HOME/.config/systemd/user/latent-image-cargo-retention.timer"
systemctl --user daemon-reload
systemctl --user enable --now latent-image-cargo-retention.timer
```

Inspect `systemctl --user status latent-image-cargo-retention.timer` and `journalctl --user -u latent-image-cargo-retention.service`. The timer has up to 30 minutes of jitter and can catch up a missed invocation. `TargetUnmet` is a retention result, not permission to delete protected files. Stop future automatic runs with `systemctl --user disable --now latent-image-cargo-retention.timer`; removing an active run's grant does not undo completed removals.

### Research-output admission and observer retention


Emulator-slot admission, new `verify-candidate` executions and native suite/case output creation require 100 GiB available on the output filesystem. `XM1_STORAGE_MIN_FREE` accepts a positive byte count with optional `K`, `M`, `G`, `T` or `KiB`/`MiB`/`GiB`/`TiB` suffix. A generic `//emulation:emulator-slot` wrapper checks the current workspace's `private-data` filesystem, or its working directory when none exists; set `XM1_STORAGE_PATH` to the actual output location when it differs. Offline receipt judging and help remain available below the reserve. This is admission control, not a quota or a monitor that terminates existing runs, and it does not cap another project's Cargo output.

Nix keeps an observer only while something roots it, and the out-links in a worktree's Bazel output base go with the worktree, so a cited receipt can outlive the binary that produced it. `build-support/store-roots.sh list|check|pin` covers the store paths the ledger cites: every store path in `docs/evidence.jsonl` and every store path whose `bin/` or `lib/` a ledger artifact of at most 1 GiB names. `pin` adds an indirect GC root for each one present at `private-data/store-roots/<store basename>`; `check` exits 1 while any present cited path has none; `missing` lines name paths already collected, which only a rebuild from their recorded source commit restores. Run `pin` when a record citing a new observer or bridge lands.

`//build-support:xm1-decomp` (`xm1-decomp.sh`, with the Haskell indexer `//build-support:xm1-decomp-index`) looks up, prints, counts and boundary-checks x70view's X-M1 1.50 decomp at a commit from committed refs only; [reports/xm1-decomp.md](../reports/xm1-decomp.md#helper) has its commands and `//tests:xm1-decomp` covers them.

Full-system emulator runs share host CPU and memory. `//emulation:xm1-viewer` waits for one of the host's 2 emulator slots, POSIX locks `slot-1.lock` and `slot-2.lock` under `~/.cache/xm1-emulator-slots/` that every checkout on the host shares, takes whichever is free, then re-executes a pinned copy of its running executable (a concurrent build can replace the on-disk binary while the wait runs). When a systemd user session answers it runs in a `systemd-run --user --scope` with `MemoryMax=2G` and no swap; without one it prints that it runs uncapped and continues. `XM1_EMULATOR_SLOTS` (1 or 2, default 2) caps how many slots one invocation holds at once, which is how many emulator routes `verify-candidate` runs together, and `XM1_EMULATOR_MEMORY_MAX` overrides the memory cap up to the emulator class's 8G. The 2 slots are the host budget shared by every concurrent agent, so an agent runs with `XM1_EMULATOR_SLOTS=1` and `verify-candidate --native-jobs 1` unless the integration owner grants it more ([development loop](../docs/principles.md#collaboration-and-records)). Emulator commands outside the viewer, such as the preserved Python suites, take a slot through `//emulation:emulator-slot`:

```sh
devenv shell -- host-build run //emulation:emulator-slot -- sh -c 'cd RUNTIME && PYTHONPATH=native/python:. "$XM1_VIEWER_PYTHON" -m unittest native.tests.test_portable_checkpoint'
```

### Emulator speed measurements

`//build-support/emulator-speed:measure` runs the RAM-push bridge in cold, restored and resumed scenarios with GNU `time -v`, capturing `uptime` immediately before each scenario. It requires explicit bridge, run-document, ROM, DAT, adjustment NAND, checkpoint and new output paths. It saves each command, input stream, bridge stdout/stderr, timing and load beside the scenario's output. Cold uses `--fresh-boot`, restored sends only start/finish, and resumed restores then executes 40,000,000 idle instructions. `--resume-input FILE` replaces that last stream and copies it into the result directory. Measurements of two builds use identical explicit inputs and separate output directories.

```sh
devenv shell -- bazel build --jobs=2 //build-support/emulator-speed:measure
devenv shell -- env XM1_EMULATOR_SLOTS=1 bazel run --jobs=2 //emulation:emulator-slot -- \
  nix shell nixpkgs#time --command bazel-bin/build-support/emulator-speed/measure-bin \
  --bridge BRIDGE --run RUN_JSON --boot-rom ROM --installed-dat DAT \
  --adjustment-nand NAND --checkpoints PRIVATE_CHECKPOINT_ROOT --output NEW_PRIVATE_RESULT
```

The [emulator speed survey](../reports/emulator-speed.md) retains the original cold/restore reference and profile; the generic runner makes subsequent before/after pairs reproducible without that survey's private scratch inputs.

The C import uses Unicorn's `libunicorn.so.2` runtime name so Bazel's runtime symlinks satisfy its SONAME in both `bazel run` and sandboxed tests. No `LD_LIBRARY_PATH` override is required. Upstream Haskell rule helpers and the schema checker use Python internally; project-authored tools use Haskell/Rust, with C limited to necessary boundaries under [current principles](../docs/principles.md#implementation-and-build). JSON parsing uses the shared strict Haskell parser, including duplicate-key rejection, and canonical review bindings retain sorted ASCII JSON semantics.

The X-M1 viewer's locked `xm1-native.nix` build runs Rust unit checks before installing the native extension. The preserved Python checkpoint/profile integration suite separately exercises that extension's supported Python boundary and requires staged pinned ROM/DAT/unpacked fixtures. Those checks do not imply that retained Python orchestration or avoidable C host models have been migrated.

`unicorn-x70.nix` builds jitl's Unicorn fork from a pinned GitHub revision and source hash, ARM-only, for the viewer's `X70_UNICORN_LIBRARY` ([engine paths and measurements](../emulation/README.md#interactive-x-m1-firmware-viewer)). The Bazel `lab_unicorn` repository and the Python binding stay on stock nixpkgs Unicorn.

`fujimod.nix` builds jitl's FujiMod `fujimod-emulator` CLI from the public [skeet70/fujimod fork](https://github.com/skeet70/fujimod.git), revision `3d90035d7d8d651e126f89f94f30765506e6f53f`, matching `manifests/sources.json`. The recursive source pin includes vendored Unicorn revision `ba96845e5c8dcb650a66b07d6d6f0e48ca03440a`; `fujimod-unicorn-context-align.patch` remains applied. `fujimod-classified-watch.patch` adds `Board::memory_watch_begin_classified`: each physical range records only its listed access kinds (CPU read, write or fetch on either core, host or model-bus read or write), and the `Fixed` view policy makes a committed memory-decoding change or an unresolvable hooked CPU access a watch violation. The all-access `memory_watch_begin` keeps its behaviour and receipt shape. The patch changes the model store path, so the combined SDK and model need requalification before a run uses it. The build uses upstream's `Cargo.lock` and Rust 1.99.1 from a pinned rust-overlay. Bazel exposes it as the manual target `//build-support:fujimod-emulator` (repository `lab_fujimod`). Its check phase selects 443 distinct model tests through unfiltered top-level commands: all-feature `fujimod-emulator-exr2-codecs` (31), `fujimod-emulator` library tests (206), `fujimod-emulator-exr2-system` library tests (17), and the unfiltered `fujimod-emulator-cpu-unicorn` package (177 library tests, 10 `access_refusal` integration tests and 2 `nested_access_refusal` integration tests). The worker-drop test repeats itself in a one-test filtered subprocess; that repeat adds no distinct test. Node remains a check input for the existing JIT execution-gate tests. These are model checks, not native consumer qualification. It is a reference build: jitl has stated FujiMod will be AGPL, but the pinned commit has no licence file yet ([licensing item 7](../LICENSING.md#release-decisions)), and its catalog boot refuses `xm1-150`.

`fujimod-fetch-attribution.patch` adds the opt-in `Board::memory_watch_begin_with_fetch_attribution(classes, FetchAttribution::new(sources, fetch_ranges)?, limit, maps, views)` API. `CpuDataAttribution::Writes` and `ReadsAndWrites` select which monitored CPU data rows require attribution. `fetch_ranges` contains physical instruction-fetch spans with `CPU_FETCH` kinds, independent of the data-address classes. The constructor normalizes at most 64 spans; the watch supports at most two processors.

The opt-in watch caches one typed fetched-instruction observation per core. Unmatched fetches do not allocate JSON rows or consume the record bound. Ordinary classified site fetches remain complete. Opt-in CPU rows add `core_instructions`; selected data rows add the actual `latest_fetch` object with matching core, PC, global and core-local instruction coordinates, tick and byte order. A missing or stale fetch preserves the data row with `latest_fetch: null` and refuses the watch. Unavailable actual CPU progress also refuses. The `fetch_attribution` header names the selected `sources` and physical fetch `ranges`. Host and model-bus rows retain their fields; existing watch callers omit the new header and row fields. The scoped patch shares the byte-order and clock helpers.

`E-FETCH-ATTRIBUTION-HOST-150` records 380 distinct model checks and 216 default SDK checks, including all six public attribution regressions. The first SDK attempt fails in alias-fixture graph creation because an existing decoder alternative has no RAM backing; enlarging that fixture's backing fixes it without changing the SDK mechanism. The corrected package passes. The mechanically rebased scoped-watch and readback variants are not built or run by that qualification.

The same patch defines opt-in `Board::instruction_points_begin_mutable(core, addresses)` and `InstructionPoints::replace_addresses`. A mutable set retains its installed all-instruction hook while replacing at most 4,096 even addresses. Install it before any watch whose stopped-fetch interpretation depends on point-before-watch ordering. Replacing membership neither restarts the watch nor resets its journal, counters or record cap. Removing the stopped address clears its resume marker; re-adding it must stop on its first returning occurrence. The existing fixed-point API and default watch wire are unchanged. The source-reviewed root observer uses this only for runtime-known IRQ resume PCs after installing the complete fixed source-derived site superset; its new regressions are not yet exercised.

`fujimod-composite-cpu.patch` adds opt-in mutation admission to the R-profile core. A `Hook::Mutation` callback sees each `CpuMutation` after validation and before its first effect: host and guest TCM writes on both banks, direct maps, MMIO maps, unmaps, protects, live restores, execution-policy changes and public `Cpu::write_memory`. Returning false refuses the transaction through the existing error. Normalized TCM no-ops and read-only inspection emit nothing. At most 16 observers install per core; dispatch does not allocate, refuses reentrant mutations and hook changes, and keeps the first refusal for enclosing transactions. Other backends refuse registration. Uncovered: coprocessor writes other than the TCM region registers, `write_register`, writes through backing pointers, and Unicorn's own map checks, which run after admission.

`fujimod-composite-watch.patch` adds `Board::memory_guard_attach(&mut MemoryWatch, CompositeGuardConfig)`, which attaches a composite write and mapping guard to the current classified watch while paused, without changing its records, classes, fetch cache, bounds or wire. Every CPU, per-core host, physical host and model-bus source needs an explicit `SourcePolicy` of allow ranges, whose union must cover the whole access, and forbid ranges, which reject any overlap, over physical spans and per-core TCM banks. A CPU source may add up to 64 `PcPartition`s, each a set of instruction spans with its own destinations; spans of one source's partitions must be disjoint (`PartitionOverlap`), and a write from a PC outside every partition uses the source default. Span lookup and allow/forbid matching are binary searches over lists sorted at attach; one configuration holds at most 512 partition spans and 1,024 ranges. A denied guest write names the partition whose policy denied it, and `GuardStatusToken::partition_checks(core)` counts the guest writes checked under each partition. Guest writes are checked on every address independent of the classified ranges and of log exhaustion; a denied guest write is refused when the core can refuse the access and otherwise reported with outcome `Attempted` with the core stopped, and the write may have happened. Bulk host and model-bus writes are checked for the whole transfer before the first copy.

`CompositeGuardConfig::declared` lists at most 16 `DeclaredMutation`s, each admitted once. `TcmRegion { core, bank, previous, next, writer }` admits a guest TCM region register write on that core whose normalized previous and next values match, made in A32 state by an `MCR p15,0,Rt,c9,c1,{0,1}` for that bank at `writer` (the guard reads PC minus 4 at the deferred apply and checks the word there). `DecoderCommit { decoder, previous, next, core, writer }` admits a memory decoder commit whose selection a guest store at `writer` on `core` made; `Board::access` records the store's PC for the duration of the MMIO access, so a device or host selection has no writer and never matches. While an admitted commit is open, each core's `ReplacePhysical` must be one not yet replaced processor map of the declared choice. Per register and per decoder the declarations must form one chain from the value read at attach (`DeclarationChain`); a declared commit on a fixed-view watch refuses (`FixedViewsWithDeclaredCommit`). After an admitted TCM write the guard moves that core's pinned window to the new value; after a commit closes it rebuilds every core's pinned views from the graph maps with the selected choices substituted. Every other mapping, TCM, decoder (including DMA-only choices), restore and execution-policy transaction is refused before its first effect, and the fault names a `MutationRefusal` (`Undeclared`, `Mismatch { declaration, field }`, `Consumed`, `WriterUnresolved`, `HostOrigin`, `UnexpectedMapping`, `CommitIncomplete`). `GuardStatusToken::report()` returns `GuardReport { status, first_fault, write_checks, mutation_attempts, mutations_admitted }` with a sticky typed first fault, and `declared()` lists each declaration with the core and instruction count of its admission. While a guard is attached, new watches refuse; ending the watch ends the guard. A teardown that cannot remove a hook keeps its callback context and sets a board latch that refuses further execution. The patch also gives `MemoryWatch` and `InstructionPoints` hook removal on drop, ownership-checked end, and a paused-board requirement for `replace_addresses`.

The same patch adds `Board::mutation_journal_begin(limit)` and `mutation_journal_end`, which record every CPU and board mutation transaction (at most 4,096 rows, refusing when exceeded) while admitting it: offering core, mutation, PC and CPSR at dispatch, instruction count, and the writer the guard would derive (PC minus 4 for an A32 guest TCM write, the selecting store for a decoder commit). A journal and a guard cannot be active together.

`E-COMPOSITE-GUARD-SDK-HOST-150` records the passing default SDK, default adapter and scoped package checks of the single owned-PC policy, with the two failed default SDK builds whose fixes were test-only. The per-piece partitions and declared mutations above replace that policy and are armed by `//emulation:composed-startup`; their requalification is pending.

`fujimod-fetch-attribution.patch` adds the opt-in `Board::memory_watch_begin_with_fetch_attribution(classes, FetchAttribution::new(sources, fetch_ranges)?, limit, maps, views)` API. `CpuDataAttribution::Writes` and `ReadsAndWrites` select which monitored CPU data rows require attribution. `fetch_ranges` contains physical instruction-fetch spans with `CPU_FETCH` kinds, independent of the data-address classes. The constructor normalizes at most64 spans; the watch supports at most two processors.

The opt-in watch caches one typed fetched-instruction observation per core. Unmatched fetches do not allocate JSON rows or consume the record bound. Ordinary classified site fetches remain complete. Opt-in CPU rows add `core_instructions`; selected data rows add the actual `latest_fetch` object with matching core, PC, global and core-local instruction coordinates, tick and byte order. A missing or stale fetch preserves the data row with `latest_fetch: null` and refuses the watch. Unavailable actual CPU progress also refuses. The `fetch_attribution` header names the selected `sources` and physical fetch `ranges`. Host and model-bus rows retain their fields; existing watch callers omit the new header and row fields. The scoped patch shares the byte-order and clock helpers.

`fujimod-enable-readback.nix` is `fujimod.nix` with `fujimod-enable-readback.patch` added through `extraPatches`; the default build passes none and is unchanged. The patch adds image-input `options.enable_readback`: `identity` (the absent-key default) reads `0x38c` as stored, and `exchanged_source_bits` exchanges bits 1 and 9 on guest reads. Stores and clock gates use the stored word under both. The separate `options.clock.primary_gate` choice defaults to `shared_bit16`; explicit `source_bit0` removes the stored bit-16 prerequisite only for source0, preserving the common gates and every source1 prerequisite. These are emulation hypotheses for X-M1 1.50, unobserved on hardware ([IRQ41 stop in physical-1](../emulation/README.md#irq41-stop-in-physical-1)).

The check phase adds the image-input crate's library tests: `nix-build build-support/fujimod-enable-readback.nix --max-jobs 1 --cores 2`. `fujimod-xm1-enable-readback.nix` builds the SDK against that model with the crate's `Cargo.lock`. No Bazel target selects either. Schema-2 sensor fixtures select readback; schema 3 additionally selects the primary gate ([synthetic sensor source](../emulation/README.md#synthetic-sensor-source-on-fujimod)). Schema-1/2 derivations and the default model remain unchanged. Changing the bridge or run document creates a distinct checkpoint identity; an earlier cohort's checkpoint is not reusable.

`fujimod-timer-upcount.patch` is part of the default stack in `fujimod.nix`, applied after the composite patches, so every SDK and variant built from `fujimod.nix` uses it. It changes only what a read of the kernel counter `0xfff91000` returns in control mode `0x23`: `reload - remaining` (rising through the period) instead of the remaining count, as X-M1 console captures show (`E-XM1-TIMER-IRQ-LAWS-150`). Expiry ticks, line-32 delivery, mode `0x63`, the channels and the checkpoint state are unchanged; the counter range `0..reload-1` and the first-period phase after a running reload write are declared assumptions. The check phase adds the `fujimod-emulator-exr2-system` library tests, including a rendering of the stock µs busy-wait `0x723ffc`. Evidence recorded before the cutover describes the down-counting model and is re-recorded only as each item next comes up ([adoption](../reports/xm1-timer-upcount-adoption.md), `E-TIMER-UPCOUNT-DEFAULT-150`).

`fujimod-wifi-unanswered-cmd53.patch` is also in the default stack, after the timer patch. With the Wi-Fi module in protocol `enumeration` (the X-M1 run document), a CMD53 byte-mode DMA transfer of four bytes to function 1 (`0x353a0001` write or `0x353a0011` read, block size 4, block count 0) in the Transfer state no longer stops the machine: after `transfer_ticks` the host sets error status bit 0 (command timeout) and normal status bit 15, returns a zero response and moves no data. Block, PIO and other CMD53 forms still refuse. This is a declared assumption: no X-M1 capture records the module's answers, so the model gives the narrowest outcome the firmware's driver handles; the driver masks every CMD53 failure and then fails its module reset poll ([audit](../reports/emulator-device-fidelity-audit.md), `E-WIFI-CMD53-UNANSWERED-150`). The check phase adds the `fujimod-emulator-exr2-storage` library tests, which cover it.

`fujimod-rprofile-byte-exclusives.patch` follows the Wi-Fi patch in the default stack. The R-profile Unicorn core executes `LDREXB`/`STREXB` and `LDREXH`/`STREXH` (A32 and T32) through the same software path as word exclusives: the shared monitor records a reservation of the access width at the canonical physical address, a store succeeds only against a reservation of the same address and width and writes nothing otherwise, a successful store reports to the same memory observers, and halfwords follow the CPSR E bit. A halfword at an odd address refuses as an unaligned exclusive; `LDREXD`/`STREXD` still refuse (#157). Its regressions run in the default emulator library check, and `experiments/byte_exclusives` exercises it in a guest.

`fujimod-translation-admission.patch` is last in the default stack. It adds `Hook::TranslationAdmission` to the R-profile backend and worker transport. The callback receives the aligned instruction address, instruction word and Thumb flag during translation, and false refuses execution. Installing or removing admission invalidates existing translations. The existing shared code-write tracking invalidates translated RAM, including writes through aliases or another core. A failed run discards translations before a retry. Thumb translation uses bit 0 of the private Unicorn filter address to carry its instruction-set flag. Admission observers do not enter the per-entry block/instruction observer indexes and do not veto exact-loop acceleration.

The adapter's `r4f::Guard` uses translation admission on every core, without a block-entry cache. ISA exclusions are checked before generated code executes. Enabled MPU regions 12-15 remain a post-window refusal; region state is checked on every scan, including repeated execution of an already translated CP15 write. The synthetic `r4f_guard` integration controls cover both cores, initial ISA exclusions, Thumb CP15 reads, host and guest rewrites of admitted RAM, MPU refusal and exact-loop acceleration.

`fujimod-scoped-watch.nix` is `fujimod.nix` with `fujimod-scoped-watch.patch` added through `extraPatches`, after the composite patches it requires. The patch adds `Board::memory_watch_begin_scoped`, a fixed-view watch under Exact execution that keeps the classified watch's logged rows and adds per-core write/read Allow or Forbid and fetch Forbid confinement over physical spans and TCM bank offsets, aligned word-read counters with exact zero and nonzero counts and at most 64 head and tail samples, and per-word zero-store coverage. Each hooked access is either inside a TCM window snapshotted at begin or resolved through the decoder and given maps; a straddled TCM edge or an unresolved address is a fault. A TCM region register that differs from the begin snapshot is a fault at that core's next hooked access; a change undone before any hooked access is not observed. It refuses to start beside a preservation observation or any legacy memory watch not yet ended, including one a later watch replaced, and while it is active legacy watches, preservation and keyframe capture or restore refuse. A legacy watch can be ended only on the board that began it. Its regressions run in the default emulator library check: `nix-build build-support/fujimod-scoped-watch.nix --max-jobs 1 --cores 2`. `fujimod-xm1-scoped-watch.nix` builds the SDK against it with the crate's `scoped-watch` feature, which compiles the schema4 loader-prefix and schema5 composed-startup observers, and the crate's `Cargo.lock`: `nix-build build-support/fujimod-xm1-scoped-watch.nix --max-jobs 1 --cores 2`. No Bazel target or run document selects either.

`fujimod-xm1.nix` builds `emulation/fujimod-xm1` against the engine passed as `fujimod` (default `fujimod.nix`), whose source is placed beside the crate at `../fujimod`, with the crate's own `Cargo.lock` (FujiMod's lock pruned to the crate's closure), the engine's Rust 1.99.1 toolchain and crane 0.24.0, pinned by commit. Its `deps` derivation (crane `buildDepsOnly`) compiles every dependency, including FujiMod's path crates and their vendored Unicorn, against a copy of the crate whose sources crane replaces with stubs, so it depends on the crate's manifest, lock and target list and the engine, not on its sources or tests. The package reuses that target directory, runs `cargo build` and `cargo test --no-run` with `--profile release --target x86_64-unknown-linux-gnu`, installs the binaries and exports the test harnesses with their manifest; it runs no tests. `//emulation:fujimod-xm1-tests` executes every exported harness with one test thread; the crate has no doctests, the one kind of test a harness export would miss. Cargo features listed in `features` (default none) apply to both derivations. The repository `lab_fujimod_xm1` builds it with the crate files listed in `repositories.bzl` and exports the binary and its run document to `//emulation:fujimod-flash-boot-check`, and `fujimod-xm1-native-case` as `//emulation:native-runner`. FujiMod repositories build only when a target needs them, through the ordinary unprivileged Nix daemon route with one Nix job and two cores per invocation (`--max-jobs 1 --cores 2`); Cargo's job count is the Nix build's core count. Direct package builds use the same concurrency flags, for example `nix-build build-support/fujimod.nix --max-jobs 1 --cores 2`. Builds do not use `sudo`, `ROOTLOCAL`, a local store, cgroup containment or emulator slots. Emulator runtime protections above are separate and unchanged. The other Nix packages keep `--max-jobs 2 --cores 2`. Devenv does not build FujiMod.

The SDK's `out` output contains runtime executables and `share/fujimod-xm1/`; its `tests` output contains `libexec/fujimod-xm1-tests/` and the harness manifest. `lab_fujimod_xm1` selects `out`, while `lab_fujimod_xm1_tests` selects `tests` from the same derivation. Runtime evidence roots should retain `out`; retain `tests` when citing test executables. The runtime closure excludes the test output. Existing rooted monolithic SDKs remain unchanged, so the split reduces future retention rather than reclaiming historical storage. The checkpoint model identity includes `fujimod-xm1.nix`; packaging edits invalidate incompatible checkpoints through the existing restore check.

Packaging and compiled-code edit measurements are recorded in `E-FUJIMOD-SDK-OUTPUTS-150`. The split changes retention rather than compilation scope; the default SDK still builds every binary and exported harness.

Routine native qualification uses `fujimod-xm1.nix` with `nativeOnly = true`, exposed by `lab_fujimod_native` and `lab_fujimod_native_tests`. It builds `fujimod-xm1-native-case` and the `native_case` integration harness from a generated manifest that preserves the original dependencies and features but declares only that binary. [Cargo otherwise builds binary targets when an integration test is selected](https://doc.rust-lang.org/cargo/commands/cargo-test.html#target-selection). The variant reuses the full SDK's Crane dependency archive. `//emulation:native-sdk-tests` executes its complete harness manifest; `//emulation:native-runner-tests` exercises the caller-facing native contract. The full integration boundary remains `//emulation:fujimod-xm1-tests`, which executes every full-SDK harness.

`FujiModNativePaths` binds native-only Haskell consumers to this variant. `FujiModPaths` continues to bind full-system drivers and bridges to the full SDK. Native qualification does not require the full SDK through the generated path module.

`E-FUJIMOD-NATIVE-TARGETS-150` records one matched compiled-code edit and the same 74 native Rust cases plus the same native-runner suite. End-to-end time includes shell activation, admission, dependency realization, compilation and tests. Both variants reuse existing dependency artifacts; the targeted sample also realizes its new manifest generator input.

| Seconds | Full SDK | Native targets |
| --- | ---: | ---: |
| End-to-end | 466.9 | 85.8 |
| Admitted build and tests | 228.0 | 75.4 |
| Host-build lock wait | 234.3 | 6.0 |

The admitted saving is 152.6 seconds, about 67%, exceeding the experiment's 30% and 60-second adoption threshold independently of the different queue waits. This is one pair on a contended host, not a statistical speed estimate. The temporary Rust probe is removed; full integration coverage and test deadlines are unchanged.

The variants share dependency artifacts, not compiled SDK source artifacts. Full integration compiles the SDK source crate again from the shared archive. This improves routine qualification latency, not total compilation work for a single edit followed immediately by full integration.

`fujimod.nix`'s Cargo check hook derives `RUST_TEST_THREADS` from `NIX_BUILD_CORES`; the package sets `dontUseCargoParallelTests`, which makes that hook use one test thread.

### Prebuilt FujiMod repository overrides

Bazel 7 bzlmod requires canonical repository names for these `--override_repository` flags: `_main~lab_dependencies~lab_fujimod` and `_main~lab_dependencies~lab_fujimod_xm1`. The apparent names `lab_fujimod` and `lab_fujimod_xm1` leave the generated Nix repositories active and can trigger an unintended SDK build. Each override directory must expose the corresponding Bazel repository, not only an SDK store output.

An override for `//emulation:fujimod-xm1-tests` also needs `_main~lab_dependencies~lab_fujimod_xm1_tests`, exposing the separate test output's manifest and harness filegroups.

```sh
devenv shell -- bazel --batch --output_base="$HOME/.cache/bazel/latent-image-$AGENT" \
  build --jobs=2 \
  --override_repository=_main~lab_dependencies~lab_fujimod="$MODEL_REPOSITORY" \
  --override_repository=_main~lab_dependencies~lab_fujimod_xm1="$SDK_REPOSITORY" \
  //emulation:first-stage-witness //emulation:fujimod-flash-boot-check
```

Before executing either launcher, check that generated `bazel-bin/emulation/FujiModPaths.hs` names the intended immutable SDK for its driver, bridges and run document. Bind the launcher, SDK binaries, run document and firmware inputs to the retained run provenance. The [startup replay commands](../emulation/README.md#owned-first-stage-witness) name the reviewed public repository adapters. A compiled launcher binding does not qualify its runtime scenarios.


## macOS host

macOS on Apple silicon builds the same graph from the same pinned inputs and runs the emulators. What differs:

- `//build-support:host` takes its OS and CPU from Bazel's host platform and adds Nix support, so each host selects its own Nix toolchains.
- `.bazelrc`'s `macos` config, read through `--enable_platform_specific_config`, passes devenv's `SDKROOT` and `MACOSX_DEPLOYMENT_TARGET` to actions and sets `--macos_minimum_os=14.0`, the pinned nixpkgs darwin libraries' minimum. Bazel's own default targets macOS 10.11, where `clock_gettime` is unavailable.
- Unicorn is `lib/libunicorn.2.dylib`: `unicorn-x70.nix` exposes the versioned library as `library`, and `lab_unicorn` selects it by OS. The viewer's extension is installed from `lib_native.dylib`.
- FujiMod's macOS-only `apple-cf` dependency builds a Swift bridge, so `fujimod.nix` and `fujimod-xm1.nix` add nixpkgs Swift; `fujimod-xm1.nix` links the SDK's Swift overlays and `-lobjc`. FujiMod defaults its H.264 encoder to VideoToolbox on macOS; the X-M1 run document selects `software`.
- `exr2-gcc.nix` applies x70view's macOS host patches and clang's GNU89 warning relaxations.
- APFS is case-insensitive, so GHC's default `.` search path resolves module `Emulation.X` to `emulation/X.hs`. A `Main` beside an `Emulation/` module of the same name builds with `-i` (`_NO_DOT_SEARCH` in `emulation/BUILD.bazel`).
- `host-build` and emulator slots find no systemd user session and run without their memory caps. Without `/proc/self/exe`, a slot opens its executable path before the slot wait and pins its copy from that handle.
- Targets that need systemd, `/proc` or glibc declare `target_compatible_with = ["@platforms//os:linux"]`, and `bazel build //...` and `bazel test //...` skip them: `//build-support:storage-maintenance` and its tests, `//reports/review:f03-critical`.
- Darwin refuses a 256 MiB `RLIMIT_AS`, so `lz/fflzd.c` and the framing probe keep their CPU, file-size and wall-clock limits there without the address-space limit.
- Transport tests stage journaled sessions on `/dev/shm` where it exists, else in the test's temporary directory.
- The RAM-push review records pin the relink toolchain's store paths and the `//replacement/bank:bank` archive bytes, and both differ on macOS: the archive's symbol names carry host-dependent crate hashes. The camera's RAM-push pins refuse macOS relinks; build camera payloads on Linux. Linked armv7r images can still match, as `//replacement/rtos_scheduler:scheduler-a.bin` and `scheduler-b.bin` match their Linux pins.
- Unicorn on Apple silicon reports a sub-word store's whole source register to memory hooks; the native-case runner records only the stored bytes.

The RAM-push review gates are tagged `manual` on every host and run in the [pre-go gate](../transport/README.md#pre-go-gate).

## Extending the graph

Add dependencies through pinned module/Nix inputs and explicit target declarations. Update `.bazelversion` and the devenv Bazel package together; regenerate the module lock, format with buildifier, and validate all targets. Record both lockfile hashes and effective build flags with experiment artifacts. Host checks do not verify firmware architecture, target ABI, or camera behavior.

Nix-backed actions rely on the Nix store and current host platform. Remote workers need equivalent declared toolchain support before remote execution is enabled. Private camera data and workspace tool metadata are excluded from wildcard package discovery by `.bazelignore`; source and redistributable fixtures remain visible. Keep private artifacts out of shared caches unless an explicitly private cache scope is established.
