#!/usr/bin/env bash
. $(dirname $0)/common.inc

on_qemu && skip
[ $MACHINE = riscv64 -o $MACHINE = riscv32 -o $MACHINE = sparc64 ] && skip
command -v gdb >& /dev/null || skip
command -v ${GCC%% *} >& /dev/null || skip

# Regression test for https://github.com/rui314/mold/issues/1634.
cat <<EOF > $t/shared.h
struct Point { int x; int y; };
struct Shape { struct Point a; struct Point b; double area; };
int use_shape(struct Shape *s);
int make_shape(struct Shape *s);
EOF

cat <<EOF > $t/a.c
#include "shared.h"
int use_shape(struct Shape *s) { return s->a.x + s->b.y; }
EOF

cat <<EOF > $t/b.c
#include "shared.h"
int make_shape(struct Shape *s) {
  s->a.x = 1;
  s->b.y = 2;
  s->area = 3.0;
  return use_shape(s);
}
int main() { struct Shape s; return make_shape(&s); }
EOF

$GCC -g -ggnu-pubnames -gdwarf-5 -fdebug-types-section -c $t/a.c -o $t/a.o || skip
$GCC -g -ggnu-pubnames -gdwarf-5 -fdebug-types-section -c $t/b.c -o $t/b.o

# Clang accepts -fdebug-types-section but does not emit type units.
readelf --debug-dump=info $t/a.o | grep -F DW_UT_type || skip

$GCC -B. -Wl,--gdb-index $t/a.o $t/b.o -o $t/exe
readelf --debug-dump=gdb_index $t/exe > $t/index

grep -A4 '^TU table:' $t/index | grep -E '^\[ *0\]'
grep -A4 '^TU table:' $t/index | grep -E '^\[ *1\]'

DEBUGINFOD_URLS= gdb $t/exe -nx -batch -ex 'ptype struct Shape' -ex quit >& $t/log
grep -F 'type = struct Shape {' $t/log
grep -F 'struct Point a;' $t/log
grep -F 'double area;' $t/log

# A partial link combines all pubnames sets and their relocations into one
# section. Make sure each set is still matched to its CU or TU contribution.
./mold -r -o $t/ab.o $t/a.o $t/b.o
$GCC -B. -Wl,--gdb-index $t/ab.o -o $t/exe-r
readelf --debug-dump=gdb_index $t/exe-r > $t/index-r
grep -A4 '^TU table:' $t/index-r | grep -E '^\[ *0\]'
grep -A4 '^TU table:' $t/index-r | grep -E '^\[ *1\]'

# Exercise the 64-bit DWARF type-unit header when the compiler supports it.
if $GCC -g -ggnu-pubnames -gdwarf-5 -gdwarf64 -fdebug-types-section \
     -c $t/a.c -o $t/a64.o 2> /dev/null; then
  $GCC -g -ggnu-pubnames -gdwarf-5 -gdwarf64 -fdebug-types-section \
    -c $t/b.c -o $t/b64.o
  $GCC -B. -Wl,--gdb-index $t/a64.o $t/b64.o -o $t/exe64
  readelf --debug-dump=gdb_index $t/exe64 > $t/index64
  grep -A4 '^TU table:' $t/index64 | grep -E '^\[ *0\]'
  grep -A4 '^TU table:' $t/index64 | grep -E '^\[ *1\]'
fi
