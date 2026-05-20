#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

extern char linker_script_start_of_text[];

int main() {
  printf("start: %p\n", linker_script_start_of_text);
  return 0;
}
EOF

cat <<EOF > $t/script.ld
PROVIDE_HIDDEN(linker_script_start_of_text = ADDR(.text) + ADDR(.data));
EOF

# We expect the link to fail with the specific error message
not $CC -B. -o $t/exe $t/a.o -T $t/script.ld 2>&1 | grep "invalid addition of two section-relative values: .text and .data"
