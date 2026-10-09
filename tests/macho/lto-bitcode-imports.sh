#!/bin/bash
source "$(dirname "$0")"/common.inc

# What bitcode referred to counts only as far as the code LTO compiled
# still refers to it, as mold resolves symbols and decides which
# as-needed DSOs to keep again after LTO. A reference the compiled code
# no longer makes - with ThinLTO and merged modules alike - binds
# nothing and is in no symbol table, one no dylib defines is no error,
# and a dylib nothing else refers to is unused: -dead_strip_dylibs drops
# it, while one the command line names stays without the option.
# (ld-prime resolves bitcode's references before LTO: it lists the
# imports among them, unbound, unless -dead_strip, and keeps their
# dylibs, under -dead_strip_dylibs and -dead_strip alike.)

cat <<EOF > $t/a.c
#include <unistd.h>
int nowhere(int);
const char *zlibVersion(void);
int unused_fn(int x) { return nowhere(x) + getpid() + zlibVersion()[0]; }
int main() { return 0; }
EOF
for lto in thin full; do
  $CC -O1 -flto=$lto -c $t/a.c -o $t/a-$lto.o
  $CC --ld-path=$mold -flto=$lto -o $t/exe-$lto $t/a-$lto.o -lz
  $RUN $t/exe-$lto
  dyld_info -fixups $t/exe-$lto > $t/fixups-$lto
  not grep -q -e _getpid -e _zlibVersion $t/fixups-$lto
  otool -L $t/exe-$lto > $t/libs-$lto
  grep -q /libz $t/libs-$lto
  nm -m $t/exe-$lto > $t/nm-$lto
  if is_mold; then
    not grep -q -e _getpid -e _nowhere -e _zlibVersion $t/nm-$lto
  fi

  $CC --ld-path=$mold -flto=$lto -o $t/exe2-$lto $t/a-$lto.o -lz -Wl,-dead_strip
  nm -m $t/exe2-$lto > $t/nm2-$lto
  not grep -q -e _getpid -e _zlibVersion $t/nm2-$lto

  $CC --ld-path=$mold -flto=$lto -o $t/exe3-$lto $t/a-$lto.o -lz -Wl,-dead_strip_dylibs
  $RUN $t/exe3-$lto
  otool -L $t/exe3-$lto > $t/libs3-$lto
  if is_mold; then
    not grep -q /libz $t/libs3-$lto
  fi
done
