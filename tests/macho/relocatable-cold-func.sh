#!/bin/bash
source "$(dirname "$0")"/common.inc

# Clang marks the split-off cold part of a function (foo.cold.1, and
# the function itself when hot/cold splitting moved code out of it)
# with N_COLD_FUNC in n_desc. ld64 -r keeps the flag; the final link
# drops it, as it drops the other layout hints.
cat <<EOF2 | $CXX -O2 -o $t/a.o -c -xc++ -
struct S { virtual ~S(); virtual int g(); };
S::~S() {}
int S::g() { try { throw 1; } catch (int) { return 2; } }
int main() { S s; return s.g() - 2; }
EOF2
nm -m $t/a.o > $t/nm0
grep -q 'cold func' $t/nm0
$mold -r -arch $ARCH -o $t/r.o $t/a.o
nm -m $t/r.o > $t/nm
grep -q '\[cold func\] __ZN1S1gEv' $t/nm
$CXX --ld-path=$mold -o $t/exe $t/r.o
$t/exe
nm -m $t/exe > $t/nm2
not grep -q 'cold func' $t/nm2
