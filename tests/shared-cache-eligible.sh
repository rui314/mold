#!/bin/bash
source "$(dirname "$0")"/common.inc

# A dylib installed in /usr/lib or /System/Library is bound for the dyld
# shared cache, which can hold it only if everything it links is there
# too. ld-prime rejects such a dylib linking a library installed
# elsewhere - the first one in load-command order - unless
# -not_for_dyld_shared_cache opts out; a library -dead_strip_dylibs
# drops doesn't count, and other outputs aren't checked.
echo 'int bar(void) { return 1; }' | $CC -o $t/bar.o -c -xc -
echo 'int baz(void) { return 2; }' | $CC -o $t/baz.o -c -xc -
$CC --ld-path=$mold -o $t/librpath.dylib -shared $t/bar.o -Wl,-install_name,@rpath/librpath.dylib
$CC --ld-path=$mold -o $t/liblocal.dylib -shared $t/baz.o -Wl,-install_name,/usr/local/lib/liblocal.dylib
$CC --ld-path=$mold -o $t/libsys.dylib -shared $t/bar.o -Wl,-install_name,/usr/lib/libsys.dylib
echo 'int bar(void); int baz(void); int foo(void) { return bar() + baz(); }' | \
  $CC -o $t/foo.o -c -xc -

not $CC --ld-path=$mold -o $t/a.dylib -shared $t/foo.o $t/librpath.dylib $t/liblocal.dylib \
  -Wl,-install_name,/usr/lib/libfoo.dylib 2> $t/log
grep -q "Shared cache eligible dylib cannot link to ineligible dylib '@rpath/librpath.dylib'" $t/log
[ "$(grep -c 'ineligible dylib' $t/log)" = 1 ]

not $CC --ld-path=$mold -o $t/b.dylib -shared $t/foo.o $t/liblocal.dylib $t/librpath.dylib \
  -Wl,-install_name,/System/Library/Frameworks/Foo.framework/Foo 2> $t/log2
grep -q "ineligible dylib '/usr/local/lib/liblocal.dylib'" $t/log2

$CC --ld-path=$mold -o $t/c.dylib -shared $t/foo.o $t/librpath.dylib $t/liblocal.dylib \
  -Wl,-install_name,/usr/lib/libfoo.dylib -Wl,-not_for_dyld_shared_cache
$CC --ld-path=$mold -o $t/d.dylib -shared $t/foo.o $t/librpath.dylib $t/liblocal.dylib \
  -Wl,-install_name,/usr/local/lib/libfoo.dylib

echo 'int baz(void) { return 2; } int foo(void) { return 1; }' | $CC -o $t/e.o -c -xc -
$CC --ld-path=$mold -o $t/e.dylib -shared $t/e.o $t/libsys.dylib $t/librpath.dylib \
  -Wl,-install_name,/usr/lib/libfoo.dylib -Wl,-dead_strip_dylibs 2> /dev/null

# /Library/Apple/usr/lib and /Library/Apple/System/Library count as
# well, as the output's install name or a dependency's; -debug_variant
# opts out as -not_for_dyld_shared_cache does.
not $CC --ld-path=$mold -o $t/f.dylib -shared $t/foo.o $t/librpath.dylib $t/liblocal.dylib \
  -Wl,-install_name,/Library/Apple/usr/lib/libfoo.dylib 2> $t/log3
grep -q "ineligible dylib '@rpath/librpath.dylib'" $t/log3
$CC --ld-path=$mold -o $t/g.dylib -shared $t/foo.o $t/librpath.dylib $t/liblocal.dylib \
  -Wl,-install_name,/usr/lib/libfoo.dylib -Wl,-debug_variant
$CC --ld-path=$mold -o $t/libapple.dylib -shared $t/baz.o \
  -Wl,-install_name,/Library/Apple/System/Library/Frameworks/A.framework/A
$CC --ld-path=$mold -o $t/h.dylib -shared $t/foo.o $t/libsys.dylib $t/libapple.dylib \
  -Wl,-install_name,/usr/lib/libfoo.dylib
