/* Linker script for the compiled-recipe commit hook blob
 * (menu/recipe_compiled.S). The runtime worker passes the hook placement with
 * --defsym=RC_BASE=<hook address>; the table and commit symbols come from
 * menu/recipe_compiled_layout.S, which every link that consumes
 * recipe_compiled.o carries, so the blob's branch targets and its table
 * references cannot disagree with the manifest
 * host/firmware/BuildCompiledRecipes.hs emits
 * (reports/recipe-patch-d3.md sections 3 and 5.2). */
ENTRY(rc_hook)
SECTIONS {
  . = RC_BASE;
  .text : { *(.text) *(.text.*) }
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) *(.rodata*) *(.data*) *(.bss*) *(COMMON) }
}
ASSERT(rc_hook == RC_BASE, "hook blob starts at RC_BASE")
ASSERT(SIZEOF(.text) == 76, "compiled-recipe hook is 19 words")
