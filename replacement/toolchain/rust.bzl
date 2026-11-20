"""Freestanding X-M1 Rust cross-build and link entry points."""

load("@rules_shell//shell:sh_test.bzl", "sh_test")

_RUSTC = "@lab_xm1_rust//:bin/rustc"
_CLANG = "@lab_clang//:bin/clang"
_LLD = "@lab_lld//:bin/ld.lld"
_OBJCOPY = "@lab_llvm//:bin/llvm-objcopy"
_NM = "@lab_llvm//:bin/llvm-nm"
_OBJDUMP = "@lab_llvm//:bin/llvm-objdump"

_TARGET_FLAGS_HEAD = [
    "--target=armv7r-none-eabi",
    "-C",
    "panic=abort",
    "-C",
    "relocation-model=static",
    "-C",
    "target-cpu=cortex-r4f",
    "-C",
    "codegen-units=1",
]

# The debuginfo level sits between codegen-units and jump-tables in the
# production command line; keep that position so an unset define is byte-for-byte
# the same action.
_TARGET_FLAGS_TAIL = [
    # Keep switch edges explicit for the native CFG/stack admission proof.
    "-C",
    "jump-tables=no",
]

def _strict_align_flags(strict_align):
    return ["-C", "target-feature=" + ("+strict-align" if strict_align else "-strict-align")]

# Production actions keep `-C debuginfo=0`. A build with
# `--define=xm1_rust_debuginfo=2` selects `-C debuginfo=2` instead, so the
# production graph can be inspected with DWARF without changing its loadable bytes.
_DEBUGINFO_SETTING = "//replacement/toolchain:xm1_rust_debuginfo_2"

def _debuginfo_flags(debuginfo):
    return ["-C", "debuginfo=" + debuginfo]

def _debuginfo_cmd(make_cmd):
    """Returns `make_cmd(debuginfo)` as a configurable command string."""
    return select({
        _DEBUGINFO_SETTING: make_cmd("2"),
        "//conditions:default": make_cmd("0"),
    })

def xm1_rust_debuginfo_cmd(cmd):
    """Adds DWARF only when `--define=xm1_rust_debuginfo=2` is set.

    Hand-written rustc genrules use this so their production command line is
    unchanged when the define is unset.
    """
    return select({
        _DEBUGINFO_SETTING: cmd + " -C debuginfo=2",
        "//conditions:default": cmd,
    })

def _qualification_out(out):
    if out.endswith(".rlib"):
        return out[:-5] + "-qualification.rlib"
    return out + "-qualification"

def _qualification_label(label):
    return label + "-qualification"


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
        rlib_deps = {},
        env = {},
        strict_align = True,
        testonly = False,
        visibility = None,
        rlib_search_deps = [],
        dependency_archives = [],
        debuginfo_override = None,
        tags = []):
    """Compiles one no_std crate root to a staticlib for armv7r-none-eabi.

    `data` stages additional module files at their package paths; `mod` names in
    the crate root resolve against them. `env` sets compile-time variables for
    `include!` and `env!`.
    Search-only rlibs and dependency archives use the same scoped setup as `xm1_rust_rlib`.
    `debuginfo_override` pins the debuginfo level instead of following the
    `--define=xm1_rust_debuginfo=2` selection.
    """
    if crate_name == None:
        crate_name = name.replace("-", "_")

    def make_cmd(debuginfo):
        return _rlib_archive_setup(dependency_archives) + " ".join(
            ["%s=%s" % (variable, value) for variable, value in env.items()] + [
                "$(location %s)" % _RUSTC,
                "--crate-name=%s" % crate_name,
                "--edition=%s" % edition,
                "--crate-type=staticlib",
                "-C",
                "opt-level=%s" % opt_level,
            ] + _TARGET_FLAGS_HEAD + _debuginfo_flags(debuginfo) + _TARGET_FLAGS_TAIL + _strict_align_flags(strict_align) + rustc_flags + [
                "--extern=%s=$(location %s)" % (crate, label)
                for crate, label in rlib_deps.items()
            ] + [
                "-Ldependency=$$(dirname $(location %s))" % label
                for label in rlib_deps.values()
            ] + _rlib_search_flags(rlib_search_deps, dependency_archives) + [
                "$(location %s)" % src,
                "-o",
                "$@",
            ],
        )

    native.genrule(
        name = name,
        srcs = [src] + data + rlib_deps.values() + rlib_search_deps + dependency_archives,
        outs = [name + ".a"],
        cmd = make_cmd(debuginfo_override) if debuginfo_override != None else _debuginfo_cmd(make_cmd),
        message = "Cross-compiling %s for armv7r-none-eabi" % crate_name,
        testonly = testonly,
        tags = tags,
        tools = [_RUSTC] + (["@lab_gnutar//:bin/tar"] if dependency_archives else []),
        visibility = visibility,
    )

