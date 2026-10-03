#!/bin/bash
source "$(dirname "$0")"/common.inc

# A fat dylib without a slice of the link's architecture serves it with
# a slice of another subtype of its CPU type (arm64e for arm64, x86_64h
# for x86_64) - unless -no_allow_dylib_sub_type_mismatches, or else
# $LD_DYLIB_CPU_SUBTYPES_MUST_MATCH, lists an architecture of that CPU
# type, ':'-separated: the dylib is then ignored, with ld-prime's
# warning. Names of other CPUs, or none, change nothing.
case $ARCH in
arm64) other=arm64e ;;
x86_64) other=x86_64h ;;
esac

echo 'int foo(void) { return 1; }' > $t/foo.c
cc -arch $other -shared $t/foo.c -o $t/thin.dylib -Wl,-install_name,/usr/lib/libfoo.dylib
lipo -create $t/thin.dylib -output $t/libfoo.dylib

echo 'int foo(void); int main() { return foo(); }' | $CC -o $t/a.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/a.o $t/libfoo.dylib

not $CC --ld-path=$mold -o $t/exe $t/a.o $t/libfoo.dylib \
  -Wl,-no_allow_dylib_sub_type_mismatches,i386::$other 2> $t/log
grep -q "warning: ignoring file '$t/libfoo.dylib': fat file missing arch '$ARCH', file has '$other'" $t/log

LD_DYLIB_CPU_SUBTYPES_MUST_MATCH=$ARCH not $CC --ld-path=$mold -o $t/exe $t/a.o \
  $t/libfoo.dylib 2> /dev/null
# The option overrides the variable.
LD_DYLIB_CPU_SUBTYPES_MUST_MATCH=$ARCH $CC --ld-path=$mold -o $t/exe $t/a.o $t/libfoo.dylib \
  -Wl,-no_allow_dylib_sub_type_mismatches,ppc

$CC --ld-path=$mold -o $t/exe $t/a.o $t/libfoo.dylib \
  -Wl,-no_allow_dylib_sub_type_mismatches,bogus
