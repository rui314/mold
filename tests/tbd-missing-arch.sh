#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime ignores a .tbd with no target on the link's architecture,
# with a warning naming the file as given and as it is: as a library
# named on the command line, as one a library re-exports, and in a link
# that would ignore a dylib anyway (-r).
[ $ARCH = arm64 ] && other=x86_64 || other=arm64
dir=$(cd $t && pwd -P)

cat > $t/libother.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $other-macos ]
install-name:    '$dir/libother.dylib'
exports:
  - targets:         [ $other-macos ]
    symbols:         [ _foo ]
...
EOF
cat > $t/libv5.tbd <<EOF
{"tapi_tbd_version":5,"main_library":{
 "target_info":[{"target":"$other-macos"}],
 "install_names":[{"name":"/usr/lib/libv5.dylib"}],
 "exported_symbols":[{"targets":["$other-macos"],"text":{"global":["_foo"]}}]}}
EOF
cat > $t/libtop.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ arm64-macos, x86_64-macos ]
install-name:    '$dir/libtop.dylib'
reexported-libraries:
  - targets:         [ arm64-macos, x86_64-macos ]
    libraries:       [ '$dir/libother.dylib' ]
exports:
  - targets:         [ arm64-macos, x86_64-macos ]
    symbols:         [ _top ]
...
EOF

echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
echo 'int foo(); int main() { return foo(); }' | $CC -o $t/b.o -c -xc -

for lib in other v5; do
  $CC --ld-path=$mold -o $t/exe-$lib $t/a.o $t/lib$lib.tbd 2> $t/log-$lib
  grep -q "warning: ignoring file '$t/lib$lib.tbd': tapi error: missing required architecture $ARCH in file $dir/lib$lib.tbd$" $t/log-$lib
  $t/exe-$lib
  otool -L $t/exe-$lib > $t/libs-$lib
  not grep -q lib$lib $t/libs-$lib
done

not $CC --ld-path=$mold -o $t/exe2 $t/b.o $t/libother.tbd 2> $t/log2
grep -q 'missing required architecture' $t/log2
grep -q '_foo' $t/log2

$CC --ld-path=$mold -o $t/exe3 $t/a.o $t/libtop.tbd 2> $t/log3
grep -q "warning: ignoring file '$dir/libother.tbd': tapi error: missing required architecture $ARCH in file $dir/libother.tbd$" $t/log3
otool -L $t/exe3 > $t/libs3
grep -q libtop $t/libs3
not grep -q libother $t/libs3

$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/libother.tbd 2> $t/log4
grep -q "warning: ignoring file '$t/libother.tbd': tapi error: missing required architecture" $t/log4
not grep -q 'unexpected dylib' $t/log4
