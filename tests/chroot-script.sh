#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A relative name in a script is looked up in the script's directory in
# the root.
root=$PWD/$t/root
mkdir -p "$root/sub"
echo 'void _start() {}' | $CC -c -xc -o "$root/sub/a.o" -
echo '' | $CC -c -xc -o "$root/empty.o" -
echo 'INPUT(a.o)' > "$root/sub/script.ld"

./mold --chroot "$root" /empty.o /sub/script.ld -o $t/exe
readelf -Ws $t/exe | grep -E ' _start( |$)'
