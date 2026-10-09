#!/bin/bash
source "$(dirname "$0")"/common.inc

# A tentative definition (a common symbol) loses to any real definition,
# and a live file's then loads the archive member that has one, as mold
# does: in a link with a.o, e.a's b.o is loaded for foo and f.a's f.o for
# bar. A member's own tentative definition loses to a live file's, so
# c.o is not loaded for bar, but it is still a definition that a
# reference loads its member for: d.o for baz.
cat <<EOF | $CC -fcommon -o $t/a.o -c -xc -
#include <stdio.h>
int foo;
int bar;
extern int baz;
int main() { printf("%d %d %d\n", foo, bar, baz); }
EOF
echo 'int foo = 5;' | $CC -o $t/b.o -c -xc -
printf 'int bar;\nint c_member = 1;\n' | $CC -fcommon -o $t/c.o -c -xc -
echo 'int baz;' | $CC -fcommon -o $t/d.o -c -xc -
printf 'int bar = 3;\nint f_member = 1;\n' | $CC -o $t/f.o -c -xc -
rm -f $t/e.a $t/f.a
ar rcs $t/e.a $t/b.o $t/c.o $t/d.o
ar rcs $t/f.a $t/f.o

$CC --ld-path=$mold -o $t/exe1 $t/a.o $t/e.a
$RUN $t/exe1 | grep -q '^5 0 0$'
nm $t/exe1 > $t/log1
not grep -q _c_member $t/log1

$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/f.a $t/e.a
$RUN $t/exe2 | grep -q '^5 3 0$'
nm $t/exe2 > $t/log2
grep -q _f_member $t/log2
not grep -q _c_member $t/log2

# A member's real definition beats a live tentative definition though a
# dylib that defines the symbol comes first: the tentative definition
# hides the dylib's but under -commons use_dylibs (see commons.sh).
cat <<EOF | $CC -o $t/x.o -c -xassembler -
.comm _x,16,4
EOF
cat <<EOF | $CC -o $t/data.o -c -xassembler -
.data
.globl _x, _data_member
_data_member: .long 0
_x: .long 42
EOF
cat <<EOF | $CC -o $t/dylib.o -c -xassembler -
.data
.globl _x
_x: .quad 0
EOF
rm -f $t/libdata.a
ar rcs $t/libdata.a $t/data.o
$CC --ld-path=$mold -shared -o $t/libx.dylib $t/dylib.o

$CC --ld-path=$mold -shared -o $t/c.dylib $t/x.o $t/libx.dylib $t/libdata.a
nm -m $t/c.dylib > $t/log3
grep -q ' (__DATA,__data) external _x$' $t/log3
grep -q ' _data_member$' $t/log3

# ld-prime differs: for a live tentative definition it loads only a
# member that defines the symbol as data, never one that defines it in
# code, and it ranks a member's tentative definition like a real one
# in archive order, so a reference loads its member first, which then
# wants a member with data, and a dylib that comes later loses to it.
cat <<EOF | $CC -o $t/code.o -c -xassembler -
.text
.globl _x, _code_member
_code_member: ret
_x: ret
EOF
cat <<EOF | $CC -o $t/ref.o -c -xassembler -
.data
.globl _ref
.p2align 3
_ref: .quad _x
EOF
cat <<EOF | $CC -o $t/tent.o -c -xassembler -
.comm _x,32,3
.data
.globl _tent_member
_tent_member: .long 0
EOF
rm -f $t/libcode.a $t/libtent.a
ar rcs $t/libcode.a $t/code.o
ar rcs $t/libtent.a $t/tent.o

if is_mold; then
  # The member's code beats the tentative definition (ld-prime keeps
  # the tentative definition).
  $CC --ld-path=$mold -shared -o $t/d.dylib $t/x.o $t/libcode.a
  nm -m $t/d.dylib > $t/log4
  grep -q ' (__TEXT,__text) external _x$' $t/log4
  grep -q ' _code_member$' $t/log4

  # A reference loads the member with code or data, not the one with a
  # tentative definition (ld-prime loads tent.o and keeps its tentative
  # definition over code, or loads it and data.o).
  $CC --ld-path=$mold -shared -o $t/e.dylib $t/ref.o $t/libtent.a $t/libcode.a
  nm -m $t/e.dylib > $t/log5
  grep -q ' (__TEXT,__text) external _x$' $t/log5
  not grep -q _tent_member $t/log5

  $CC --ld-path=$mold -shared -o $t/f.dylib $t/ref.o $t/libtent.a $t/libdata.a
  nm -m $t/f.dylib > $t/log6
  grep -q ' (__DATA,__data) external _x$' $t/log6
  grep -q ' _data_member$' $t/log6
  not grep -q _tent_member $t/log6

  # A dylib's definition beats a member's tentative one, whatever the
  # order (ld-prime loads tent.o).
  $CC --ld-path=$mold -shared -o $t/g.dylib $t/ref.o $t/libtent.a $t/libx.dylib
  nm -m $t/g.dylib > $t/log7
  grep -q '(undefined) external _x (from libx)' $t/log7
  not grep -q _tent_member $t/log7
fi
