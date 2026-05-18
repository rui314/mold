#!/bin/bash
. $(dirname $0)/common.inc

# Regression test: an unreferenced GCC slim LTO object inside an archive should
# not abort a mixed LLVM-LTO link.

[ $MACHINE = $(uname -m) ] || skip

CLANG="${TEST_CLANG:-clang}"
$CLANG --version >& /dev/null || skip
$GCC --version >& /dev/null || skip

echo 'int main() {}' | $CLANG -B. -flto -o /dev/null -xc - >& /dev/null || skip
echo 'int foo() { return 0; }' |
  $GCC -flto -fno-fat-lto-objects -c -o /dev/null -xc - >& /dev/null || skip

cat <<'EOF' | $CLANG -flto -c -o $t/main.o -xc -
int main(void) { return 0; }
EOF

cat <<'EOF' | $GCC -flto -fno-fat-lto-objects -c -o $t/foo.c.o -xc -
int foo(void) { return 7; }
EOF

ar rcs $t/libfoo.a $t/foo.c.o
$CLANG -B. -flto -o $t/exe $t/main.o $t/libfoo.a

$QEMU $t/exe
