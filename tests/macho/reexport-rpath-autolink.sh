#!/bin/bash
source "$(dirname "$0")"/common.inc

# libA privately re-exports libB, whose install name is @rpath-relative.
# When the command line names libB, its symbols bind to it, but when an
# auto-link option does (as UI tests' objects auto-link XCUIAutomation,
# which XCTest re-exports), ld-prime takes it for the library libA has
# loaded and binds them to libA, giving libB no load command. With any
# other install name, an auto-linked libB is one of its own.
mkdir -p $t/sub $t/lib
echo 'int sym = 1; int b_only = 2;' | $CC -o $t/b.o -c -xc -
echo 'int a_only = 3;' | $CC -o $t/ao.o -c -xc -
$CC --ld-path=$mold -dynamiclib -o $t/sub/libB.dylib $t/b.o \
  -Wl,-install_name,@rpath/libB.dylib
cp $t/sub/libB.dylib $t/lib/libB.dylib
$CC --ld-path=$mold -dynamiclib -o $t/lib/libA.dylib $t/ao.o \
  -Wl,-install_name,@rpath/libA.dylib -Wl,-rpath,@loader_path/../sub \
  -Wl,-reexport_library,$t/sub/libB.dylib

echo 'extern int sym, b_only; int main() { return (long)&sym + (long)&b_only == 0; }' | \
  $CC -o $t/a.o -c -xc -
printf '.linker_option "-lB"\n' | $CC -o $t/auto.o -c -xassembler -

from() { dyld_info -fixups $1 | grep -E "/_$2( |$)" | awk '{ print $NF }' | sed 's|/.*||'; }

$CC --ld-path=$mold -o $t/exe1 $t/a.o $t/auto.o -L$t/lib -lA
[ "$(from $t/exe1 sym)" = libA ]
[ "$(from $t/exe1 b_only)" = libA ]
otool -L $t/exe1 > $t/libs1
not grep -q libB $t/libs1

$CC --ld-path=$mold -o $t/exe2 $t/a.o -L$t/lib -lA -lB
[ "$(from $t/exe2 sym)" = libB ]

# An absolute install name.
$CC --ld-path=$mold -dynamiclib -o $t/sub/libB.dylib $t/b.o \
  -Wl,-install_name,$PWD/$t/sub/libB.dylib
cp $t/sub/libB.dylib $t/lib/libB.dylib
$CC --ld-path=$mold -dynamiclib -o $t/lib/libA.dylib $t/ao.o \
  -Wl,-install_name,@rpath/libA.dylib -Wl,-reexport_library,$t/sub/libB.dylib
$CC --ld-path=$mold -o $t/exe3 $t/a.o $t/auto.o -L$t/lib -lA
[ "$(from $t/exe3 sym)" = libB ]
