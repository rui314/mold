#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime makes each initializer or terminator pointer a subsection of
# its own: its diagnostics name the pointer's subsection (anon-N, the
# Nth of the object's subsections), each needs a relocation of its own,
# and a -r output lists the pointers' relocations in their order.
cat <<EOF | $CC -o $t/main.o -c -xc -
int main() { return 0; }
EOF

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __DATA,__mod_init_func,mod_init_funcs
.p2align 3
.quad _f
.long 0
.quad _g
.long 0
.text
.globl _f, _g
.p2align 2
_f: ret
_g: ret
.subsections_via_symbols
EOF
not $CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o 2> /dev/null

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__myinit,mod_init_funcs
.p2align 3
.quad _f
.quad _g
.text
.globl _f, _g
.p2align 2
_f: ret
_g: ret
.subsections_via_symbols
EOF
not $CC --ld-path=$mold -o $t/exe $t/main.o $t/b.o -Wl,-no_fixup_chains 2> $t/log
grep -q "text-relocation in 'anon-3' (.*/b.o) to '_g'" $t/log

cat <<EOF | $CC -o $t/c.o -c -xassembler -
.section __DATA,__mod_init_func,mod_init_funcs
.p2align 3
.quad _f
.quad _g
.text
.globl _f, _g
.p2align 2
_f: ret
_g: ret
.subsections_via_symbols
EOF
$mold -r -arch $ARCH -o $t/r.o $t/c.o
objdump --macho -r $t/r.o | grep -A3 __mod_init_func | tail -2 | awk '{print $1}' > $t/relocs
printf '00000000\n00000008\n' | cmp - $t/relocs
