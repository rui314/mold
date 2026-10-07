#!/usr/bin/env bash
. $(dirname $0)/common.inc

lto_library=$(dirname "$(xcrun -f clang)")/../lib/libLTO.dylib

echo 'int f(void) { return 0; }' | $CC -flto -O2 -c -xc - -o $t/a.o
cat <<EOF | $CC -O2 -fno-builtin -c -xc - -o $t/ms.o
void *memset(void *p, int c, unsigned long n) {
  char *q = p;
  while (n--)
    *q++ = c;
  return p;
}
int from_softloaded(void) { return 42; }
EOF
rm -f $t/libms.a
ar rcs $t/libms.a $t/ms.o
cat <<EOF | $CC -c -xc - -o $t/main.o
#include <stdio.h>
int from_softloaded(void);
int main() { printf("%d\n", from_softloaded()); }
EOF

# -lto_softload_runtime_symbols has the archive member that defines a
# runtime routine LTO code may come to call load, though nothing calls
# it before LTO.
$CC --ld-path=$mold -flto -dynamiclib -o $t/libsoft.dylib $t/a.o $t/libms.a \
  -Wl,-lto_softload_runtime_symbols
$CC --ld-path=$mold -o $t/exe $t/main.o $t/libsoft.dylib
$RUN $t/exe | grep -q '^42$'

# Not by default, but in a -static or -preload image.
$CC --ld-path=$mold -flto -dynamiclib -o $t/libsoft2.dylib $t/a.o $t/libms.a
nm $t/libsoft2.dylib > $t/nm2
not grep -q _from_softloaded $t/nm2

$mold -arch $ARCH -platform_version macos 13.0 13.0 -static -e _f -o $t/static \
  -lto_library $lto_library $t/a.o $t/libms.a -map $t/map
grep -q 'libms.a(ms.o)$' $t/map

# A dylib named before the archive provides the routine instead: the
# archive member doesn't load.
$CC --ld-path=$mold -flto -dynamiclib -o $t/libsoft3.dylib $t/a.o -lSystem $t/libms.a \
  -Wl,-lto_softload_runtime_symbols
nm -m $t/libsoft3.dylib > $t/nm3
not grep -q _from_softloaded $t/nm3
