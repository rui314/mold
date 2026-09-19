#!/bin/bash
source "$(dirname "$0")"/common.inc

# arm64 compilers emit compact unwind for every frame, and a DWARF FDE
# beside it only at -O0 under -fasynchronous-unwind-tables (Swift's closure
# thunks get them; NetNewsWire's RSCore prelink carries 119 such
# atoms). A final link needs only the compact records, but ld64 -r
# carries every input CIE and FDE through, each named (EH_Frame1,
# func.eh) with its fields as SUBTRACTOR pairs against those symbols,
# so a prelink's __eh_frame matches ld-prime's byte for byte.
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
otool -l $t/a.o | grep -q 'sectname __eh_frame' || skip

$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o
otool -l $t/r.o > $t/lc
grep -q 'sectname __eh_frame' $t/lc
# Bare regular section, as in ld-prime's -r output.
grep -A8 'sectname __eh_frame' $t/lc | grep -q 'flags 0x00000000'
nm -xp $t/r.o | awk '{print $NF}' > $t/names
[ "$(grep -c '^EH_Frame1$' $t/names)" -ge 1 ]
[ "$(grep -c '^func.eh$' $t/names)" -ge 1 ]
otool -rv $t/r.o | sed -n '/__eh_frame/,/^Relocation information (__/p' > $t/relocs
grep -q 'SUB     False     EH_Frame1' $t/relocs
grep -q 'UNSIGND False     __ZN1S1gEv' $t/relocs

$CXX --ld-path=$mold -o $t/exe $t/r.o
$t/exe | grep -q '^42$'
$CXX -o $t/exe2 $t/r.o
$t/exe2 | grep -q '^42$'
