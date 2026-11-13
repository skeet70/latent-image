"""Declared owner-supplied inputs for the bank image's generated tables.

The menu tables, package plan, image 4, image 5, controller-75 inventory and
picker document are private inputs. They are declared here so the table
generators run as build actions with watched inputs instead of the adapter
crates reading a private-data path at compile time.

Set RB_MENU_TABLES, RB_BANK_PLAN, RB_IMAGE4, RB_IMAGE5,
RB_CONTROLLER75_INVENTORY and RB_PICKER_DOCUMENT with --repo_env to override
the conventional private-data paths. When a required input is absent the
repository exposes a target that fails with the missing paths, so the image is
not buildable rather than silently different.
"""

_INPUTS = {
    "menu-tables.json": ("RB_MENU_TABLES", "private-data/scratch/ControllerStrategy/menu-tables.json"),
    "bank-plan.json": ("RB_BANK_PLAN", "private-data/scratch/ControllerStrategy/menu-engine/bank-plan.json"),
    "image4.bin": ("RB_IMAGE4", "private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin"),
    "image5.bin": ("RB_IMAGE5", "private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin"),
    "controller75-inventory.json": ("RB_CONTROLLER75_INVENTORY", "private-data/scratch/ControllerPilot75/inventory.json"),
    "picker-document.json": ("RB_PICKER_DOCUMENT", "private-data/scratch/ControllerStrategy/picker-fold/picker-document.json"),
}

def _bank_image_inputs_impl(ctx):
    environment = {name: ctx.os.environ.get(variable, "") for name, (variable, _) in _INPUTS.items()}
    files = {}
    missing = []
    for name, (_, default) in _INPUTS.items():
        path = ctx.path(environment[name] or (str(ctx.workspace_root) + "/" + default))
        if not path.exists or path.is_dir:
            missing.append(str(path))
        else:
            files[name] = path
    if missing:
        ctx.file(
            "BUILD.bazel",
            "genrule(name = \"unconfigured\", outs = %r, cmd = \"echo 'bank image inputs are missing: %s' >&2; exit 1\", visibility = [\"//visibility:public\"])\n" %
            (sorted(_INPUTS.keys()), " ".join(missing)),
        )
        return
    for name, path in files.items():
        ctx.watch(path)
        ctx.symlink(path, name)
    ctx.file("BUILD.bazel", "exports_files(%r, visibility = [\"//visibility:public\"])\n" % sorted(files.keys()))

_bank_image_inputs = repository_rule(
    implementation = _bank_image_inputs_impl,
    environ = [variable for variable, _ in _INPUTS.values()],
    local = True,
)

def _bank_image_inputs_extension_impl(_ctx):
    _bank_image_inputs(name = "bank_image_inputs")

bank_image_inputs = module_extension(implementation = _bank_image_inputs_extension_impl)
