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

# Only the name of each list entry's subsection is marked, though: of
# several labels at one place the one naming the subsection (the highest
# ranked, an external over locals, then by descending name), not its
# aliases - in an object without subsections too, where every other
# symbol is marked, the assembler's ltmpN labels included.
cat > $t/b.s <<EOF
.text
.globl _i1
.p2align 2
_i1: ret
.section __DATA,__mod_init_func,mod_init_funcs
.p2align 3
ca:
cb:
  .quad _i1
.globl _cg
_cg:
cl:
  .quad _i1
.section __DATA,__mod_term_func,mod_term_funcs
.p2align 3
ta:
tb:
  .quad _i1
.data
.p2align 3
da:
db:
  .quad 1
EOF
$CC -o $t/b.o -c $t/b.s
cat $t/b.s > $t/c.s
echo .subsections_via_symbols >> $t/c.s
$CC -o $t/c.o -c $t/c.s

for obj in b c; do
  $mold -arch $ARCH -r $t/$obj.o -o $t/r$obj.o
  nm -m $t/r$obj.o > $t/nm$obj
  grep -q '\[no dead strip\] cb$' $t/nm$obj
  grep -q '\[no dead strip\] _cg$' $t/nm$obj
  grep -q '\[no dead strip\] tb$' $t/nm$obj
  grep -q 'non-external ca$' $t/nm$obj
  grep -q 'non-external cl$' $t/nm$obj
  grep -q 'non-external ta$' $t/nm$obj
done
grep -q '\[no dead strip\] da$' $t/nmb
grep -q '\[no dead strip\] db$' $t/nmb
