#!/bin/bash
. $(dirname $0)/common.inc

# .sframe is a compact stack-unwinding format. The linker parses the
# .sframe sections in the input files, drops the entries for dead
# functions, concatenates the rest, sorts the resulting index by PC and
# rewrites the header. mold handles SFrame Version 3.

# Skip if the assembler cannot emit SFrame Version 3.
cat <<EOF | $CC -Wa,--gsframe -o $t/probe.o -c -xc - 2>/dev/null || skip
int main() { return 0; }
EOF
readelf --sframe=.sframe $t/probe.o 2>/dev/null | grep -q SFRAME_VERSION_3 || skip

# Compile two translation units, each with several functions, so that the
# linker has to merge and sort more than one .sframe section.
cat <<EOF | $CC -O1 -Wa,--gsframe -o $t/a.o -c -xc -
int foo(int x) { return x + 1; }
int bar(int x) { return foo(x) * 2; }
EOF

cat <<EOF | $CC -O1 -Wa,--gsframe -o $t/b.o -c -xc -
int foo(int);
int bar(int);
int baz(int x) { return foo(x) - bar(x); }
int main(void) { return baz(7) + 8; }
EOF

$CC -B. -Wa,--gsframe -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe

# The output is a single Version 3 section whose index is sorted by PC.
readelf --sframe=.sframe $t/exe > $t/log
grep -q SFRAME_VERSION_3 $t/log
grep -q SFRAME_F_FDE_SORTED $t/log

# A PT_GNU_SFRAME segment must point at the section.
readelf -Wl $t/exe | grep -q GNU_SFRAME

# Verify that the FDEs really are sorted by function address.
prev=0
for pc in $(grep -oE 'pc = 0x[0-9a-f]+' $t/log | grep -oE '0x[0-9a-f]+'); do
  [ $((pc)) -ge $prev ]
  prev=$((pc))
done

# --gc-sections must drop the FDEs of collected functions.
cat <<EOF | $CC -O0 -ffunction-sections -Wa,--gsframe -o $t/c.o -c -xc -
int used(int x) { return x; }
int unused(int x) { return x + 1; }
int main(void) { return used(0); }
EOF

$CC -B. -Wa,--gsframe -ffunction-sections -o $t/exe2 $t/c.o
$CC -B. -Wa,--gsframe -ffunction-sections -Wl,--gc-sections -o $t/exe3 $t/c.o
$QEMU $t/exe3

n1=$(readelf --sframe=.sframe $t/exe2 | grep -c 'func idx')
n2=$(readelf --sframe=.sframe $t/exe3 | grep -c 'func idx')
[ "$n2" -lt "$n1" ]

# A relocatable link (-r) must merge the inputs into a single .sframe and
# keep each func_start as a relocation for the final link to resolve.
./mold --relocatable -o $t/r.o $t/a.o $t/b.o
readelf --sframe=.sframe $t/r.o > $t/rlog
grep -q SFRAME_VERSION_3 $t/rlog
[ "$(readelf -SW $t/r.o | grep -cw GNU_SFRAME)" = 1 ]
readelf -SW $t/r.o | grep -q '\.rela\.sframe'

# The relocatable object must link into a working executable that still
# describes every function from the original inputs.
$CC -B. -Wa,--gsframe -o $t/exe4 $t/r.o
$QEMU $t/exe4
[ "$(grep -c 'func idx' $t/rlog)" -le "$(readelf --sframe=.sframe $t/exe4 | grep -c 'func idx')" ]
