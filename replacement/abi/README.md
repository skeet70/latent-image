# X-M1 Rust build and ABI qualification

Issue #119 qualifies one pinned Rust `no_std` cross-build for the inferred Cortex-R4F target, using the established Clang `armv7r-none-eabi` path as the reference. The built artifact runs in the existing bounded native-case harness. This page records the facts #120 and #123 consume.

## Build

```sh
devenv shell -- bazel build //replacement/abi:all
devenv shell -- bazel test //replacement/abi:pilot-abi-tests --test_output=all
```

The Haskell test builds the positive and defect artifacts, runs both through `//emulation:native-case`, reads the positive ELF attributes, scans the linked disassembly for forbidden instructions, and runs the panic loop to the instruction budget. `pilot-case.json.in` carries the native-case contract with `@SIZE@` and `@SHA@` placeholders; the test fills them from the built image.

The test prints the positive status, the defect's changed ABI result, and the panic-loop budget stop. The native-case reports carry the artifact, case, wire and runner identities.

## Toolchain and target

- rustc: `1.98.1 (48a229cea 2026-09-01) (built from a source tarball)` from the `devenv.lock` nixpkgs revision.
- Target: `armv7r-none-eabi`, `-C target-cpu=cortex-r4f`, `-C relocation-model=static`, `-C panic=abort`, `-C codegen-units=1`, `-C debuginfo=0`, `-C opt-level=z`.
- Target spec: `panic-strategy=abort`, `relocation-model=static`, `llvm-floatabi=soft`, `frame-pointer=always`, `max-atomic-width=64`.
- `core`: `libcore-22ab4ea25c29b795.rlib`, SHA-256 `1280983b85cb9bf44577775635204ec609d75ff9e9eacb9aab378d2f9efa9844`, built from `rustLibSrc/core` with the target flags above.
- `compiler_builtins` 0.1.160: `libcompiler_builtins-4306dd84dbb95647.rlib`, SHA-256 `a47dace0bb08e82bac6fa976cac8aeea6cd9afb192d53440024858156a749c82`, built from `rustLibSrc/compiler-builtins/compiler-builtins`.
- `@lab_xm1_rust//:share/xm1-rust/build-facts.txt` carries the same identity for build manifests.

## Panic and runtime

`panic = "abort"` is fixed by the target and passed explicitly. The pilot's `#[panic_handler]` calls the assembly halt loop `xm1_panic_loop`. No unwinding tables, libc, libgcc, interpreter or dynamic section are linked. `llvm-nm -u` on the pilot ELF is empty, and `llvm-readelf -d` reports no dynamic section. Referenced `memcpy`-family intrinsics come from the staticlib's `compiler_builtins` members.

The panic smoke enters `xm1_panic_loop` directly with a 1,000-instruction budget and stops with `instruction_budget`; the loop is bounded by the harness. The `#[panic_handler]` call into that loop is inspected in the disassembly, not executed, and makes no hardware abort claim.

## Emitted ISA

The ELF carries `Tag_CPU_name: cortex-r4`, `CPU_arch: ARM v7`, `CPU_arch_profile: Real-time`, `ABI_PCS_R9_use: v6`, `ABI_align_needed: 8-byte`, `ABI_align_preserved: 8-byte data`, and `CPU_unaligned_access: Not Permitted`.

The linked disassembly contains only `mov`, `movw`, `movt`, `ldr`, `ldrb`, `str`, `strd`, `push`, `pop`, `add`, `adds`, `adc`, `sub`, `cmp`, `tst`, `eor`, `orr`, `orrs`, `orrne`, `mul`, `mvn`, `bl`, `b`, `beq`, `bne`, `movne`, `moveq`, `movwne`, `bx` and literal data. There is no A32 SDIV/UDIV, VFP, coprocessor, exclusive, barrier or mode-switch instruction.

The native runner's admission is the execution guard: the pilot returns `returned` with no `unsupported_instruction`, `memory_guard`, `fetch_guard` or `unknown_call`. Instructions outside the runner's admitted set fail at the instruction, so an emitted instruction outside the R4F profile is reported as `unsupported_instruction`, not as missing engine support.

## Actual ABI

The C harness and assembly entry are compiled and linked by the same Clang/LLD path; the expected results were derived independently in the case template.

