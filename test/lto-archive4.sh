#!/bin/bash
. $(dirname $0)/common.inc

# Test that we don't get a spurious "duplicate symbol" error when an
# LTO compilation produces a symbol that was previously only provided
# by an archive member. After LTO, the archive member should be
# recognized as no longer needed.
#
# This simulates the real-world case where the LTO backend emits
# compiler-generated helpers (e.g. __udivsi3 on ARM) that also exist
# in runtime libraries like libgcc.
#
# Regression test for https://github.com/rui314/mold/issues/1421

test_cflags -flto || skip

# LTO object: defines asm_sym via inline asm (invisible to LTO plugin's
# symbol table) and references it. The plugin sees asm_sym as undefined,
# so the archive member below will be extracted to satisfy it.
cat <<EOF | $CC -o $t/a.o -c -xc - -flto
__asm__(".data\n"
        ".globl asm_sym\n"
        "asm_sym:\n"
        ".byte 42\n"
        ".text\n");
extern char asm_sym;
void baz() { *(volatile char *)&asm_sym; }
EOF

# Non-LTO archive member: defines asm_sym only.
# This will be extracted pre-LTO to satisfy the undefined reference,
# but after LTO the compiled output also provides asm_sym.
cat <<EOF | $CC -o $t/b.o -c -xc -
char asm_sym = 42;
EOF

rm -f $t/b.a
ar rc $t/b.a $t/b.o

cat <<EOF | $CC -o $t/c.o -c -xc - -flto
extern void baz(void);
int main() { baz(); }
EOF

$CC -B. -o $t/exe -flto $t/c.o $t/a.o $t/b.a
