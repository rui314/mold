#!/bin/bash
source "$(dirname "$0")"/common.inc

# REFERENCED_DYNAMICALLY has strip(1) keep a symbol dyld looks up by
# name (crt1.o's _NXArgc, _environ and so on). ld-prime warns that it
# is deprecated on each exported non-weak definition in a section - not
# on a private extern, a local, an undefined, an absolute or a weak one -
# as it reads the object, an archive member it never loads too. The
# output's entry still carries it, in an executable and a dylib alike.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl _main, _g, _h, _p, _w, _abs
.desc _g, 0x10
.desc _h, 0x10
.desc _l, 0x10
.desc _u, 0x10
.desc _p, 0x10
.private_extern _p
.desc _w, 0x90
.desc _abs, 0x10
_abs = 0x1234
.text
_main: ret
.data
.p2align 3
_g: .quad 0
_h: .quad _u
_l: .quad 0
_p: .quad _l
_w: .quad 0
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.globl _unused
.desc _unused, 0x10
.data
_unused: .quad 0
.subsections_via_symbols
EOF
rm -f $t/libb.a
ar rcs $t/libb.a $t/b.o

$CC --ld-path=$mold -o $t/exe $t/a.o $t/libb.a -Wl,-U,_u 2> $t/log
grep -q "REFERENCED_DYNAMICALLY flag on symbol '_g' is deprecated" $t/log
grep -q "REFERENCED_DYNAMICALLY flag on symbol '_h' is deprecated" $t/log
grep -q "REFERENCED_DYNAMICALLY flag on symbol '_unused' is deprecated" $t/log
[ $(grep -c REFERENCED_DYNAMICALLY $t/log) = 3 ]

nm -m $t/exe > $t/syms
grep -q '\[referenced dynamically\] external _g$' $t/syms
grep -q '\[referenced dynamically\] external _h$' $t/syms
grep -q '\[referenced dynamically\] external __mh_execute_header$' $t/syms
[ $(grep -c 'referenced dynamically' $t/syms) = 3 ]

$CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -Wl,-U,_u 2> /dev/null
nm -m $t/b.dylib > $t/syms2
grep -q '\[referenced dynamically\] external _g$' $t/syms2
[ $(grep -c 'referenced dynamically' $t/syms2) = 2 ]
