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
readelf -Ws $t/direct | grep -E ' _start( |$)'
readelf -Ws $t/script | grep -E ' _start( |$)'

# A relative member path is relative to the archive in the root.
mkdir -p "$root/sub"
cp "$root$member" "$root/sub/rel.o"
rm -f "$root/sub/rel.a"
(cd "$root/sub"; ar crsT rel.a rel.o)
./mold --chroot "$root" --whole-archive /sub/rel.a -o $t/relative
readelf -Ws $t/relative | grep -E ' _start( |$)'
