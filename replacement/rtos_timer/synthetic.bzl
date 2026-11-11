"""Manual synthetic source linkage using caller-supplied canonical inputs.

Invoke from //host/firmware, which owns the RAM producer's source visibility.
SYNTHETIC_SPEC, SYNTHETIC_IMAGE4, SYNTHETIC_IMAGE5, SYNTHETIC_PLAIN,
SYNTHETIC_SERVICE_ARCHIVE, SYNTHETIC_MENU_TABLES, SYNTHETIC_MODAL75_TABLE,
SYNTHETIC_SDK_QUALIFICATION and SYNTHETIC_SOURCE_ROOT name watched inputs.
The declared base 0x02000000 is synthetic, not an allocation witness.
"""

load("//replacement/property_service:recipe.bzl", "property_service_recipe_image")

_VALIDATOR = "//replacement/rtos_timer:synthetic-combined"
_SOURCES = [
    "//menu:ram-push-relink-sources",
    "//replacement/bank:modal-render-relink-sources",
    "//host/firmware:build-recipe-bank-sources",
    "//host/firmware:RenderStack.hs",
    "//host/firmware:build-recipe-bank-modal-render-ram.sh",
    "//menu:recipe_bank_ram_engine.ld",
    "//menu:recipe_bank_modal_allowlist_fragment.json",
    "//menu:recipe_bank_menu_engine_allowlist_fragment.json",
    "//menu:recipe_bank_render_allowlist_fragment.json",
    "//host/firmware:build-recipe-bank-ram.sh",
]
_TOOLS = [
    "@lab_bash//:bin/bash", "@lab_clang//:bin/clang", "@lab_lld//:bin/ld.lld",
    "@lab_llvm//:bin/llvm-objcopy", "@lab_llvm//:bin/llvm-nm", "@lab_llvm//:bin/llvm-objdump",
    "//host/firmware:build-recipe-bank", _VALIDATOR,
    "@lab_xm1_rust//:bin/rustc", "//replacement/modal_runtime:menu-engine-tree",
    "@lab_coreutils//:bin/sha256sum",
    "@lab_xm1_rust//:share/xm1-rust/build-facts.txt",
]
_EDIT = " ".join(["\"$$(realpath menu/" + path + ")\"" for path in [
    "recipe_bank_edit.c", "recipe_bank_edit.h", "recipe_bank_storage.c",
    "recipe_bank_arm.S", "recipe_bank_fields.inc", "recipe_bank_ui.h",
]])
_PRIVATE = [
    "private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin",
    "private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin",
    "private-data/analysis/x-m1-1.50/lz/plain-01100000.bin",
    "private-data/analysis/x-m1-1.50/filmsim/patch-02/id4-patched.bin",
    "private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/package-field-marker/d3/id2-patched.bin",
    "private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/package-field-marker/d3/id4-patched.bin",
    "private-data/analysis/x-m1-1.50/filmsim/id2-00130000.bin",
    "private-data/analysis/classic-chrome-patch/candidate-build-1/patched-id4.bin",
    "private-data/analysis/classic-chrome-patch/candidate-build-1/patched-id5.bin",
    "private-data/analysis/classic-chrome-patch/candidate-build-1/patched-plain.bin",
    "private-data/analysis/classic-chrome/candidate.json",
]

_ENVIRONMENT_INPUTS = {
    "spec.json": "SYNTHETIC_SPEC",
    "image4.bin": "SYNTHETIC_IMAGE4",
    "image5.bin": "SYNTHETIC_IMAGE5",
    "plain.bin": "SYNTHETIC_PLAIN",
    "service.a": "SYNTHETIC_SERVICE_ARCHIVE",
    "menu-tables.json": "SYNTHETIC_MENU_TABLES",
    "modal75.rs": "SYNTHETIC_MODAL75_TABLE",
    "sdk-qualification.json": "SYNTHETIC_SDK_QUALIFICATION",
}

def _synthetic_inputs_impl(ctx):
    environment = {name: ctx.os.environ.get(name, "") for name in _ENVIRONMENT_INPUTS.values() + ["SYNTHETIC_SOURCE_ROOT"]}
    if not any(environment.values()):
        outputs = _ENVIRONMENT_INPUTS.keys() + ["source/" + path for path in _PRIVATE] + ["provenance.txt"]
        ctx.file("BUILD.bazel", "genrule(name = \"unconfigured\", outs = %s, cmd = \"echo 'synthetic source inputs require the SYNTHETIC_* --repo_env paths' >&2; exit 1\", visibility = [\"//visibility:public\"])\n" % repr(outputs))
        return
    for name, value in environment.items():
        if not value.startswith("/"):
            fail("%s requires an absolute source path" % name)
    files = {path: environment[name] for path, name in _ENVIRONMENT_INPUTS.items()}
    files.update({"source/" + path: environment["SYNTHETIC_SOURCE_ROOT"] + "/" + path for path in _PRIVATE})
    for relative, absolute in files.items():
        source = ctx.path(absolute)
        ctx.watch(source)
        if not source.exists or source.is_dir:
            fail("regular source file required: %s" % absolute)
        if str(source.dirname.realpath.get_child(source.basename)) != str(source.realpath):
            fail("source leaf must not be a symlink: %s" % absolute)
        ctx.symlink(source, relative)
    ctx.file("provenance.txt", json.encode(environment) + "\n")
    ctx.file("BUILD.bazel", "exports_files(%s, visibility = [\"//visibility:public\"])\n" % repr(files.keys() + ["provenance.txt"]))

