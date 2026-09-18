#!/usr/bin/env bash
. $(dirname $0)/common.inc

echo 'int data;' | $CC -c -o $t/a.o -xc -

./mold -o $t/exe $t/a.o -e data \
  --defsym=max_dec=18446744073709551615 \
  --defsym=max_hex=0xffffffffffffffff --defsym=zero=0
nm --format=posix $t/exe > $t/symbols
awk '
  $1 == "max_dec" { dec = $3 }
  $1 == "max_hex" { hex = $3 }
  $1 == "zero" { zero = $3; found_zero = 1 }
  END { exit !(dec != "" && dec == hex && found_zero && zero == 0) }
' $t/symbols

for value in 18446744073709551616 99999999999999999999999999999999999999 \
             0x10000000000000000; do
  not ./mold -o $t/overflow $t/a.o -e data --defsym=overflow=$value > $t/log 2>&1
  grep -F -- "-defsym: not a number: $value" $t/log
done
