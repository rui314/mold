#!/bin/bash
source "$(dirname "$0")"/common.inc

# The export trie, the function starts, __unwind_info and
# __init_offsets hold offsets from the image's mach header, which
# -image_base moves away from the end of __PAGEZERO: a dylib's lies at
# the base itself, as does the __TEXT of a non-PIE executable.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo(void) { return 42; }
EOF
# (-image_base takes no effect with chained fixups, which a simulator's
# version defaults to: -no_fixup_chains turns them off there.)
if on_simulator; then classic=-Wl,-no_fixup_chains; else classic=-mmacosx-version-min=11.0; fi
$CC --ld-path=$mold -o $t/a.dylib -shared $t/a.o -Wl,-image_base,0x180000000 $classic
dyld_info -exports $t/a.dylib > $t/exports
grep -Eq '^ *0x0000[0-9A-F]{4} +_foo$' $t/exports
dyld_info -function_starts $t/a.dylib > $t/starts
grep -Eq '^ *0x18000[0-9A-F]{4} +_foo$' $t/starts

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <dlfcn.h>
#include <stdio.h>
int main(int argc, char **argv) {
  void *h = dlopen(argv[1], RTLD_NOW);
  int (*fn)(void) = h ? dlsym(h, "foo") : 0;
  printf("%d\n", fn ? fn() : -1);
}
EOF
$CC --ld-path=$mold -o $t/main $t/main.o
$RUN $t/main $t/a.dylib | grep '^42$'

# arm64 has no non-PIE executable to place.
[ $ARCH = x86_64 ] || exit 0

cat <<EOF | $CXX -o $t/b.o -c -xc++ -
#include <stdio.h>
__attribute__((noinline)) static void thrower() { throw 42; }
int main() {
  try { thrower(); } catch (int x) { printf("caught %d\n", x); }
}
EOF
$CXX --ld-path=$mold -o $t/exe $t/b.o -Wl,-no_pie -Wl,-image_base,0x180000000 \
  -mmacosx-version-min=12.0
$RUN $t/exe | grep '^caught 42$'

# A base inside __PAGEZERO is reported once the image is laid out.
not $CXX --ld-path=$mold -o $t/exe2 $t/b.o -Wl,-no_pie -Wl,-image_base,0x1000 \
  -mmacosx-version-min=12.0 2> $t/log2
grep -q 'custom segments overlap: __PAGEZERO(0x0-0x100000000) __TEXT(0x1000-' $t/log2