_synthetic_inputs = repository_rule(
    implementation = _synthetic_inputs_impl,
    environ = _ENVIRONMENT_INPUTS.values() + ["SYNTHETIC_SOURCE_ROOT"],
    local = True,
)

def _synthetic_source_inputs_impl(_ctx):
    _synthetic_inputs(name = "candidate_synthetic_inputs")

synthetic_source_inputs = module_extension(implementation = _synthetic_source_inputs_impl)

def synthetic_combined_host_source_fixture(name):
    """Builds and exercises source-bound composition without DAT admission."""
    imported = name + "-inputs"
    environment_inputs = _ENVIRONMENT_INPUTS.keys()
    input_names = environment_inputs + ["provenance.txt"] + ["source/" + path for path in _PRIVATE]
    for path in input_names:
        native.alias(name = imported + "/" + path, actual = "@candidate_synthetic_inputs//:" + path, tags = ["manual"])
    native.filegroup(name = imported, srcs = [":" + imported + "/" + path for path in input_names], tags = ["manual"])
    preliminary = name + "-prepublication"
    native.genrule(
        name = preliminary,
        srcs = _SOURCES + [":" + imported + "/" + path for path in input_names],
        outs = [preliminary + "/" + path for path in ["property-prepublication.json", "runtime.bin", "kernel.elf", "recipe-edit-getter.ld", "synthetic-binding.json", "bank.a", "menu-tree.rs", "menu-records.rs", "menu-labels.rs"]],
        tools = _TOOLS,
        cmd = "\n".join([
            "set -eu; umask 077; here=$$PWD; root=$$(mktemp -d); trap 'rm -rf \"$$root\"' EXIT",
            "out=$$root/private-data/scratch/synthetic; mkdir -p $$out/seed",
            "for source in " + " ".join(["$(locations %s)" % source for source in _SOURCES]) + "; do mkdir -p $$root/$$(dirname $$source); test -e $$root/$$source || ln -s $$here/$$source $$root/$$source; done",
            " ".join(["mkdir -p $$root/$$(dirname %s); ln -s $$here/$(location :%s/source/%s) $$root/%s;" % (path, imported, path, path) for path in _PRIVATE]),
            "cd $$root",
            " ".join([
                "export RB_PROPERTY_PUBLICATION=1 RB_PUBLICATION_GROUPS=modal75,menu-engine,render",
                "RAM_PUSH_BUILDER=$$here/$(location //host/firmware:build-recipe-bank)",
                "RB_RUST_BUILD_FACTS=$$here/$(location @lab_xm1_rust//:share/xm1-rust/build-facts.txt)",
                "RAM_PUSH_CLANG=$$(dirname $$(realpath $$here/$(location @lab_clang//:bin/clang)))/clang",
                "RAM_PUSH_LD_LLD=$$(dirname $$(realpath $$here/$(location @lab_lld//:bin/ld.lld)))/ld.lld",
                "RAM_PUSH_OBJCOPY=$$(dirname $$(realpath $$here/$(location @lab_llvm//:bin/llvm-objcopy)))/llvm-objcopy",
                "RAM_PUSH_NM=$$(dirname $$(realpath $$here/$(location @lab_llvm//:bin/llvm-nm)))/llvm-nm",
                "RAM_PUSH_OBJDUMP=$$(dirname $$(realpath $$here/$(location @lab_llvm//:bin/llvm-objdump)))/llvm-objdump",
                "SPEC=$$here/$(location :%s/spec.json)" % imported,
                "RC0_ID4=$$here/$(location :%s/image4.bin)" % imported,
                "RC0_ID5=$$here/$(location :%s/image5.bin)" % imported,
                "RC0_PLAIN=$$here/$(location :%s/plain.bin)" % imported,
            ]),
            "pinned_path=$$(dirname $$(realpath $$here/$(location @lab_coreutils//:bin/sha256sum)))",
            "PATH=$$pinned_path RB_RUST_ARCHIVE=$$here/$(location :%s/service.a) $$here/$(location @lab_bash//:bin/bash) host/firmware/build-recipe-bank-ram.sh 0x02000000 $$out/seed" % imported,
            "$$here/$(location //replacement/modal_runtime:menu-engine-tree) --tables $$here/$(location :%s/menu-tables.json) --package-plan $$out/seed/bank-plan.json --tree-out $$out/menu-tree.rs --records-out $$out/menu-records.rs --labels-out $$out/menu-labels.rs" % imported,
            "RB_MODAL75_TABLE=$$here/$(location :%s/modal75.rs) RB_MENU_ENGINE_TREE=$$out/menu-tree.rs RB_MENU_ENGINE_RECORDS=$$out/menu-records.rs $$here/$(location @lab_xm1_rust//:bin/rustc) --crate-name=bank --edition=2021 --crate-type=staticlib -C opt-level=z --target=armv7r-none-eabi -C panic=abort -C relocation-model=static -C target-cpu=cortex-r4f -C target-feature=+strict-align -C codegen-units=1 -C debuginfo=0 -C llvm-args=-min-jump-table-entries=4096 --cfg rb_modal_scoped --cfg rb_modal75_owned --cfg rb_menu_engine_owned --cfg rb_render_owned replacement/bank/lib.rs -o $$out/bank.a" % imported,
            "PATH=$$pinned_path RB_RUST_ARCHIVE=$$out/bank.a $$here/$(location @lab_bash//:bin/bash) host/firmware/build-recipe-bank-ram.sh 0x02000000 $$out",
            "mkdir $$out/recheck; $$here/$(location //replacement/modal_runtime:menu-engine-tree) --tables $$here/$(location :%s/menu-tables.json) --package-plan $$out/bank-plan.json --tree-out $$out/recheck/menu-tree.rs --records-out $$out/recheck/menu-records.rs --labels-out $$out/recheck/menu-labels.rs" % imported,
            "for file in menu-tree.rs menu-records.rs menu-labels.rs; do cmp $$out/$$file $$out/recheck/$$file; done",
            "$$here/$(location %s) bind $$out/property-prepublication.json %s $$root/binding" % (_VALIDATOR, _EDIT),
            "mkdir -p $$here/$(RULEDIR)/" + preliminary + "; cp $$out/property-prepublication.json $$out/runtime.bin $$out/kernel.elf $$out/bank.a $$out/menu-tree.rs $$out/menu-records.rs $$out/menu-labels.rs $$root/binding/* $$here/$(RULEDIR)/" + preliminary + "/",
        ]),
        tags = ["manual"],
    )
    recipe = name + "-recipe"
    property_service_recipe_image(
        name = recipe,
        recipe_binding = ":" + preliminary + "/recipe-edit-getter.ld",
        tags = ["manual"],
        visibility = ["//visibility:public"],
    )
    output_names = ["runtime.bin", "kernel.elf", "recipe.elf", "recipe.bin", "recipe.symbols", "publication-table.bin", "recipe-edit-getter.ld", "synthetic-manifest.json", "runtime-source.json", "synthetic-proof.json", "bank.a", "menu-tree.rs", "menu-records.rs", "menu-labels.rs"]
    native.genrule(
        name = name,
        srcs = _SOURCES + [":" + imported, ":" + preliminary, ":" + recipe] +
               [":" + imported + "/" + path for path in input_names] +
               [":" + recipe + suffix for suffix in [".bin", ".symbols", ".elf"]] + [
            "//replacement/property_service:property-service-image.bin", "//replacement/property_service:property-service-image.symbols", "//replacement/property_service:property-service-image.elf"],
        outs = [name + "/" + path for path in output_names],
        tools = _TOOLS,
        cmd = "\n".join([
            "set -eu; umask 077; here=$$PWD; root=$$(mktemp -d); trap 'rm -rf \"$$root\"' EXIT",
            "mkdir -p $$root/preliminary",
            "cp $(locations :%s) $$root/preliminary/; chmod 600 $$root/preliminary/*" % preliminary,
            "$(location %s) complete $$root/preliminary/property-prepublication.json \"$$(realpath $(location :%s/image4.bin))\" \"$$(realpath $(location :%s/source/private-data/analysis/x-m1-1.50/filmsim/id2-00130000.bin))\" \"$$(realpath $(location :%s/image5.bin))\" \"$$(realpath $(location :%s/plain.bin))\" \"$$(realpath $(location //replacement/property_service:property-service-image.bin))\" \"$$(realpath $(location //replacement/property_service:property-service-image.symbols))\" \"$$(realpath $(location //replacement/property_service:property-service-image.elf))\" \"$$(realpath $(location :%s.bin))\" \"$$(realpath $(location :%s.symbols))\" \"$$(realpath $(location :%s.elf))\" \"$$(realpath $(location :%s/sdk-qualification.json))\" $$root/completed %s %s" % (
                _VALIDATOR, imported, imported, imported, imported, recipe, recipe, recipe, imported, _EDIT,
                " ".join(["$$(realpath $(locations %s))" % source for source in _SOURCES + [":" + imported, ":" + preliminary] + _TOOLS])),
            "mkdir -p $(RULEDIR)/" + name + "; cp $$root/completed/* $(RULEDIR)/" + name + "/",
        ]),
        tags = ["manual"],
        visibility = ["//visibility:public"],
    )