- `u64 f(u64, u64)`: arguments in r0/r1 and r2/r3, result in r0/r1.
- `u64 f(u32, u64, u32)`: the leading `u32` occupies r0, the aligned `u64` occupies r2/r3, and the trailing `u32` is passed on the stack at `[sp]`; the result is r0/r1.
- `Pair f(u32, u32)` with `#[repr(C)] Pair { u32, u32 }`: AAPCS returns the 8-byte composite through a hidden pointer in r0; the arguments are r1 and r2. The disassembly writes with `strd r2, r3, [r0]`.
- `u32 f(Pair)`: the 8-byte composite is passed in r0/r1.
- `u32 f(u32, u32)` and `void f(u32 *, u32)`: r0/r1 and r0/r1.
- `r0`-`r3`, `r12` and `lr` are caller-saved; `r4`-`r11` and `sp` are preserved. The assembly entry seeds `r4`-`r11` and checks them immediately after the direct `xm1_pilot_preserve_pressure` call; that instruction pushes `{r4, r5, r6, r10, r11, lr}` and preserves all eight. The native case's final `r4`-`r11` values come from the entry's own push/pop, so they confirm the wrapper restores its frame rather than independently re-proving the Rust callee.
- Stack: `sp` is eight-byte aligned at entry and at every emitted call. The entry adjusts by four bytes after a nine-word push, and the generated callee pushes an even number of words (12 or 24 bytes).
- Float: the target is soft-float (`llvm-floatabi: soft`, no `ABI_VFP_args` tag), and the artifact emits no VFP instruction. The existing C reference (the recipe-bank and firmware builds) is also compiled with `-mfloat-abi=soft`; this pilot adds no retained stock float boundary. The retained stock float ABI is not qualified by this pilot; the target suffix alone is not treated as evidence for it.

The assembly entry also verifies that `sp` is unchanged after the Rust call and after the C harness. The native case's exact final-memory check covers the results region; the stack is zeroed before return so no call residue can satisfy the check.

## Negative control

The defect build compiles the same crate with `--cfg pilot_abi_defect`, which gives `xm1_pilot_add_u64` a `(u32, u32) -> u32` definition while the C harness keeps the contract's `u64` declaration. The run returns normally with runner status `returned`, but `r0` is `0x00000001` instead of zero and the results region differs from the positive run. The test asserts those structured differences and fails if the defect is accepted or if it fails before returning; the underlying report is kept in the failure message.

## Consumed facts

- #120 can use `xm1_rust_staticlib` plus `xm1_rust_elf`; the ELF, flat image, `<address> <size> <type> <name>` symbol list and disassembly are declared outputs. The linker script supplies the stock link address.
- #123 and the recipe-bank UI can link the `.a` into their existing C/assembly payload with LLD; the archive has no runtime dependency beyond its own `core` and `compiler_builtins` members. A crate root declares module files through the macro's `data` attribute.
- One linked artifact owns one Rust staticlib. Two staticlibs each carry a `#[panic_handler]`, and LLD rejects the duplicate `rust_begin_unwind`; compose the service and UI as modules of one crate root instead of linking two archives.
- `xm1_pilot_checksum` and `xm1_pilot_store` are `unsafe extern "C"` with their pointer preconditions documented in the source.
- Panic traps by looping in the pilot; a consumer that needs a different halt supplies its own handler and halt symbol. The cross-build fixes `panic=abort`.
- `@lab_xm1_rust//:share/xm1-rust/build-facts.txt` is the toolchain identity for RAM-push manifests.

## Limits

- Synthetic Cortex-R5F user-mode ARM/little-endian execution only. No X-M1 hardware, exception, interrupt, cache, DMA, MMU/MPU, peripheral or timing behavior is claimed.
- The pilot is bounded to integer, pointer and in-memory composite boundaries. No floating-point, variadic or retained stock-service boundary is qualified here.
- The native runner's Cortex-R5F model is the closest available engine; every instruction executed here is also admissible on the inferred R4F. An engine limitation (`engine_error` or a named runner gap) is not qualified by this case and would be a named prerequisite; `unsupported_instruction` is the CPU-admission refusal for an emitted instruction outside the profile.
- The panic smoke executes the halt loop directly; the `panic!` call path is inspected, not executed, and no hardware abort behavior is claimed.
- The defect demonstration covers one signature mismatch. It does not enumerate all ABI or effect defects.
