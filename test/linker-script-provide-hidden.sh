#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

extern char linker_script_start_of_text[];
extern char linker_script_end_of_text[];

int main() {
  printf("start: %p\n", linker_script_start_of_text);
  printf("end:   %p\n", linker_script_end_of_text);
  if (linker_script_start_of_text == 0 || linker_script_end_of_text == 0) {
    return 1;
  }
  return 0;
}
EOF

cat <<EOF > $t/script.ld
PROVIDE_HIDDEN(linker_script_start_of_text = ADDR(.text));
PROVIDE_HIDDEN(linker_script_end_of_text = ADDR(.text) + SIZEOF(.text) + SIZEOF(malloc_hook));
EOF

$CC -B. -o $t/exe $t/a.o -T $t/script.ld
$QEMU $t/exe
