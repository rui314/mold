#!/bin/bash
source "$(dirname "$0")"/common.inc

# arm64 compilers emit compact unwind for every frame, and a DWARF FDE
# beside it only at -O0 under -fasynchronous-unwind-tables (Swift's closure
# thunks get them; NetNewsWire's RSCore prelink carries 119 such
# atoms). A final link needs only the compact records, but ld-prime -r
# carries every input CIE and FDE through, unnamed, with the FDE fields
# recomputed as self-relative values and only the personality's GOT
# reference as a relocation.
cat <<EOF2 | $CXX -O0 -fasynchronous-unwind-tables -o $t/a.o -c -xc++ -
struct S { virtual ~S(); virtual int g(); };
S::~S() {}
int S::g() { try { throw 1; } catch (int) { return 2; } }
EOF2
cat <<EOF2 | $CXX -O0 -fasynchronous-unwind-tables -o $t/b.o -c -xc++ -
#include <cstdio>
struct S { virtual ~S(); virtual int g(); };
int main() { S s; printf("%d\n", s.g() + 40); }
EOF2
otool -l $t/a.o | grep 'sectname __eh_frame' || skip

$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o
otool -l $t/r.o > $t/lc
grep -q 'sectname __eh_frame' $t/lc
# The section's conventional flags, as in ld-prime's -r output.
grep -A8 'sectname __eh_frame' $t/lc | grep 'flags 0x6800000b'
nm -xp $t/r.o | awk '{print $NF}' > $t/names
not grep -q '^EH_Frame1$' $t/names
not grep -q '^func.eh$' $t/names
otool -rv $t/r.o | sed -n '/__eh_frame/,/^Relocation information (__/p' > $t/relocs
grep -q 'GOT .*___gxx_personality_v0' $t/relocs
not grep -q 'SUB ' $t/relocs
not grep -q '__ZN1S1gEv' $t/relocs

$CXX --ld-path=$mold -o $t/exe $t/r.o
$t/exe | grep '^42$'
# Apple's ld is asked to sign because the CI runner hangs running
# unsigned x86_64 binaries (see relocatable.sh).
$CXX -Wl,-adhoc_codesign -o $t/exe2 $t/r.o
$t/exe2 | grep '^42$'
