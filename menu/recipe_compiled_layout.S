@ The compiled-recipe hook's absolute layout symbols
@ (reports/recipe-patch-d3.md sections 3 and 5.2). menu/recipe_compiled.S
@ references them through link relocations, so the definitions must reach
@ every link that consumes recipe_compiled.o: this object carries them as
@ global absolute symbols, and menu/recipe_compiled.ld no longer assigns them
@ (a link taking both would see duplicate symbols). The values are the prefix
@ layout's own; the emit's patch/ABI check refuses a hook blob whose words do
@ not resolve to them.
.syntax unified

.global rc_row_tables
.global rc_props
.global menu_commit
.set rc_row_tables, 0x0061c38c
.set rc_props, 0x0061c39c
.set menu_commit, 0x00d4fc74
