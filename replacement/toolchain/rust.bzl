"""Freestanding X-M1 Rust cross-build and link entry points."""

_RUSTC = "@lab_xm1_rust//:bin/rustc"
_CLANG = "@lab_clang//:bin/clang"
_LLD = "@lab_lld//:bin/ld.lld"
_OBJCOPY = "@lab_llvm//:bin/llvm-objcopy"
_NM = "@lab_llvm//:bin/llvm-nm"
_OBJDUMP = "@lab_llvm//:bin/llvm-objdump"

_TARGET_FLAGS = [
    "--target=armv7r-none-eabi",
    "-C",
    "panic=abort",
    "-C",
    "relocation-model=static",
    "-C",
    "target-cpu=cortex-r4f",
    "-C",
    "codegen-units=1",
    "-C",
    "debuginfo=0",
]

_C_FLAGS = [
    "--target=armv7r-none-eabi",
    "-march=armv7-r",
    "-marm",
    "-mfloat-abi=soft",
    "-ffreestanding",
    "-nostdlib",
    "-fno-builtin",
    "-fno-stack-protector",
    "-mno-unaligned-access",
    "-Oz",
    "-Wall",
    "-Wextra",
    "-c",
]

_LINK_FLAGS = [
    "--target=armv7r-none-eabi",
    "-march=armv7-r",
    "-marm",
    "-mfloat-abi=soft",
    "-nostdlib",
    "-static",
    "-fuse-ld=lld",
    "-Wl,--no-dynamic-linker",
    "-Wl,--build-id=none",
]

def xm1_rust_staticlib(
        name,
        src,
        crate_name = None,
        edition = "2021",
        opt_level = "z",
        rustc_flags = [],
        data = [],
        testonly = False,
        visibility = None):
    """Compiles one no_std crate root to a staticlib for armv7r-none-eabi.

    `data` stages additional module files at their package paths; `mod` names in
    the crate root resolve against them.
    """
    if crate_name == None:
        crate_name = name.replace("-", "_")
    cmd = " ".join(
        [
            "$(location %s)" % _RUSTC,
            "--crate-name=%s" % crate_name,
            "--edition=%s" % edition,
            "--crate-type=staticlib",
            "-C",
            "opt-level=%s" % opt_level,
        ] + _TARGET_FLAGS + rustc_flags + [
            "$(location %s)" % src,
            "-o",
            "$@",
        ],
    )
    native.genrule(
        name = name,
        srcs = [src] + data,
        outs = [name + ".a"],
        cmd = cmd,
        message = "Cross-compiling %s for armv7r-none-eabi" % crate_name,
        testonly = testonly,
        tools = [_RUSTC],
        visibility = visibility,
    )

def xm1_rust_elf(
        name,
        rust_staticlib,
        c_srcs = [],
        asm_srcs = [],
        link_script = None,
        entry = None,
        extra_ldflags = [],
        testonly = False,
        visibility = None):
    """Links a Rust staticlib with C/assembly into an ELF, flat image and symbol list.

    The linker script owns the load addresses. The outputs are <name>.elf,
    <name>.bin, <name>.symbols and <name>.disasm in the caller's package.
    """
    srcs = [rust_staticlib] + c_srcs + asm_srcs
    if link_script != None:
        srcs.append(link_script)

    commands = ["set -e", "tmp=$$(mktemp -d)", "trap 'rm -rf \"$$tmp\"' EXIT", "objects=\"\""]
    index = 0
    for src in c_srcs:
        obj = "$$tmp/%d.o" % index
        commands.append(
            "NIX_HARDENING_ENABLE= $(location %s) %s $(location %s) -o %s" %
            (_CLANG, " ".join(_C_FLAGS), src, obj),
        )
        commands.append("objects=\"$$objects %s\"" % obj)
        index += 1
    for src in asm_srcs:
        obj = "$$tmp/%d.o" % index
        commands.append(
            "NIX_HARDENING_ENABLE= $(location %s) --target=armv7r-none-eabi -march=armv7-r -marm -c $(location %s) -o %s" %
            (_CLANG, src, obj),
        )
        commands.append("objects=\"$$objects %s\"" % obj)
        index += 1

    link = [
        "NIX_HARDENING_ENABLE= $(location %s)" % _CLANG,
        " ".join(_LINK_FLAGS),
        "--ld-path=$(location %s)" % _LLD,
    ]
    if link_script != None:
        link.append("-Wl,-T,$(location %s)" % link_script)
    if entry != None:
        link.append("-Wl,-e,%s" % entry)
    link += extra_ldflags
    link += [
        "$$objects",
        "$(location %s)" % rust_staticlib,
        "-o",
        "$(@D)/%s.elf" % name,
    ]
    commands.append(" ".join(link))
    commands.append(
        "$(location %s) -O binary $(@D)/%s.elf $(@D)/%s.bin" % (_OBJCOPY, name, name),
    )
    commands.append(
        "$(location %s) -S --defined-only --numeric-sort $(@D)/%s.elf > $(@D)/%s.symbols" %
        (_NM, name, name),
    )
    commands.append(
        "$(location %s) -d --no-show-raw-insn --triple=armv7r-none-eabi --mcpu=cortex-r4f $(@D)/%s.elf > $(@D)/%s.disasm" %
        (_OBJDUMP, name, name),
    )

    native.genrule(
        name = name,
        srcs = srcs,
        outs = [name + ".elf", name + ".bin", name + ".symbols", name + ".disasm"],
        cmd = "\n".join(commands),
        message = "Linking %s for armv7r-none-eabi" % name,
        testonly = testonly,
        tools = [_CLANG, _LLD, _OBJCOPY, _NM, _OBJDUMP],
        visibility = visibility,
    )
