#!/usr/bin/env bash
. $(dirname $0)/common.inc

echo 'int choice() { return 42; }' | $CC -dynamiclib -xc - -o $t/libchoice.dylib
cat <<EOF | $CC -c -xc - -o $t/main.o
int choice();
int main() { return choice() != 42; }
EOF
for strip in -dead_strip_dylibs -dead_strip_dylibs,-dead_strip; do
  $CC --ld-path=$mold $t/main.o $t/libchoice.dylib -Wl,-no_fixup_chains,$strip -o $t/exe
  $t/exe
  otool -L $t/exe > $t/libs
  grep -q libSystem $t/libs
  grep -q libchoice $t/libs
done
