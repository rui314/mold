#!/bin/bash
source "$(dirname "$0")"/common.inc

# A reference only bitcode made, which the code LTO compiled no longer
# makes - with ThinLTO and merged modules alike - binds nothing and is
# in no symbol table, and one no dylib defines is no error. (ld-prime
# resolves bitcode's references before LTO and lists the imports among
# them, unbound, unless -dead_strip.)
cat <<EOF > $t/a.c
#include <unistd.h>
int nowhere(int);
int unused_fn(int x) { return nowhere(x) + getpid(); }
int main() { return 0; }
EOF
for lto in thin full; do
  $CC -O1 -flto=$lto -c $t/a.c -o $t/a-$lto.o
  $CC --ld-path=$mold -flto=$lto -o $t/exe-$lto $t/a-$lto.o
  $RUN $t/exe-$lto
  nm -m $t/exe-$lto > $t/nm-$lto
  not grep -q -e _getpid -e _nowhere $t/nm-$lto
  dyld_info -fixups $t/exe-$lto > $t/fixups-$lto
  not grep -q _getpid $t/fixups-$lto

  $CC --ld-path=$mold -flto=$lto -o $t/exe2-$lto $t/a-$lto.o -Wl,-dead_strip
  nm -m $t/exe2-$lto > $t/nm2-$lto
  not grep -q _getpid $t/nm2-$lto
done
