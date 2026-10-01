#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime resolves bitcode's references before LTO, and lists the
# imports among them in the symbol table even if the code LTO compiled
# no longer refers to them, unbound - with ThinLTO and merged modules
# alike - unless -dead_strip drops what no live code refers to. One no
# dylib defines is no error then.
cat <<EOF > $t/a.c
#include <unistd.h>
int nowhere(int);
int unused_fn(int x) { return nowhere(x) + getpid(); }
int main() { return 0; }
EOF
for lto in thin full; do
  $CC -O1 -flto=$lto -c $t/a.c -o $t/a-$lto.o
  $CC --ld-path=$mold -flto=$lto -o $t/exe-$lto $t/a-$lto.o
  $t/exe-$lto
  nm -m $t/exe-$lto > $t/nm-$lto
  grep -q '(undefined) external _getpid (from libSystem)' $t/nm-$lto
  not grep -q _nowhere $t/nm-$lto
  dyld_info -fixups $t/exe-$lto > $t/fixups-$lto
  not grep -q _getpid $t/fixups-$lto

  $CC --ld-path=$mold -flto=$lto -o $t/exe2-$lto $t/a-$lto.o -Wl,-dead_strip
  nm -m $t/exe2-$lto > $t/nm2-$lto
  not grep -q _getpid $t/nm2-$lto
done
