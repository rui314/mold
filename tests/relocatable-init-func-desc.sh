#!/bin/bash
source "$(dirname "$0")"/common.inc

# Dead stripping keeps the initializer and terminator pointer lists
# whatever their attributes, and a -r output says so for the next link:
# ld-prime marks their symbols N_NO_DEAD_STRIP, as it marks those of a
# no_dead_strip section.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _f
_f: ret
.section __DATA,__mod_init_func,mod_init_funcs
.p2align 3
init_ptr: .quad _f
.section __DATA,__mod_term_func,mod_term_funcs
.p2align 3
term_ptr: .quad _f
.data
dd: .quad 1
.subsections_via_symbols
EOF
$mold -arch $ARCH -r $t/a.o -o $t/r.o
nm -m $t/r.o > $t/nm
grep -q '\[no dead strip\] init_ptr$' $t/nm
grep -q '\[no dead strip\] term_ptr$' $t/nm
not grep -q 'dead strip\] dd$' $t/nm
