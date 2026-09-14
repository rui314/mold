#!/usr/bin/env bash
. $(dirname $0)/common.inc

root=$PWD/$t/root
mkdir -p "$root"
cat <<EOF | $CC -c -xc -o "$root/a.o" -
void _start() {}
int retained;
int discarded;
EOF
echo retained > "$root/symbols"

./mold --chroot "$root" --retain-symbols-file=/symbols /a.o -o $t/exe
readelf -Ws $t/exe > $t/log
grep ' retained$' $t/log
not grep ' discarded$' $t/log
