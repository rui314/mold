#!/usr/bin/env bash
. $(dirname $0)/common.inc

# -entry=foo is --entry=foo, not -e ntry=foo.

# On PPC64, a given entry point address is set to .opd, and the
# address in .opd is set to the ELF header.
[ $MACHINE = ppc64 ] && skip

cat <<EOF | $CC -o $t/a.o -c -x assembler -
.globl foo
foo = 0x1000
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o -Wl,-entry=foo
readelf -e $t/exe1 > $t/log1
grep "Entry point address:.*0x1000$" $t/log1

$CC -B. -o $t/exe2 $t/a.o $t/b.o -Wl,-efoo
readelf -e $t/exe2 > $t/log2
grep "Entry point address:.*0x1000$" $t/log2

# GNU ld and lld accept --export-dynamic-symbol only with double dashes
# and read -export-dynamic-symbol as -e xport-dynamic-symbol. So does mold.
$CC -B. -o $t/exe3 $t/a.o $t/b.o -Wl,-export-dynamic-symbol=foo |&
  grep 'entry symbol is not defined: xport-dynamic-symbol=foo'
