#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -fno-PIC -
__attribute__((section(".sum"))) __attribute__((aligned(8192))) void sum() {};
int main() {}
EOF

$CC -B. -o $t/exe1 $t/a.o
lines=$(readelf -Wl $t/exe1 | grep "LOAD")

while IFS= read -ra line; do
  read -ra row <<< "$line"
  offset=${row[1]}
  vaddr=${row[2]}
  align=${row[-1]}
  if (( offset % align != vaddr % align )); then
    false
  fi
done <<< "$lines"
