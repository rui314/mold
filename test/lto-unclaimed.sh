#!/bin/bash
. $(dirname $0)/common.inc

[ "$CC" = cc ] || skip
test_cflags -flto || skip

# Create a fake LLVM bitcode file
echo -e 'BC\xc0\xde' > $t/a.o

rm -f $t/b.a
ar rc $t/b.a $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
int main() {}
EOF

# mold should not report an "not claimed by the LTO plugin" error
$CC -B. -o $t/exe -flto $t/c.o $t/b.a
