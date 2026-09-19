#!/bin/bash
source "$(dirname "$0")"/common.inc

echo 'int foo() { return 0; }' | $CC -mmacosx-version-min=13.0 \
  --ld-path=$mold -dynamiclib -xc - -o $t/libfoo.dylib
echo 'int foo(); int main() { return foo(); }' | \
  $CC -mmacosx-version-min=13.0 -c -xc - -o $t/main.o

$CC --ld-path=$mold $t/main.o $t/libfoo.dylib -o $t/exe

# Change only the platform metadata, keeping the CPU architecture.
for cmd in -set-build-version -set-version-min; do
  xcrun vtool $cmd ios 12.0 12.0 -replace \
    -output $t/libios.dylib $t/libfoo.dylib
  not $CC --ld-path=$mold $t/main.o $t/libios.dylib -o $t/exe 2> $t/log
  grep -q 'macOS.*iOS' $t/log
  grep -q libios.dylib $t/log
done
