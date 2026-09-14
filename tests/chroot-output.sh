#!/usr/bin/env bash
. $(dirname $0)/common.inc

root=$PWD/$t/root
mkdir -p "$root"
echo 'void _start() {}' | $CC -c -xc -o "$root/a.o" -

for mode in executable relocatable; do
  flags=
  [ "$mode" = executable ] || flags=-r
  output=$PWD/$t/$mode
  mkdir -p "$(dirname "$root$output")"
  rm -f "$output" "$root$output"

  ./mold $flags --chroot "$root" /a.o -o "$output"
  test -f "$root$output"
  not test -e "$output"
  readelf -h "$root$output" | grep -q ELF
done
