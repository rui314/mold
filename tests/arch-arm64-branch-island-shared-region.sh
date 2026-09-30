#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = arm64 ] || skip

# A dylib bound for the shared region has its __stubs after
# __unwind_info, whose size is only known once the code is placed, so
# whether a call to a stub needs a branch island is decided then: a
# call across 100 MiB of code needs none, one across 140 MiB does.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int f(void) { puts("hello"); return 42; }
EOF

pad() {
  cat <<EOF | $CC -o $t/pad$1.o -c -xassembler -
.subsections_via_symbols
.macro pad
_pad\@:
  .long \@
  .space 0x100000 - 4
.endm
.rept $1
pad
.endr
EOF
}

cat <<EOF | $CC -o $t/main -xc -
#include <dlfcn.h>
#include <stdio.h>
int main(int argc, char **argv) {
  void *h = dlopen(argv[1], RTLD_NOW);
  int (*f)(void) = (int (*)(void))dlsym(h, "f");
  printf("%d\n", f());
}
EOF

pad 100
$CC --ld-path=$mold -dynamiclib -o $t/lib100.dylib $t/a.o $t/pad100.o \
  -install_name /usr/lib/libmoldtest.dylib
$t/main $t/lib100.dylib | grep -q '^42$'
nm $t/lib100.dylib > $t/syms100
not grep -q island $t/syms100

pad 140
$CC --ld-path=$mold -dynamiclib -o $t/lib140.dylib $t/a.o $t/pad140.o \
  -install_name /usr/lib/libmoldtest.dylib
$t/main $t/lib140.dylib | grep -q '^42$'
nm $t/lib140.dylib > $t/syms140
grep -q ' _puts\.island$' $t/syms140