def xm1_rust_elf(
        name,
        rust_staticlib,
        rust_components = [],
        c_srcs = [],
        asm_srcs = [],
        asm_defines = [],
        link_script = None,
        link_includes = [],
        entry = None,
        extra_ldflags = [],
        link_map = False,
        testonly = False,
        visibility = None,
        tags = []):
    """Links a Rust staticlib with C/assembly into an ELF, flat image and symbol list.

    The linker script owns the load addresses. `link_includes` are scripts it
    `INCLUDE`s by file name. `asm_defines` are `-D` names for
    the preprocessed assembly sources. The outputs are <name>.elf, <name>.bin,
    <name>.symbols and <name>.disasm in the caller's package.
    `link_map` adds <name>.map from the same link invocation.
    """
    srcs = [rust_staticlib] + rust_components + c_srcs + asm_srcs + link_includes
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
            "NIX_HARDENING_ENABLE= $(location %s) --target=armv7r-none-eabi -march=armv7-r -marm%s -c $(location %s) -o %s" %
            (_CLANG, "".join([" -D" + define for define in asm_defines]), src, obj),
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
    link += ["-Wl,-L,$$(dirname $(location %s))" % include for include in link_includes]
    if entry != None:
        link.append("-Wl,-e,%s" % entry)
    link += extra_ldflags
    link += ["$$objects"]
    if rust_components:
        # The primary archive carries the Rust runtime; the group lets later component
        # references resolve against it.
        link += ["-Wl,--start-group", "$(location %s)" % rust_staticlib]
        link += ["$(location %s)" % component for component in rust_components]
        link += ["-Wl,--end-group"]
    else:
        link += ["$(location %s)" % rust_staticlib]
    if link_map:
        link.append("-Wl,--Map=$(@D)/%s.map" % name)
    link += [
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
        outs = [name + ".elf", name + ".bin", name + ".symbols", name + ".disasm"] + ([name + ".map"] if link_map else []),
        cmd = "\n".join(commands),
        message = "Linking %s for armv7r-none-eabi" % name,
        testonly = testonly,
        tools = [_CLANG, _LLD, _OBJCOPY, _NM, _OBJDUMP],
        tags = tags,
        visibility = visibility,
    )

def _rlib_search_flags(rlib_search_deps, dependency_archives):
    flags = ["-Ldependency=$$(dirname $(location %s))" % label for label in rlib_search_deps]
    return flags + (["-Ldependency=\"$$rlib_dependencies\""] if dependency_archives else [])

def _rlib_archive_setup(dependency_archives):
    if not dependency_archives:
        return ""
    return "set -eu; rlib_dependencies=$$(mktemp -d); trap 'rm -rf \"$$rlib_dependencies\"' EXIT; " + " ".join([
        "$(location @lab_gnutar//:bin/tar) -xf $(location %s) -C \"$$rlib_dependencies\";" % archive
        for archive in dependency_archives
    ]) + " "

