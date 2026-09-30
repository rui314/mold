#!/bin/bash
source "$(dirname "$0")"/common.inc

# Classic dyld info binds imports with a small opcode program. ld64
# sorts the binds by library ordinal (flat lookups, -2, first), symbol,
# addend and address, sets each piece of state only when it changes,
# steps the address by ADD_ADDR_ULEB within a segment - backwards too -
# and packs a bind plus a step into one opcode, a run of equal steps
# into DO_BIND_ULEB_TIMES_SKIPPING_ULEB. ld-prime writes the same bytes.
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
dyld_info -opcodes $t/exe | sed -n '/^ *bind opcodes:/,/^ *lazy bind opcodes:/p' > $t/ops
[ "$(grep -o 'FLAGS_IMM(0x00, [_a-z]*' $t/ops | cut -d' ' -f2 | tr '\n' ' ')" = \
  '_dynsym_a _dynsym_b _zvar _abort _free ' ]
grep -q 'SET_DYLIB_SPECIAL_IMM(-2)' $t/ops
grep -q 'BIND_OPCODE_ADD_ADDR_ULEB(0xFFFFFFFFFFFFFF' $t/ops
grep -q 'DO_BIND_ULEB_TIMES_SKIPPING_ULEB' $t/ops
grep -q 'DO_BIND_ADD_ADDR_IMM_SCALED' $t/ops
[ "$(grep -c 'SET_SYMBOL_TRAILING_FLAGS_IMM(0x00, _free)' $t/ops)" = 1 ]
