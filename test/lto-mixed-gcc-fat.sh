#!/bin/bash
. $(dirname $0)/common.inc

# Regression test: link LLVM IR and GCC FAT LTO objects in one link.
# GCC FAT LTO objects should fall back to regular ELF handling when
# the active plugin is LLVM's.

[ $MACHINE = $(uname -m) ] || skip

CLANG="${TEST_CLANG:-clang}"
$CLANG --version >& /dev/null || skip
$GCC --version >& /dev/null || skip

echo 'int main() {}' | $CLANG -B. -flto -o /dev/null -xc - >& /dev/null || skip
echo 'int foo() { return 0; }' |
  $GCC -flto -ffat-lto-objects -c -o /dev/null -xc - >& /dev/null || skip

cat <<'EOF' | $CLANG -flto -c -o $t/main.o -xc -
int foo(void);
int main(void) { return foo(); }
EOF

cat <<'EOF' | $GCC -flto -ffat-lto-objects -c -o $t/foo.o -xc -
int foo(void) { return 0; }
EOF

$CLANG -B. -flto -o $t/exe $t/main.o $t/foo.o
$QEMU $t/exe
