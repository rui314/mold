#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime ignores a bitcode file built for another architecture than
# the link's, as it does a Mach-O object (see foreign-arch), naming the
# architecture its target triple does - a Thumb one by its ARM name -,
# an archive member needed or not. -arch_errors_fatal makes it an
# error.
[ $ARCH = arm64 ] && other=x86_64 || other=arm64
echo 'int foo(void) { return 1; }' > $t/foo.c
${CC/$ARCH/$other} -flto -c $t/foo.c -o $t/other.o
clang -target armv7-apple-ios9.0 -flto -c $t/foo.c -o $t/armv7.o 2> /dev/null
rm -f $t/libother.a
ar rcs $t/libother.a $t/other.o 2> /dev/null

echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
$CC --ld-path=$mold -flto -o $t/exe $t/a.o $t/other.o $t/armv7.o $t/libother.a 2> $t/log
grep -qF "warning: ignoring file '$t/other.o': found architecture '$other', required architecture '$ARCH'" $t/log
grep -qF "warning: ignoring file '$t/armv7.o': found architecture 'armv7', required architecture '$ARCH'" $t/log
grep -qF "warning: ignoring file '$t/libother.a(other.o)': found architecture '$other', required architecture '$ARCH'" $t/log
$RUN $t/exe

not $CC --ld-path=$mold -flto -o $t/exe2 $t/a.o $t/other.o -Wl,-arch_errors_fatal 2> $t/log2
grep -qF "found architecture '$other', required architecture '$ARCH' in '$t/other.o'" $t/log2

# An arm64e one is another architecture for an arm64 link, even with
# -allow_sub_type_mismatches, and bitcode of the link's own draws no
# word for it.
echo 'int bar(void) { return 2; }' | $CC -flto -c -xc - -o $t/same.o
$CC --ld-path=$mold -flto -o $t/exe3 $t/a.o $t/same.o -Wl,-allow_sub_type_mismatches 2> $t/log3
not grep -q warning $t/log3
if [ $ARCH = arm64 ]; then
  ${CC/$ARCH/arm64e} -flto -c $t/foo.c -o $t/e.o
  $CC --ld-path=$mold -flto -o $t/exe4 $t/a.o $t/e.o -Wl,-allow_sub_type_mismatches 2> $t/log4
  grep -qF "warning: ignoring file '$t/e.o': found architecture 'arm64e', required architecture 'arm64'" $t/log4
fi

# With it, x86_64h bitcode links into an x86_64 link, with a warning
# for it as it is loaded. (ld-prime warns again as it checks it, and
# twice for the object LTO made of it, x86_64h too.)
if [ $ARCH = x86_64 ]; then
  ${CC/$ARCH/x86_64h} -flto -c $t/foo.c -o $t/h.o
  $CC --ld-path=$mold -flto -o $t/exe5 $t/a.o $t/h.o -Wl,-allow_sub_type_mismatches 2> $t/log5
  [ "$(grep -c "warning: linking x86_64h file '$t/h.o' into x86_64 link" $t/log5)" = 1 ]
fi

# ld-prime knows bitcode only in the wrapper Apple's compilers put it
# in: raw bitcode (what a compiler for another OS makes) is a file of
# an unknown type.
clang -target $ARCH-unknown-linux-gnu -flto -c $t/foo.c -o $t/raw.o
not $CC --ld-path=$mold -flto -o $t/exe6 $t/a.o $t/raw.o 2> $t/log6
grep -qF "unknown file type in '$t/raw.o'" $t/log6
