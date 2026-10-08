#!/usr/bin/env bash
. $(dirname $0)/common.inc

# OneTBB isn't tsan-clean
nm mold | grep '__tsan_init' && skip

on_qemu && skip
[ $MACHINE = riscv64 -o $MACHINE = riscv32 -o $MACHINE = sparc64 ] && skip
command -v gdb >& /dev/null || skip
command -v ${GCC%% *} >& /dev/null || skip
command -v ${GXX%% *} >& /dev/null || skip

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

# Old GDB versions choke on DWARF 5 type units no matter how the program
# is linked; GDB 10 even crashes. Skip if GDB cannot debug an executable
# that doesn't contain a .gdb_index section.
$GCC -B. $t/a.o $t/b.o -o $t/exe-noindex
DEBUGINFOD_URLS= gdb $t/exe-noindex -nx -batch -ex 'ptype struct Shape' \
  -ex quit >& $t/log-noindex || skip
grep -Fq 'type = struct Shape {' $t/log-noindex || skip

# Prints the symbol table entries of a name in a readelf dump of an index.
symbol_entries() {
  awk -v name="$2" '/^\[/ { p = index($0, "] " name ":") } p' $1
}

check_index() {
  readelf --debug-dump=gdb_index $1 > $2 || true
  grep -F 'Version 9' $2 || return 1

  # Binutils 2.38 predates version 9 and cannot decode the tables. Newer
  # readelf versions should show both type units.
  if grep -q '^TU table:' $2; then
    grep -A4 '^TU table:' $2 | grep -E '^\[ *0\]' || return 1
    grep -A4 '^TU table:' $2 | grep -E '^\[ *1\]' || return 1

    # GCC emits no pubnames for type units, but GDB 18 finds a type only
    # if the index lists it for the type unit defining it.
    symbol_entries $2 Shape | grep -E 'T[0-9]+ \[static, type\]' || return 1
  fi
}

$GCC -B. -Wl,--gdb-index $t/a.o $t/b.o -o $t/exe
check_index $t/exe $t/index

DEBUGINFOD_URLS= gdb $t/exe -nx -batch -ex 'ptype struct Shape' -ex quit >& $t/log
grep -F 'type = struct Shape {' $t/log
grep -F 'struct Point a;' $t/log
grep -F 'double area;' $t/log

# A partial link combines all pubnames sets and their relocations into one
# section. Make sure each set is still matched to its CU or TU contribution.
./mold -r -o $t/ab.o $t/a.o $t/b.o
$GCC -B. -Wl,--gdb-index $t/ab.o -o $t/exe-r
check_index $t/exe-r $t/index-r

# Exercise the 64-bit DWARF type-unit header when the compiler supports it.
if $GCC -g -ggnu-pubnames -gdwarf-5 -gdwarf64 -fdebug-types-section \
     -c $t/a.c -o $t/a64.o 2> /dev/null; then
  $GCC -g -ggnu-pubnames -gdwarf-5 -gdwarf64 -fdebug-types-section \
    -c $t/b.c -o $t/b64.o
  $GCC -B. -Wl,--gdb-index $t/a64.o $t/b64.o -o $t/exe64
  check_index $t/exe64 $t/index64
fi

# C++ types are global symbols qualified with their enclosing scopes. GCC
# defines a nested class outside of the class declaring it.
cat <<EOF > $t/c.cc
namespace ns { struct Foo { int x; }; }
namespace { struct Bar { int y; }; }
struct Outer { struct Inner { int z; }; };
int main() { ns::Foo a{}; Bar b{}; Outer::Inner c{}; return a.x + b.y + c.z; }
EOF

$GXX -g -ggnu-pubnames -gdwarf-5 -fdebug-types-section -c $t/c.cc -o $t/c.o
$GXX -B. -Wl,--gdb-index $t/c.o -o $t/exe-cc
readelf --debug-dump=gdb_index $t/exe-cc > $t/index-cc || true

if grep -q '^TU table:' $t/index-cc; then
  for name in ns::Foo '(anonymous namespace)::Bar' Outer::Inner; do
    symbol_entries $t/index-cc "$name" | grep -E 'T[0-9]+ \[global, type\]'
  done
fi
