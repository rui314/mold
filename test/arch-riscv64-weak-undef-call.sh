#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -static || skip

cat <<EOF | $CC -c -o $t/a.o -xassembler -
.globl foo
.weak bar
foo:
 call bar
 ret
EOF

$CC -B. -shared -o $t/b.so $t/a.o
$OBJDUMP -d $t/b.so | grep -E '\bjalr\b.*<bar\$plt>'
