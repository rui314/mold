#!/bin/bash
source "$(dirname "$0")"/common.inc

# The macOS versions are the point of the test.
on_simulator && skip

# A dylib the link names that was built for a newer OS than the output
# targets draws a warning naming its install name, as an object does.
# A library found in the SDK - by a search under
# the -syslibroot, or by a path looked up under it - draws none, nor
# does one a dylib re-exports.
root=$t/root
mkdir -p $t/lib $root/usr/lib $root/opt/x

echo 'int foo(void) { return 3; }' > $t/foo.c
$CC -o $t/lib/libfoo.dylib -shared $t/foo.c -mmacosx-version-min=15.0 \
  -install_name /opt/inst/libfoo.dylib
cp $t/lib/libfoo.dylib $root/usr/lib/
cp $t/lib/libfoo.dylib $root/opt/x/

echo 'int foo(void); int main() { return foo(); }' | \
  $CC -o $t/main.o -c -xc - -mmacosx-version-min=14.0
echo 'int x;' | $CC -o $t/new.o -c -xc - -mmacosx-version-min=15.0

link() {
  $mold -arch $ARCH -platform_version macos 13.0 13.0 -o $t/exe $t/main.o \
    $SDK/usr/lib/libSystem.tbd "$@"
}

link $t/lib/libfoo.dylib $t/new.o 2> $t/log
cat > $t/expected <<EOF
object file (main.o) was built for newer 'macOS' version (14.0) than being linked (13.0)
building for macOS-13.0, but linking with dylib '/opt/inst/libfoo.dylib' which was built for newer version 15.0
object file (new.o) was built for newer 'macOS' version (15.0) than being linked (13.0)
EOF
grep 'warning: .*built for newer' $t/log | sed -e 's/.*warning: //' -e 's|([^()]*/|(|' | sort > $t/actual
sort $t/expected | diff - $t/actual

link -syslibroot $root -lfoo 2> $t/log2
not grep -q 'linking with dylib' $t/log2
link -syslibroot $root -L/opt/x -lfoo 2> $t/log3
not grep -q 'linking with dylib' $t/log3
link -syslibroot $root -weak_library /opt/x/libfoo.dylib 2> $t/log4
not grep -q 'linking with dylib' $t/log4
link -syslibroot $root -L$t/lib -lfoo 2> $t/log5
grep -q "linking with dylib '/opt/inst/libfoo.dylib'" $t/log5
link -syslibroot $root $root/opt/x/libfoo.dylib 2> $t/log6
grep -q "linking with dylib '/opt/inst/libfoo.dylib'" $t/log6

# A dylib for macOS 13 re-exporting the one for 15.
$CC -o $t/lib/libre.dylib -shared $t/foo.c -mmacosx-version-min=13.0 \
  -install_name /opt/inst/libre.dylib -Wl,-reexport_library,$t/lib/libfoo.dylib 2> /dev/null
link $t/lib/libre.dylib 2> $t/log7
not grep -q 'linking with dylib' $t/log7

# A version 5 stub gives each target's minimum OS version.
cat > $t/foo.tbd <<EOF
{
  "main_library": {
    "exported_symbols": [ { "text": { "global": [ "_foo" ] } } ],
    "install_names": [ { "name": "/opt/tbd/libfoo.dylib" } ],
    "target_info": [ { "min_deployment": "15.0", "target": "$ARCH-macos" } ]
  },
  "tapi_tbd_version": 5
}
EOF
link $t/foo.tbd 2> $t/log8
grep -q "building for macOS-13.0, but linking with dylib '/opt/tbd/libfoo.dylib' which was built for newer version 15.0" $t/log8

# A dylib is warned of once, however often it is named: twice by path,
# by path and by -l, or by its copy with the same install name. (ld-prime
# warns once per input naming it.)
count() { grep -c "linking with dylib '/opt/inst/libfoo.dylib'" $t/log9; }
link $t/lib/libfoo.dylib $t/lib/libfoo.dylib 2> $t/log9
[ "$(count)" = 1 ]
link -L$t/lib -lfoo $t/lib/libfoo.dylib 2> $t/log9
[ "$(count)" = 1 ]
link $t/lib/libfoo.dylib $root/opt/x/libfoo.dylib 2> $t/log9
[ "$(count)" = 1 ]
link -L$t/lib -lfoo -needed-lfoo 2> $t/log9
[ "$(count)" = 1 ]