def xm1_rust_rlib(
        name,
        src,
        crate_name = None,
        edition = "2021",
        opt_level = "z",
        rustc_flags = [],
        data = [],
        rlib_deps = {},
        env = {},
        out = None,
        strict_align = True,
        testonly = False,
        tags = [],
        visibility = None,
        rlib_search_deps = [],
        dependency_archives = [],
        debuginfo_override = None,
        qualification = False,
        qualification_deps = [],
        qualification_rustc_flags = [],
        qualification_tags = []):
    """Compiles one no_std crate root to an rlib for armv7r-none-eabi.

    The rlib carries no panic handler unless its root declares one, so a final
    image can own the single handler. `env` sets compile-time variables for
    `include!` and `env!`.
    `rlib_search_deps` stages dependency rlibs without extern bindings.
    `dependency_archives` supplies dependency files unpacked into a scoped temporary directory.
    `debuginfo_override` pins the debuginfo level instead of following the
    `--define=xm1_rust_debuginfo=2` selection.
    `qualification` adds a test-only `<name>-qualification` rlib compiled with
    `--cfg saclac_qualification -C debuginfo=2`; labels named in
    `qualification_deps` are rewritten to their qualification targets.
    """
    if crate_name == None:
        crate_name = name.replace("-", "_")
    if out == None:
        out = name + ".rlib"

    _xm1_rust_rlib(
        name = name,
        src = src,
        crate_name = crate_name,
        edition = edition,
        opt_level = opt_level,
        rustc_flags = rustc_flags,
        data = data,
        rlib_deps = rlib_deps,
        env = env,
        out = out,
        strict_align = strict_align,
        testonly = testonly,
        tags = tags,
        visibility = visibility,
        rlib_search_deps = rlib_search_deps,
        dependency_archives = dependency_archives,
        debuginfo_override = debuginfo_override,
    )

    if qualification:
        qualified = {
            crate: (_qualification_label(label) if label in qualification_deps else label)
            for crate, label in rlib_deps.items()
        }
        qualified_search = [
            (_qualification_label(label) if label in qualification_deps else label)
            for label in rlib_search_deps
        ]
        _xm1_rust_rlib(
            name = name + "-qualification",
            src = src,
            crate_name = crate_name,
            edition = edition,
            opt_level = opt_level,
            rustc_flags = rustc_flags + ["--cfg", "saclac_qualification"] + qualification_rustc_flags,
            data = data,
            rlib_deps = qualified,
            env = env,
            out = _qualification_out(out),
            strict_align = strict_align,
            testonly = True,
            tags = tags + qualification_tags,
            visibility = visibility,
            rlib_search_deps = qualified_search,
            dependency_archives = dependency_archives,
            debuginfo_override = "2",
        )

def _xm1_rust_rlib(
        name,
        src,
        crate_name,
        edition,
        opt_level,
        rustc_flags,
        data,
        rlib_deps,
        env,
        out,
        strict_align,
        testonly,
        tags,
        visibility,
        rlib_search_deps,
        dependency_archives,
        debuginfo_override):
    def make_cmd(debuginfo):
        return _rlib_archive_setup(dependency_archives) + " ".join(
            ["%s=%s" % (variable, value) for variable, value in env.items()] + [
                "$(location %s)" % _RUSTC,
                "--crate-name=%s" % crate_name,
                "--edition=%s" % edition,
                "--crate-type=rlib",
                "-C",
                "opt-level=%s" % opt_level,
            ] + _TARGET_FLAGS_HEAD + _debuginfo_flags(debuginfo) + _TARGET_FLAGS_TAIL + _strict_align_flags(strict_align) + rustc_flags + [
                "--extern=%s=$(location %s)" % (crate, label)
                for crate, label in rlib_deps.items()
            ] + [
                "-Ldependency=$$(dirname $(location %s))" % label
                for label in rlib_deps.values()
            ] + _rlib_search_flags(rlib_search_deps, dependency_archives) + [
                "$(location %s)" % src,
                "-o",
                "$@",
            ],
        )

    native.genrule(
        name = name,
        srcs = [src] + data + rlib_deps.values() + rlib_search_deps + dependency_archives,
        outs = [out],
        cmd = make_cmd(debuginfo_override) if debuginfo_override != None else _debuginfo_cmd(make_cmd),
        message = "Cross-compiling %s as an rlib" % crate_name,
        testonly = testonly,
        tags = tags,
        tools = [_RUSTC] + (["@lab_gnutar//:bin/tar"] if dependency_archives else []),
        visibility = visibility,
    )

