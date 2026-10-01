#!/bin/bash
source "$(dirname "$0")"/common.inc

# A function that deduplication folded into an identical one has no
# bytes of its own, but a mergeable dylib's record keeps its name, as
# ld-prime records it: an alias of the function kept, after the
# imports, which its other labels are aliases of in turn. A merging
# link finds an exported Swift function folded so (Swift promises no
# function an address of its own), and the image has every name.
if [ $ARCH = arm64 ]; then
  body() { echo "mov w0, #$1"; echo ret; }
else
  body() { echo "movl \$$1, %eax"; echo ret; echo nop; echo nop; }
fi

{
  echo '.subsections_via_symbols'
  echo '.text'
  echo '.globl "_$s2a", "_$s2b"'
  echo '"_$s2a":'; body 2
  echo '"_$s2b":'; body 2
} > $t/a.s
$CC -o $t/a.o -c $t/a.s

cat <<EOF | $CC -o $t/b.o -c -O1 -xc -
__attribute__((visibility("hidden"), noinline)) int h1(int x) { return x * 7 + 3; }
__attribute__((visibility("hidden"), noinline)) int h2(int x) { return x * 7 + 3; }
int call(int x) { return h1(x) + h2(x + 1); }
EOF

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int s2a(void) __asm__("_\$s2a");
int s2b(void) __asm__("_\$s2b");
int call(int);
int main() { printf("%d %d %d\n", s2a(), s2b(), call(1)); }
EOF

# ld-prime folds at -O1 and up or with -deduplicate.
$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o $t/b.o -Wl,-make_mergeable \
  -Wl,-deduplicate -Wl,-install_name,@rpath/libfoo.dylib
nm $t/libfoo.dylib > $t/syms
[ "$(grep -c '_\$s2[ab]$' $t/syms)" = 2 ]
[ "$(grep '_\$s2[ab]$' $t/syms | cut -d' ' -f1 | uniq | wc -l)" -eq 1 ]
[ "$(grep -E ' _h[12]$' $t/syms | cut -d' ' -f1 | uniq | wc -l)" -eq 1 ]

$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -Wl,-merge-lfoo
$t/exe | grep -q '^2 2 27$'
nm $t/exe > $t/exe-syms
grep -q ' _h2$' $t/exe-syms

$CC -o $t/exe2 $t/main.o -L$t -Wl,-merge-lfoo
$t/exe2 | grep -q '^2 2 27$'
otool -L $t/exe2 > $t/libs
not grep -q libfoo $t/libs
