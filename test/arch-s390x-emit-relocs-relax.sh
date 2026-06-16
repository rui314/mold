#!/bin/bash
. $(dirname $0)/common.inc

# mold relaxes a GOT-loading LGRL into an address-materializing LARL when the
# symbol's address is a link-time constant. With --emit-relocs the R_390_GOTENT
# relocation must be rewritten to R_390_PC32DBL to match the LARL; otherwise it
# would describe a GOT slot that the relaxation removed.

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

$CC -B. -o $t/exe $t/a.o $t/b.o $t/c.o -static -Wl,--emit-relocs
$QEMU $t/exe

$OBJDUMP -dr $t/exe > $t/exe.objdump

# The GOT load was relaxed to a direct larl of foo.
grep -E 'larl.*<foo>' $t/exe.objdump

# Its relocation was rewritten to match the larl, and the stale GOTENT is gone.
grep -E $'R_390_PC32DBL[ \t]+foo' $t/exe.objdump
not grep -E $'R_390_GOTENT[ \t]+foo' $t/exe.objdump