def xm1_rust_host_rlib(
        name,
        src,
        crate_name,
        data = [],
        rlib_deps = {},
        testonly = False,
        visibility = None,
        rlib_search_deps = [],
        dependency_archives = [],
        rustc_flags = [],
        out = None,
        qualification = False,
        qualification_deps = []):
    """Compiles a host rlib; search-only rlibs and dependency archives add no extern bindings.

    `qualification` adds a test-only `<name>-qualification` host rlib compiled
    with `--cfg saclac_qualification`; labels named in `qualification_deps` are
    rewritten to their host qualification targets.
    """
    if out == None:
        out = "lib" + crate_name + ".rlib"
    _xm1_rust_host_rlib(name, src, crate_name, data, rlib_deps, testonly, visibility, rlib_search_deps, dependency_archives, rustc_flags, out)
    if qualification:
        qualified = {
            crate: (_qualification_label(label) if label in qualification_deps else label)
            for crate, label in rlib_deps.items()
        }
        qualified_search = [
            (_qualification_label(label) if label in qualification_deps else label)
            for label in rlib_search_deps
        ]
        _xm1_rust_host_rlib(
            name + "-qualification",
            src,
            crate_name,
            data,
            qualified,
            True,
            visibility,
            qualified_search,
            dependency_archives,
            rustc_flags + ["--cfg", "saclac_qualification"],
            _qualification_out(out),
        )

def _xm1_rust_host_rlib(name, src, crate_name, data, rlib_deps, testonly, visibility, rlib_search_deps, dependency_archives, rustc_flags, out):
    native.genrule(
        name = name,
        srcs = [src] + data + rlib_deps.values() + rlib_search_deps + dependency_archives,
        outs = [out],
        cmd = _rlib_archive_setup(dependency_archives) + " ".join([
            "$(location %s)" % _RUSTC,
            "--edition=2021 --crate-type=rlib --crate-name=" + crate_name,
        ] + rustc_flags + ["--extern=%s=$(location %s)" % (crate, label) for crate, label in rlib_deps.items()] + [
            "-Ldependency=$$(dirname $(location %s))" % label for label in rlib_deps.values()
        ] + _rlib_search_flags(rlib_search_deps, dependency_archives) + ["$(location %s) -o $@" % src]),
        tools = [_RUSTC] + (["@lab_gnutar//:bin/tar"] if dependency_archives else []),
        testonly = testonly,
        visibility = visibility,
    )

def xm1_rust_host_test(name, src, crate_name, data = [], rlib_deps = {}, size = "small", visibility = None, rlib_search_deps = [], dependency_archives = [], rustc_flags = []):
    """Builds and runs host tests with optional search-only rlibs and scoped dependency archives."""
    binary = name + "-binary"
    native.genrule(
        name = binary,
        srcs = [src] + data + rlib_deps.values() + rlib_search_deps + dependency_archives,
        outs = [binary + ".bin"],
        cmd = _rlib_archive_setup(dependency_archives) + " ".join([
            "$(location %s)" % _RUSTC,
            "--edition=2021 --test --crate-name=" + crate_name,
        ] + rustc_flags + ["--extern=%s=$(location %s)" % (crate, label) for crate, label in rlib_deps.items()] + [
            "-Ldependency=$$(dirname $(location %s))" % label for label in rlib_deps.values()
        ] + _rlib_search_flags(rlib_search_deps, dependency_archives) + ["$(location %s) -o $@" % src]),
        tools = [_RUSTC] + (["@lab_gnutar//:bin/tar"] if dependency_archives else []),
        executable = True,
        testonly = True,
    )
    sh_test(
        name = name,
        srcs = ["//replacement/toolchain:host-test.sh"],
        args = ["$(location :%s)" % binary],
        data = [":" + binary],
        size = size,
        visibility = visibility,
    )
