#!/bin/bash
source "$(dirname "$0")"/common.inc

# A dylib's exports come from its export trie, which is what dyld binds
# against; the symbol table is optional and `strip` empties it of
# defined externals (nextdefsym 0). Binary frameworks ship this way
# (Lottie.xcframework keeps 16 of its 1846 exports in the symbol
# table), so a linker that reads only the symbol table finds nothing
# to resolve against. Thread-local exports are recognizable by the
# trie entry's kind flag.
cat <<EOF | $CC -o $t/a.o -c -xc -
int xfn(void) { return 7; }
__thread int xt = 3;
EOF
$CC --ld-path=$mold -dynamiclib -o $t/libx.dylib $t/a.o -install_name @rpath/libx.dylib
strip $t/libx.dylib
otool -l $t/libx.dylib | grep -q 'nextdefsym 0'
dyld_info -exports $t/libx.dylib > $t/exports
grep -q '_xfn$' $t/exports
grep -q '_xt \[per-thread\]$' $t/exports

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int xfn(void);
extern __thread int xt;
int main() { printf("%d %d\n", xfn(), xt); }
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/libx.dylib -Wl,-rpath,$t
$t/exe | grep -q '^7 3$'

# And when reached through a re-export.
$CC --ld-path=$mold -dynamiclib -o $t/liby.dylib -install_name @rpath/liby.dylib \
  -Wl,-reexport_library,$t/libx.dylib -Wl,-rpath,$t
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/liby.dylib -Wl,-rpath,$t
$t/exe2 | grep -q '^7 3$'
