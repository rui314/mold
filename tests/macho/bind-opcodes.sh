#!/bin/bash
source "$(dirname "$0")"/common.inc

# Classic dyld info binds imports with a small opcode program, sorted by
# library ordinal (flat lookups, -2, first), symbol, addend and address
# so that each library and symbol is set once. dyld binds each pointer
# to its symbol plus addend. objdump decodes the binds: Xcode 26's
# dyld_info -fixups (dyld-1267) misreads a symbol bound with two
# addends, in ld-prime's output too, giving later binds the next one's
# target or none, and crashes on some.
cat <<EOF | $CC -o $t/lib.o -c -xc -
char zvar[16];
EOF
$CC --ld-path=$mold -o $t/libz.dylib -shared $t/lib.o -Wl,-install_name,@rpath/libz.dylib

cat <<EOF | $CC -o $t/a.o -c -xc - -mmacosx-version-min=11.0
#include <stdlib.h>
extern int dynsym_b, dynsym_a;
extern char zvar[];
void *p1[] = { &dynsym_b, zvar + 8, (void *)&free, zvar, &dynsym_a, zvar + 8, (void *)&abort };
void *rep[] = { (void *)&free, (void *)&free, (void *)&free, 0, (void *)&free, 0,
                (void *)&free, 0, (void *)&free };
int main() { return p1[0] != rep[0]; }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/libz.dylib -mmacosx-version-min=11.0 \
  -Wl,-undefined,dynamic_lookup
p1=0x$(nm $t/exe | awk '$3 == "_p1" { print $1 }')
rep=0x$(nm $t/exe | awk '$3 == "_rep" { print $1 }')
objdump --macho --bind $t/exe | awk '$4 == "pointer" { print $3, $6 "/" $7, $5 }' | sort > $t/binds
{
  b() { printf '0x%x %s %d\n' $(($1 + $2)) $3 $4; }
  b $p1 0 flat-namespace/_dynsym_b 0
  b $p1 8 libz/_zvar 8
  b $p1 16 libSystem/_free 0
  b $p1 24 libz/_zvar 0
  b $p1 32 flat-namespace/_dynsym_a 0
  b $p1 40 libz/_zvar 8
  b $p1 48 libSystem/_abort 0
  for off in 0 8 16 32 48 64; do b $rep $off libSystem/_free 0; done
} | sort > $t/expected
diff $t/expected $t/binds

dyld_info -opcodes $t/exe | sed -n '/^ *bind opcodes:/,/^ *lazy bind opcodes:/p' > $t/ops
grep -q 'SET_DYLIB_SPECIAL_IMM(-2)' $t/ops
[ "$(grep -c 'SET_SYMBOL_TRAILING_FLAGS_IMM(0x00, _free)' $t/ops)" = 1 ]
