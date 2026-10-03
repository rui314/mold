#!/bin/bash
source "$(dirname "$0")"/common.inc

# -trace_implicit_libraries prints on stdout the libraries the link
# brings in on its own, once each: the auto-link hints of the live
# objects, an archive member's included, and the libraries a dylib
# re-exports but those the link names itself, with the file of the
# dylib. Files go by their paths, an archive member as "lib.a(member.o)".
# -trace_implicit_library picks the lines about libraries whose names
# hold its argument. (ld-prime prints the command line objects' hints
# twice, frameworks first, a stub's re-exports once per platform it has
# for the target, and names files by their real paths.)
dir=$(cd $t && pwd -P)

echo 'int inner() { return 3; }' | $CC -o $t/inner.o -c -xc -
$CC -o $t/libinner.dylib -dynamiclib $t/inner.o -install_name $dir/libinner.dylib
echo 'int outer() { return 2; }' | $CC -o $t/outer.o -c -xc -
$CC -o $t/libouter.dylib -dynamiclib $t/outer.o -Wl,-reexport_library,$t/libinner.dylib \
  -Wl,-reexport_library,$t/libinner.dylib -install_name $dir/libouter.dylib

cat <<EOF > $t/libzip.tbd
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-macos, $ARCH-maccatalyst ]
install-name:    '$dir/libzip.dylib'
reexported-libraries:
  - targets:         [ $ARCH-macos, $ARCH-maccatalyst ]
    libraries:       [ '$dir/libnone.dylib' ]
exports:
  - targets:         [ $ARCH-macos, $ARCH-maccatalyst ]
    symbols:         [ _zip ]
...
EOF

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl _main
.p2align 2
_main:
  ret
.linker_option "-lnosuch"
.linker_option "-framework", "Foundation"
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.globl _b
.p2align 2
_b:
  ret
.linker_option "-lzip"
EOF
rm -f $t/libb.a
ar rcs $t/libb.a $t/b.o
echo 'int b(); int main() { return b(); }' | $CC -o $t/c.o -c -xc -

$CC --ld-path=$mold -o $t/exe1 $t/a.o $t/libouter.dylib -L$t -Wl,-trace_implicit_libraries \
  2> /dev/null > $t/log1
[ "$(grep -cx "auto-linking framework hint 'Foundation' from file '$t/a.o'" $t/log1)" = 1 ]
[ "$(grep -cx "auto-linking library hint 'nosuch' from file '$t/a.o'" $t/log1)" = 1 ]
[ "$(grep -c "^indirect library '$dir/libinner.dylib' from file '$t/libouter.dylib'$" $t/log1)" = 1 ]
[ "$(grep -c "^indirect library '/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation' from file '.*/Foundation.tbd'$" $t/log1)" = 1 ]
[ "$(grep -c "^indirect library '/usr/lib/libobjc.A.dylib' from file " $t/log1)" = 1 ]
[ "$(sort $t/log1 | uniq -d)" = "" ]

$CC --ld-path=$mold -o $t/exe2 $t/c.o $t/libb.a -L$t -Wl,-trace_implicit_libraries \
  2> /dev/null > $t/log2
grep -qx "auto-linking library hint 'zip' from file '$t/libb.a(b.o)'" $t/log2
[ "$(grep -c "auto-linking library hint 'zip'" $t/log2)" = 1 ]
not grep -q libnone $t/log2

$CC --ld-path=$mold -o $t/exe3 $t/a.o $t/libouter.dylib -L$t -Wl,-trace_implicit_library,inner \
  -Wl,-trace_implicit_library,nosuch 2> /dev/null > $t/log3
[ "$(grep -c "^indirect library '$dir/libinner.dylib'" $t/log3)" = 1 ]
[ "$(grep -c "hint 'nosuch'" $t/log3)" = 1 ]
not grep -q Foundation $t/log3
