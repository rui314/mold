#!/bin/bash
source "$(dirname "$0")"/common.inc

# C++'s inline thread_local is a weak definition in every image that
# uses it, and dyld coalesces them: the executable and the dylib share
# one variable. So its TLV loads go through a __got slot bound by weak
# lookup rather than relaxing to the image's own descriptor.
echo 'inline thread_local int wtl = 3;' > $t/w.h
cat <<EOF | $CXX -std=c++17 -I$t -o $t/a.o -c -xc++ -
#include "w.h"
int bump() { return ++wtl; }
EOF
cat <<EOF | $CXX -std=c++17 -I$t -o $t/b.o -c -xc++ -
#include "w.h"
#include <cstdio>
int bump();
int main() { bump(); std::printf("%d\n", ++wtl); }
EOF

$CXX --ld-path=$mold -dynamiclib -o $t/liba.dylib $t/a.o \
  -install_name @rpath/liba.dylib
$CXX --ld-path=$mold -o $t/exe $t/b.o $t/liba.dylib -Wl,-rpath,$PWD/$t
$RUN $t/exe | grep -q '^5$'
for f in $t/exe $t/liba.dylib; do
  dyld_info -fixups $f > $t/fixups
  grep -q '__got .* bind .*weak-def-coalesce>/_wtl' $t/fixups
done

$CXX --ld-path=$mold -dynamiclib -o $t/liba.dylib $t/a.o \
  -install_name @rpath/liba.dylib -Wl,-no_fixup_chains
$CXX --ld-path=$mold -o $t/exe2 $t/b.o $t/liba.dylib -Wl,-rpath,$PWD/$t \
  -Wl,-no_fixup_chains
$RUN $t/exe2 | grep -q '^5$'
