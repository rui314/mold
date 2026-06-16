#!/bin/bash
. $(dirname $0)/common.inc

# mold relaxes a GOT load of a non-preemptible symbol from
#   mov foo@GOT(%ebx), %reg   =>   lea foo@GOTOFF(%ebx), %reg
# With --emit-relocs the R_386_GOT32X relocation must be rewritten to
# R_386_GOTOFF to match the lea (GNU ld does the same).

cat <<'EOF' | $CC -o $t/a.o -c -xc -
int foo = 42;
EOF

cat <<'EOF' | $CC -fPIC -O2 -o $t/b.o -c -xc -
extern int foo;
int get(void) { return foo; }
EOF

cat <<'EOF' | $CC -o $t/c.o -c -xc -
extern int get(void);
int main(void) { return get() != 42; }   // exits 0 iff foo resolved to 42
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o $t/c.o -no-pie -Wl,--emit-relocs
$QEMU $t/exe

$OBJDUMP -dr $t/exe > $t/exe.objdump

# The GOT load of foo was relaxed and its relocation rewritten to GOTOFF. The
# compiler only emits GOT32X for the extern access, so a GOTOFF naming foo can
# only come from the relaxation.
grep -E $'R_386_GOTOFF[ \t]+foo' $t/exe.objdump
not grep -E $'R_386_GOT32X[ \t]+foo' $t/exe.objdump
