#!/bin/bash
source "$(dirname "$0")"/common.inc

# A C-string section may end in bytes without a NUL, which no compiler
# writes. They are linked as they are, a string of their own: merged
# with no NUL-terminated one, in a final image and a -r output alike.
# (ld-prime completes them with a NUL.)
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__cstring,cstring_literals
.asciz "xy"
.globl _tail
_tail: .ascii "abc"
EOF
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__cstring,cstring_literals
L_str: .asciz "abc"
.data
.globl _ptr
.p2align 3
_ptr: .quad L_str
EOF
cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
#include <string.h>
extern const char tail[];
extern const char *ptr;
int main() { printf("%s %d %d\n", ptr, !strncmp(tail, "abc", 3), tail == ptr); }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/main.o
$RUN $t/exe > $t/out
grep -qx 'abc 1 0' $t/out

if $mold -v 2> /dev/null | grep -q mold-macho; then
  # "xy\0", "abc" and "abc\0".
  $mold -arch $ARCH -r -o $t/r.o $t/a.o $t/b.o
  size=$(otool -l $t/r.o | awk '$1 == "sectname" { n = $2 } n == "__cstring" && $1 == "size" { print $2 }')
  [ $((size)) = 10 ]
  $CC --ld-path=$mold -o $t/exe2 $t/r.o $t/main.o
  $RUN $t/exe2 > $t/out2
  grep -qx 'abc 1 0' $t/out2
fi
