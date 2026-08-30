#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Currently, CREL is not supported on REL-type targets
[[ $MACHINE = arm* ]] && skip
[ $MACHINE = i686 ] && skip

clang_args=()
[ "$TRIPLE" = "" ] || clang_args+=(--target=$TRIPLE)

clang "${clang_args[@]}" -c -xc -o /dev/null /dev/null \
  -Wa,--crel,--allow-experimental-crel || skip

cat <<EOF | clang "${clang_args[@]}" -o $t/a.o -c -g -xc - \
  -Wa,--crel,--allow-experimental-crel
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

clang "${clang_args[@]}" -B. -o $t/exe $t/a.o
$QEMU $t/exe | grep 'Hello world'
readelf --debug-dump=info $t/exe | grep -E 'DW_AT_name.*: main$'
