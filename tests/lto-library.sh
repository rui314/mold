#!/usr/bin/env bash
. $(dirname $0)/common.inc

lto_library=$(dirname "$(xcrun -f clang)")/../lib/libLTO.dylib
sdk=$(xcrun --show-sdk-path)
cat <<EOF | $CC -flto -c -xc - -o $t/a.o
#include <stdio.h>
int main() { printf("Hello\n"); }
EOF
link() { $mold -arch $ARCH -platform_version macos 13.0 13.0 -syslibroot $sdk -lSystem "$@"; }

# -lto_library names the libLTO that compiles the bitcode; the last one
# counts. (ld-prime takes only a file named libLTO.dylib, and ignores
# one that doesn't exist with a warning.)
if $mold -v 2> /dev/null | grep -q mold-macho; then
  cp $lto_library $t/libfoo.dylib
  link -lto_library $t/nosuch.dylib -lto_library $t/libfoo.dylib $t/a.o -o $t/exe
  $RUN $t/exe | grep -q Hello

  # A library that can't be loaded fails a link with bitcode.
  not link -lto_library $t/libfoo.dylib -lto_library $t/nosuch.dylib $t/a.o -o $t/exe \
    2> $t/log
  grep -qF "$t/nosuch.dylib" $t/log
fi

# A link without bitcode loads none.
echo 'int main() { return 0; }' | $CC -c -xc - -o $t/b.o
link -lto_library $t/x/libLTO.dylib $t/b.o -o $t/exe
