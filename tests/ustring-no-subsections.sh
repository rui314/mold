#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime makes the atoms of UTF-16 literals by content and names none
# of them, but in an object without subsections the __ustring section
# is one atom, whose labels it keeps as those of any other section: in
# the symbol table and the map of a final image, and in a -r output
# (which marks the names of such a whole-section atom no-dead-strip).
cat <<'EOF' > $t/a.s
.text
.globl _main
_main:
  ret
.section __TEXT,__ustring
.p2align 1
_ustr_local:
  .short 0x48, 0x69, 0
lustr_temp:
  .short 0x41, 0
.globl _ustr_global
_ustr_global:
  .short 0x42, 0
.globl _ustr_pext
.private_extern _ustr_pext
_ustr_pext:
  .short 0x43, 0
.data
.p2align 3
_ptrs:
  .quad _ustr_local
  .quad lustr_temp
EOF
$CC -c -o $t/a.o $t/a.s

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-map,$t/map
nm -m $t/exe > $t/syms
grep -q '(__TEXT,__ustring) non-external _ustr_local$' $t/syms
grep -q '(__TEXT,__ustring) non-external (was a private external) _ustr_pext$' $t/syms
grep -q '(__TEXT,__ustring) external _ustr_global$' $t/syms
grep -Eq $'\t\\[  1\\] _ustr_local$' $t/map
not grep -q anon $t/map

$mold -r -arch $ARCH -o $t/r.o $t/a.o
nm -m $t/r.o > $t/syms2
grep -q '(__TEXT,__ustring) non-external \[no dead strip\] _ustr_local$' $t/syms2
grep -q '(__TEXT,__ustring) non-external \[no dead strip\] lustr_temp$' $t/syms2
grep -q 'non-external (was a private external) \[no dead strip\] _ustr_pext$' $t/syms2
