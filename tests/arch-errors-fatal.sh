#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime ignores an input file without the link's architecture, with
# a warning. -arch_errors_fatal makes that an error in the same words,
# naming the file after them: for an object or an archive member of
# another architecture, a dylib, a fat file without the slice, and a
# stub without the target.
[ $ARCH = arm64 ] && other=x86_64 || other=arm64

echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
echo 'int foo() { return 1; }' | cc -arch $other -o $t/b.o -c -xc -
rm -f $t/libb.a
ar rcs $t/libb.a $t/b.o
cc -arch $other -o $t/libc.dylib -shared $t/b.o
lipo -create $t/b.o -output $t/fat.o
cat > $t/libd.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $other-$PLATFORM ]
install-name:    '/usr/lib/libd.dylib'
exports:
  - targets:         [ $other-$PLATFORM ]
    symbols:         [ _foo ]
...
EOF

found="found architecture '$other', required architecture '$ARCH'"
fat="fat file missing arch '$ARCH', file has '$other'"
tapi="tapi error: missing required architecture $ARCH in file .*libd.tbd"

for file in b.o libb.a libc.dylib fat.o libd.tbd; do
  case $file in
  b.o | libc.dylib) why=$found name=$t/$file ;;
  libb.a) why=$found name="$t/libb.a(b.o)" ;;
  fat.o) why=$fat name=$t/$file ;;
  libd.tbd) why=$tapi name=$t/$file ;;
  esac

  $CC --ld-path=$mold -o $t/exe $t/a.o $t/$file 2> $t/log
  grep -q "warning: ignoring file '$name': $why" $t/log

  not $CC --ld-path=$mold -o $t/exe $t/a.o $t/$file -Wl,-arch_errors_fatal 2> $t/log
  grep -q "$why in '$name'" $t/log
  not grep -q warning $t/log
done
