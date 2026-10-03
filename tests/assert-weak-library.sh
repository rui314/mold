#!/bin/bash
source "$(dirname "$0")"/common.inc

# -assert-weak-lfoo, -assert_weak_library path and
# -assert_weak_framework Foo load a dylib weakly (LC_LOAD_WEAK_DYLIB,
# or a dylib_use_command's flag beside re-export or upward), but unlike
# -weak-l leave its imports as weak as the references make them, and
# ld-prime refuses the link if one isn't: it names each such symbol and
# the files that refer to it strongly by leaf name, and its own file of
# GOT slots ("stubs-got-file") if the symbol has one. A dylib the
# asserted one re-exports loads as its own imports say.
cat <<EOF | $CC -o $t/foo.o -c -xc -
#include <stdio.h>
__attribute__((constructor)) static void init(void) { printf("foo init\n"); }
int foo_var = 42;
int foo(void) { return 3; }
EOF
$CC -o $t/libfoo.dylib -shared $t/foo.o -Wl,-install_name,@rpath/libfoo.dylib

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
extern int foo(void) __attribute__((weak_import));
int main() { printf("%d\n", foo ? foo() : -1); }
EOF
cat <<EOF | $CC -o $t/b.o -c -xc -
int foo(void);
extern int foo_var;
int *p = &foo_var;
int main() { return foo(); }
EOF
echo 'int main() { return 0; }' | $CC -o $t/c.o -c -xc -

$CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-assert_weak_library,$t/libfoo.dylib \
  -Wl,-rpath,$t
otool -L $t/exe1 | grep -q 'libfoo.dylib (.*, weak)'
$t/exe1 | grep -q '^3$'

$CC --ld-path=$mold -o $t/exe2 $t/c.o -L$t -Wl,-assert-weak-lfoo
otool -L $t/exe2 | grep -q 'libfoo.dylib (.*, weak)'
$CC --ld-path=$mold -o $t/exe3 $t/c.o -L$t -Wl,-assert-weak-lfoo,-dead_strip_dylibs
otool -L $t/exe3 > $t/libs3
not grep -q libfoo $t/libs3

not $CC --ld-path=$mold -o $t/exe4 $t/b.o -L$t -Wl,-assert-weak-lfoo 2> $t/log4
grep -q 'Found non-weak-imported symbol(s) preventing @rpath/libfoo.dylib from being weak-linked:$' $t/log4
grep -A2 '^  "_foo" imported from:$' $t/log4 | tr -d ' ' | tr '\n' ' ' > $t/foo4
[ "$(cat $t/foo4)" = "\"_foo\"importedfrom: $t/b.o stubs-got-file " ]
grep -A1 '^  "_foo_var" imported from:$' $t/log4 | grep -q "^      $t/b.o\$"

# -weak-l makes the imports weak.
$CC --ld-path=$mold -o $t/exe5 $t/b.o -L$t -Wl,-assert-weak-lfoo,-weak-lfoo

# Weak and re-exported take a dylib_use_command, which -weak-l with
# -reexport-l can't.
$CC --ld-path=$mold -o $t/d.dylib -shared $t/c.o -L$t -Wl,-assert-weak-lfoo,-reexport-lfoo
otool -l $t/d.dylib | grep -A3 'name @rpath/libfoo.dylib (offset 28)' | grep -q 'options weak re-export'

cat <<EOF | $CC -o $t/e.o -c -xc -
typedef const void *CFTypeRef;
void CFRelease(CFTypeRef);
int main() { CFRelease(0); return 0; }
EOF
$CC --ld-path=$mold -o $t/exe6 $t/e.o -Wl,-assert_weak_framework,Foundation
otool -L $t/exe6 > $t/libs6
grep -q '/Foundation (.*, weak)' $t/libs6
grep -q '/CoreFoundation (.*[0-9])$' $t/libs6

$CC --ld-path=$mold -o $t/exe7 $t/c.o -L$t \
  -Wl,-assert-weak-lfoo,-assert-weak-lfoo,-assert_weak_library,$t/libfoo.dylib 2> $t/log7
grep -q "ignoring duplicate libraries: '.*foo'" $t/log7

# A lazy dylib's (macOS 27) refused imports are named alone.
sdk=$(xcrun --show-sdk-path)
if grep -q __dyld_lazy_load "$sdk/usr/lib/system/libdyld.tbd"; then
  echo 'int foo(void); int main() { return foo(); }' | $CC -o $t/f.o -c -xc -
  not $CC --ld-path=$mold -o $t/exe8 $t/f.o -L$t -Wl,-lazy-lfoo,-assert-weak-lfoo \
    -mmacosx-version-min=27.0 2> $t/log8
  grep -A1 "building lazy load dylibs: Found non-weak-imported symbol(s) preventing '@rpath/libfoo.dylib' from being weak-lazy-linked:" $t/log8 | \
    grep -q '^  "_foo"$'
fi
