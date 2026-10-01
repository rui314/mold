#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Swift promises no function an address of its own, so ld-prime folds a
# function whose atom a Swift-mangled symbol ("_$s...") names even if
# its address is taken (a coroutine's resume function, a value witness)
# or it is exported. It goes by the name the atom is known by: of the
# labels at its start, an exported one before a local one, and among
# locals the greatest. ld-prime deduplicates at -O1 and up or with
# -deduplicate.
if [ $ARCH = arm64 ]; then
  body() { echo "mov w0, #$1"; echo ret; }
else
  body() { echo "movl \$$1, %eax"; echo ret; echo nop; echo nop; }
fi

{
  echo '.subsections_via_symbols'
  echo '.text'
  # Local Swift functions
  echo '"_$s1a":'; body 1
  echo '"_$s1b":'; body 1
  # Exported Swift functions
  echo '.globl "_$s2a", "_$s2b"'
  echo '"_$s2a":'; body 2
  echo '"_$s2b":'; body 2
  # Not Swift's mangling
  echo '"_$S3a":'; body 3
  echo '"_$S3b":'; body 3
  # Named after their local label _x, which outranks the Swift one
  echo '"_$s4a":'; echo '_x4a:'; body 4
  echo '"_$s4b":'; echo '_x4b:'; body 4
  # Named after their exported Swift label
  echo '.globl "_$s5a", "_$s5b"'
  echo '_x5a:'; echo '"_$s5a":'; body 5
  echo '_x5b:'; echo '"_$s5b":'; body 5
  echo '.data'
  echo '.globl _ptrs'
  echo '.p2align 3'
  echo '_ptrs:'
  echo '.quad "_$s1a", "_$s1b", "_$s2a", "_$s2b", "_$S3a", "_$S3b"'
  echo '.quad "_$s4a", "_$s4b", _x5a, _x5b'
} > $t/a.s
$CC -o $t/a.o -c $t/a.s

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
extern void *ptrs[10];
int main() {
  for (int i = 0; i < 10; i += 2)
    printf("%d", ptrs[i] == ptrs[i + 1]);
  printf("\n");
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-deduplicate
$t/exe | grep '^11001$'

$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o -Wl,-no_deduplicate
$t/exe2 | grep '^00000$'
