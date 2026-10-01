#!/bin/bash
source "$(dirname "$0")"/common.inc

# From macOS 27, a dylib named by -lazy-l, -lazy_library or
# -lazy_framework loads at the first use of one of its symbols. It has
# no LC_LOAD_DYLIB but an LC_LAZY_LOAD_DYLIB_INFO record; calls go
# through a helper per symbol ($lazyLoadStub) and GOT loads through
# load helpers, which have __dyld_lazy_load load the dylib and bind
# its __lazy_load_got slots, then go on through the slots.
sdk=$(xcrun --show-sdk-path)
grep -q __dyld_lazy_load "$sdk/usr/lib/system/libdyld.tbd" || skip

cat <<EOF | $CC -o $t/foo.o -c -xc -
#include <stdio.h>
int fdata = 5;
int foo(void) { return 3; }
int bar(int x) { return x + 1; }
__attribute__((constructor)) static void init(void) { printf("foo loaded\n"); }
EOF
cat <<EOF | $CC -o $t/qux.o -c -xc -
#include <stdio.h>
int qux(void) { return 4; }
__attribute__((constructor)) static void init(void) { printf("qux loaded\n"); }
EOF
$CC -o $t/libfoo.dylib -shared $t/foo.o -Wl,-install_name,@rpath/libfoo.dylib
$CC -o $t/libqux.dylib -shared $t/qux.o -Wl,-install_name,@rpath/libqux.dylib

# leaf() saves no link register, so its GOT load branches to a helper
# of its own, which branches back.
cat <<EOF | $CC -o $t/a.o -c -xc - -O1 -mmacosx-version-min=27.0
#include <stdio.h>
extern int fdata;
int foo(void), bar(int), qux(void);
__attribute__((noinline)) int leaf(void) { return fdata; }
int main() {
  printf("start\n");
  int n = leaf();
  printf("%d %d %d\n", n, foo(), bar(4));
  printf("%d\n", qux());
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -L$t -Wl,-lazy-lfoo,-lazy_library,$t/libqux.dylib \
  -Wl,-rpath,$t -mmacosx-version-min=27.0
otool -l $t/exe > $t/lc
[ "$(grep -c 'cmd LC_LAZY_LOAD_DYLIB_INFO' $t/lc)" = 2 ]
not grep -q libfoo $t/lc
grep -q 'sectname __lazy_helpers' $t/lc
grep -q 'sectname __lazy_load_got' $t/lc
nm -m $t/exe > $t/nm
grep -q '(undefined) external __dyld_lazy_load (from libSystem)' $t/nm
grep -q 'non-external (was a private external) _foo$lazyLoadStub' $t/nm
grep -q '(__DATA,__lazy_load_got) non-external _foo$lazyGOT' $t/nm
grep -q '(__DATA,__data) non-external _lazyLoadFlag$libfoo.dylib' $t/nm
if [ $ARCH = arm64 ]; then
  grep -q '_fdata$lazyGOT$loadHelper_x8$for$_leaf+0' $t/nm
else
  grep -q '_fdata$lazyGOT$loadHelper_rax' $t/nm
fi

# The record: the install name's offset, the flag's and the first
# slot's image offsets, the chain's pointer format, and the symbols.
otool -l $t/exe | grep -A3 'cmd LC_LAZY_LOAD_DYLIB_INFO' | grep dataoff | tail -1 > $t/rec
off=$(awk '{print $2}' $t/rec)
xxd -s $off -l 40 -p $t/exe | tr -d '\n' > $t/bytes
grep -q '^24000000........00000600........0300000018000000' $t/bytes

# The dylibs load as the program first uses them.
$t/exe > $t/out
printf 'start\nfoo loaded\n5 3 5\nqux loaded\n4\n' | cmp - $t/out

# Only calls and GOT loads can be lazy; a pointer in data is refused.
cat <<EOF | $CC -o $t/b.o -c -xc -
extern int fdata;
int *p = &fdata;
int main() { return *p; }
EOF
not $CC --ld-path=$mold -o $t/exe2 $t/b.o -L$t -Wl,-lazy-lfoo -mmacosx-version-min=27.0 2> $t/log
grep -q "ptr64 use of '_fdata' in '_p' cannot be lazy loaded." $t/log

# ld-prime refuses them all in one error, a line each.
cat <<EOF | $CC -o $t/b2.o -c -xc -
extern int fdata;
int foo(void);
int *p = &fdata;
void *q = (void *)foo;
int main() { return *p; }
EOF
not $CC --ld-path=$mold -o $t/exe2 $t/b2.o -L$t -Wl,-lazy-lfoo -mmacosx-version-min=27.0 2> $t/log
grep -A1 "ptr64 use of '_fdata' in '_p' cannot be lazy loaded.$" $t/log | \
  grep -q "^ptr64 use of '_foo' in '_q' cannot be lazy loaded.$"

# A dylib can load one lazily too.
cat <<EOF | $CC -o $t/c.o -c -xc -
int foo(void);
int mid(void) { return foo() + 1; }
EOF
$CC --ld-path=$mold -o $t/libmid.dylib -shared $t/c.o -L$t -Wl,-lazy-lfoo \
  -Wl,-install_name,@rpath/libmid.dylib -mmacosx-version-min=27.0
cat <<EOF | $CC -o $t/d.o -c -xc -
#include <stdio.h>
int mid(void);
int main() { printf("start\n"); printf("%d\n", mid()); }
EOF
$CC --ld-path=$mold -o $t/exe3 $t/d.o -L$t -lmid -Wl,-rpath,$t -mmacosx-version-min=27.0
$t/exe3 > $t/out3
printf 'start\nfoo loaded\n4\n' | cmp - $t/out3

# A lazy dylib the program does not use has no record, but
# __dyld_lazy_load is still imported.
cat <<EOF | $CC -o $t/e.o -c -xc -
int main() { return 0; }
EOF
$CC --ld-path=$mold -o $t/exe4 $t/e.o -L$t -Wl,-lazy-lfoo -mmacosx-version-min=27.0
otool -l $t/exe4 > $t/lc4
not grep -q LC_LAZY_LOAD_DYLIB_INFO $t/lc4
nm $t/exe4 | grep -q 'U __dyld_lazy_load'
