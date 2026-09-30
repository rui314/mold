#!/bin/bash
source "$(dirname "$0")"/common.inc

# -dylib_file install_name:file loads a library a dylib re-exports
# under that install name from the file, ahead of the search by its leaf
# in the library path; one not there leaves the search to find it.
# ld-prime deprecates the option, once.
mkdir -p $t/sub $t/lib
echo 'int foo(void) { return 3; }' | $CC -o $t/foo.o -c -xc -
echo 'int bar(void) { return 2; }' | $CC -o $t/bar.o -c -xc -
$CC -o $t/sub/libfoo_real.dylib -shared $t/foo.o -Wl,-install_name,/nonexistent/libfoo.dylib
$CC -o $t/lib/libfoo.dylib -shared $t/foo.o -Wl,-install_name,/nonexistent/libfoo.dylib
$CC -o $t/libbar.dylib -shared $t/bar.o -Wl,-install_name,/u/libbar.dylib \
  -Wl,-reexport_library,$t/sub/libfoo_real.dylib
echo 'int foo(void); int main() { return foo() != 3; }' | $CC -o $t/a.o -c -xc -

not $CC --ld-path=$mold -o $t/exe $t/a.o $t/libbar.dylib 2> $t/log
grep -q "ignoring missing indirect library: library for install name '/nonexistent/libfoo.dylib' not found" $t/log

$CC --ld-path=$mold -o $t/exe $t/a.o $t/libbar.dylib -L$t/lib -Wl,-t \
  -Wl,-dylib_file,/nonexistent/libfoo.dylib:$t/sub/libfoo_real.dylib \
  -Wl,-dylib_file,/nonexistent/libfoo.dylib:$t/lib/libfoo.dylib > $t/log 2> $t/log2
grep -q "$t/sub/libfoo_real.dylib" $t/log
not grep -q "$t/lib/libfoo.dylib" $t/log
[ "$(grep -c -- '-dylib_file is deprecated. Use -F or -L to control where indirect dylibs are found' $t/log2)" = 1 ]

$CC --ld-path=$mold -o $t/exe $t/a.o $t/libbar.dylib -L$t/lib -Wl,-t \
  -Wl,-dylib_file,/nonexistent/libfoo.dylib:$t/sub/none.dylib > $t/log
grep -q "$t/lib/libfoo.dylib" $t/log

not $CC --ld-path=$mold -o $t/exe $t/a.o $t/libbar.dylib \
  -Wl,-dylib_file,/nonexistent/libfoo.dylib 2> $t/log
grep -q -- '-dylib_file malformed <path:path>' $t/log
