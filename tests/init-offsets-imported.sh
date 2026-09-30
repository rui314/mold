#!/bin/bash
source "$(dirname "$0")"/common.inc

# An initializer that dyld binds - a library's function, or one left
# to dynamic lookup - has no offset in the image, so __init_offsets
# can't hold it: ld-prime fails the link as it writes the offsets, with
# a fixup error of the first such in its "inits-file", whose atoms name
# the k-th initializer's offset anon-(2k+1). __mod_init_func's
# absolute pointers (-no_fixup_chains) bind such an initializer.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __DATA,__mod_init_func,mod_init_funcs
.p2align 3
.quad _init
.quad _init
.quad _undef
.text
.p2align 2
_init: ret
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __DATA,__mod_init_func,mod_init_funcs
.p2align 3
.quad _init2
.quad _puts
.quad _printf
.text
.globl _main
.p2align 2
_init2: ret
_main: ret
.subsections_via_symbols
EOF

not $CC --ld-path=$mold -o $t/exe $t/b.o 2> $t/log
grep -q "fixup error (kind=imageOffset32) at 'anon-3' from inits-file, target '_puts' does not have address" $t/log
not grep -q _printf $t/log

not $CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-U,_undef 2> $t/log
grep -q "at 'anon-5' from inits-file, target '_undef' does not have address" $t/log

not $CC --ld-path=$mold -o $t/exe $t/b.o $t/a.o -Wl,-U,_undef 2> $t/log
grep -q "at 'anon-3' from inits-file, target '_puts' does not have address" $t/log

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-U,_undef -Wl,-no_fixup_chains
otool -l $t/exe | grep -q 'sectname __mod_init_func'
