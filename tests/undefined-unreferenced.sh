#!/bin/bash
source "$(dirname "$0")"/common.inc

# An object may name a symbol undefined that nothing refers to: a
# .globl with neither a definition nor a relocation (XNU declares
# SleepToken that way in some configurations). ld-prime drops such a
# name without a word - no error, and no import under -undefined
# dynamic_lookup - while an undefined symbol that is referenced is
# still an error.
cat <<EOF | $CC -o $t/a.o -c -xc -
__asm__(".globl _nosuch");
int main() { return 0; }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe
nm $t/exe > $t/nm
not grep -q nosuch $t/nm

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-undefined,dynamic_lookup
$t/exe2
nm $t/exe2 > $t/nm2
not grep -q nosuch $t/nm2

cat <<EOF | $CC -o $t/b.o -c -xc -
int nosuch2(void);
int f(void) { return nosuch2(); }
EOF
not $CC --ld-path=$mold -o $t/exe3 $t/a.o $t/b.o 2> $t/log
grep -q '_nosuch2' $t/log
not grep -Eq '_nosuch([^2]|$)' $t/log
