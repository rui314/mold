#!/bin/bash
source "$(dirname "$0")"/common.inc

# A dylib -delay-l, -delay_library or -delay_framework names loads and
# binds at launch as any other, but its initializers run only when the
# image dlopen()s it, at its first use of one of the dylib's symbols.
# Its LC_LOAD_DYLIB is a dylib_use_command with the delay-init flag;
# calls go through $delayInitStub stubs and GOT loads through
# $loadHelper helpers, which have the dylib's dlopen helper dlopen() it
# once (setting its flag word) before going on through the GOT.
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
# of its own, which branches back. bar is both called, through its stub,
# and loaded from the GOT, whose slot the stub jumps through.
cat <<EOF | $CC -o $t/a.o -c -xc - -O1
#include <stdio.h>
extern int fdata;
int foo(void), bar(int), qux(void);
__attribute__((noinline)) int leaf(void) { return fdata; }
int main() {
  printf("start\n");
  int n = leaf();
  int (*volatile fp)(int) = bar;
  printf("%d %d %d %d\n", n, foo(), bar(4), fp(5));
  printf("%d\n", qux());
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -L$t -Wl,-delay-lfoo,-delay_library,$t/libqux.dylib \
  -Wl,-rpath,$PWD/$t
otool -l $t/exe > $t/lc
[ "$(grep -c 'options delay-init' $t/lc)" = 2 ]
grep -A3 'name @rpath/libfoo.dylib (offset 28)' $t/lc | grep -q 'options delay-init'
grep -q 'sectname __delay_stubs' $t/lc
grep -q 'sectname __delay_helper' $t/lc
nm -m $t/exe > $t/nm
grep -q '(undefined) external _dlopen (from libSystem)' $t/nm
grep -q '(__TEXT,__delay_stubs) non-external (was a private external) _foo$delayInitStub' $t/nm
grep -q '(__TEXT,__delay_helper) non-external _dlopenHelper$libfoo.dylib' $t/nm
grep -q '(__DATA,__data) non-external _dlopenHelperFlag$libfoo.dylib' $t/nm
if [ $ARCH = arm64 ]; then
  grep -q '_fdata$loadHelper_x8$for$_leaf+0' $t/nm
else
  grep -q '_fdata$loadHelper_rax' $t/nm
fi

# The dylibs' initializers run as the program first uses them.
$RUN $t/exe > $t/out
printf 'start\nfoo loaded\n5 3 5 6\nqux loaded\n4\n' | cmp - $t/out

# Only calls and GOT loads can be delayed; a pointer in data, which dyld
# binds at launch, is refused.
cat <<EOF | $CC -o $t/b.o -c -xc -
extern int fdata;
int *p = &fdata;
int main() { return *p; }
EOF
not $CC --ld-path=$mold -o $t/exe2 $t/b.o -L$t -Wl,-delay-lfoo 2> $t/log
grep -q "use of '_fdata' in '_p' cannot be delayed" $t/log

# So is a class reference of Objective-C code below macOS 15, a pointer
# to the class (see delay-library-objc-class.sh).
cat <<EOF | $CC -o $t/f.o -c -xobjective-c - -mmacosx-version-min=14.0
#import <Foundation/Foundation.h>
int main() { return [NSString string] != nil ? 0 : 1; }
EOF
not $CC --ld-path=$mold -o $t/exe10 $t/f.o -framework Foundation \
  -Wl,-delay_framework,Foundation -mmacosx-version-min=14.0 2> $t/log
grep -q "NSString.* cannot be delayed" $t/log

# A delayed dylib the program doesn't use keeps its load command, and
# _dlopen is still imported; -dead_strip_dylibs drops the dylib.
echo 'int main() { return 0; }' | $CC -o $t/c.o -c -xc -
$CC --ld-path=$mold -o $t/exe3 $t/c.o -L$t -Wl,-delay-lfoo
otool -l $t/exe3 | grep -A3 'name @rpath/libfoo.dylib' | grep -q 'options delay-init'
nm $t/exe3 | grep -q 'U _dlopen'
$CC --ld-path=$mold -o $t/exe4 $t/c.o -L$t -Wl,-delay-lfoo,-dead_strip_dylibs
otool -L $t/exe4 > $t/libs4
not grep -q libfoo $t/libs4

# dyld before macOS 15 runs the initializers at launch: ld-prime warns,
# but delays the dylib all the same.
$CC --ld-path=$mold -o $t/exe5 $t/a.o -L$t -Wl,-delay-lfoo,-lqux -Wl,-rpath,$PWD/$t \
  -mmacosx-version-min=14.0 2> $t/log5
grep -q "delay-init will be ignored for 'foo' because deployment target version is too low" $t/log5
otool -l $t/exe5 | grep -q 'options delay-init'

# Nor does ld-prime care for a dylib with weak definitions to export.
cat <<EOF | $CC -o $t/wd.o -c -xc -
__attribute__((weak)) int wfoo(void) { return 1; }
int foo(void) { return 3; }
EOF
$CC -o $t/libwd.dylib -shared $t/wd.o -Wl,-install_name,@rpath/libwd.dylib
$CC --ld-path=$mold -o $t/exe6 $t/c.o -Wl,-delay_library,$t/libwd.dylib 2> $t/log6
grep -q "delay-init link with '@rpath/libwd.dylib' will be ignored because it has weak-def exports" $t/log6

# A delayed dylib can be neither re-exported nor lazily loaded.
not $CC --ld-path=$mold -o $t/d.dylib -shared $t/c.o -L$t -Wl,-delay-lfoo,-reexport-lfoo 2> $t/log
grep -Fq "'-delay-lfoo' and '-reexport-lfoo' cannot be used together" $t/log
not $CC --ld-path=$mold -o $t/exe7 $t/c.o -L$t -Wl,-delay-lfoo,-lazy-lfoo 2> $t/log
grep -Fq "'-delay-lfoo' and '-lazy-lfoo' cannot be used together" $t/log

# -assert-weak-l refuses a delayed dylib's strong imports too. (ld-prime
# lists the file of its stubs and helpers, "deferred-dylib-file", with
# the importers.)
not $CC --ld-path=$mold -o $t/exe9 $t/a.o -L$t -Wl,-delay-lfoo,-assert-weak-lfoo,-lqux 2> $t/log
grep -A1 '^  "_foo" imported from:$' $t/log | grep -q "^      $t/a.o\$"

# The public libraries a delayed dylib re-exports are delayed with it,
# and dlopen() it.
cat <<EOF | $CC -o $t/e.o -c -xc -
typedef const void *CFTypeRef;
void CFRelease(CFTypeRef);
int main() { CFRelease(0); return 0; }
EOF
$CC --ld-path=$mold -o $t/exe8 $t/e.o -Wl,-delay_framework,Foundation
otool -l $t/exe8 | grep -A3 'CoreFoundation (offset 28)' | grep -q 'options delay-init'
nm $t/exe8 | grep -q '_CFRelease$delayInitStub'
nm $t/exe8 | grep -q '_dlopenHelper$Foundation'
