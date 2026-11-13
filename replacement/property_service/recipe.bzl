"""Optional recipe image linkage from a package-generated linker binding.

The binding file is the exact output of BankPackage.recipeEditLinkerInclude.
RecipeArtifact admission must bind the resulting image to that same package.
Neither linkage nor static admission establishes installed callback lifetime.
"""

load("//replacement/toolchain:rust.bzl", "xm1_rust_elf")

def property_service_recipe_image(name, recipe_binding, visibility = None, tags = []):
    """Links the recipe variant using a caller-supplied generated package binding."""
    xm1_rust_elf(
        name = name,
        rust_staticlib = "//replacement/property_service:property-service-recipe-edit-rust",
        asm_srcs = ["//replacement/property_service:entry.S", "//replacement/property_service:storage-boundary.S"],
        entry = "xm1_property_get_entry",
        link_script = "//replacement/property_service:property-service.ld",
        link_includes = [recipe_binding],
        extra_ldflags = ["-Wl,--gc-sections", "-Wl,--emit-relocs", "-Wl,-T,$(location %s)" % recipe_binding],
        tags = tags,
        visibility = visibility,
    )
