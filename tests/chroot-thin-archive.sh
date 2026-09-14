#!/usr/bin/env bash
. $(dirname $0)/common.inc

root=$PWD/$t/root
member=$PWD/$t/member.o
mkdir -p "$root$(dirname "$member")"
echo 'void _start() {}' | $CC -c -xc -o "$member" -
echo '' | $CC -c -xc -o "$root/empty.o" -
rm -f "$root/lib.a"
ar crsT "$root/lib.a" "$member"
mv "$member" "$root$member"
echo 'INPUT(/lib.a)' > "$root/script.ld"

# Both target detection and archives reached through scripts must resolve
# the absolute member path inside the selected root.
./mold --chroot "$root" --whole-archive /lib.a -o $t/direct
./mold --chroot "$root" --whole-archive /empty.o /script.ld -o $t/script
readelf -Ws $t/direct | grep ' _start$'
readelf -Ws $t/script | grep ' _start$'
