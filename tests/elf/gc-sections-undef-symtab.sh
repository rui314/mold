#!/usr/bin/env bash
. $(dirname $0)/common.inc

# An undefined symbol that only a section removed by --gc-sections refers to
# is left out of .symtab, as in GNU ld.
cat <<EOF | $CC -c -o $t/a.o -ffunction-sections -fdata-sections -xc -
void undef_fn(void);
void dead(void) { undef_fn(); }
int main() { return 0; }
EOF

$CC -B. -o $t/exe $t/a.o -Wl,--gc-sections
nm $t/exe | not grep undef_fn
