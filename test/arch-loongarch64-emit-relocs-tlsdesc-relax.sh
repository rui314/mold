#!/bin/bash
. $(dirname $0)/common.inc

# Depending on the symbol, mold relaxes a TLSDESC sequence into a local-exec
# (LE) or initial-exec (IE) sequence, or keeps it as TLSDESC but folds its
# first two instructions into a single pcaddi. With --emit-relocs, the emitted
# relocations must be rewritten to match whichever relaxation was applied.

cat <<'EOF' | $CC -o $t/a.o -c -xc - -fPIC
_Thread_local char foo[4] = "foo";
_Thread_local char padding[100000] = "pad";
_Thread_local char bar[4] = "bar";
EOF

cat <<'EOF' | $CC -o $t/b.o -c -xc - -fPIC -mtls-dialect=desc -O2
extern _Thread_local char foo[4];
extern _Thread_local char bar[4];
char *get_foo() { return foo; }
char *get_bar() { return bar; }
EOF

cat <<EOF | $CC -o $t/c.o -c -xc - -mtls-dialect=desc
#include <stdio.h>
char *get_foo();
char *get_bar();
int main() { printf("%s %s\n", get_foo(), get_bar()); }
EOF

#
# Local TLS in an executable: TLSDESC => LE
#
$CC -B. -o $t/exe1 $t/a.o $t/b.o $t/c.o -Wl,--emit-relocs,--relax
{ $QEMU $t/exe1; true; } |& grep -q 'unexpected reloc type' && skip
$QEMU $t/exe1 | grep -F 'foo bar'

$OBJDUMP -dr $t/exe1 > $t/exe1.objdump

# foo has a small TP offset, so its sequence relaxes to a single instruction
# with only a low-part relocation. bar has a large offset, so it keeps both
# the high- and low-part relocations.
grep -E $'R_LARCH_TLS_LE_LO12[ \t]+foo' $t/exe1.objdump
not grep -E $'R_LARCH_TLS_LE_HI20[ \t]+foo' $t/exe1.objdump
grep -E $'R_LARCH_TLS_LE_HI20[ \t]+bar' $t/exe1.objdump
grep -E $'R_LARCH_TLS_LE_LO12[ \t]+bar' $t/exe1.objdump

# No TLSDESC relocation survives.
not grep -Fw R_LARCH_TLS_DESC_PC_HI20 $t/exe1.objdump
not grep -Fw R_LARCH_TLS_DESC_PC_LO12 $t/exe1.objdump
not grep -Fw R_LARCH_TLS_DESC_LD $t/exe1.objdump
not grep -Fw R_LARCH_TLS_DESC_CALL $t/exe1.objdump

#
# TLS defined in a shared library: TLSDESC => IE
#
$CC -B. -shared -o $t/d.so $t/a.o -Wl,--relax
$CC -B. -o $t/exe2 $t/b.o $t/c.o $t/d.so -Wl,--emit-relocs,--relax -Wl,-rpath=$t
$QEMU $t/exe2 | grep -F 'foo bar'

$OBJDUMP -dr $t/exe2 > $t/exe2.objdump
grep -Fw R_LARCH_TLS_IE_PC_HI20 $t/exe2.objdump
grep -Fw R_LARCH_TLS_IE_PC_LO12 $t/exe2.objdump
not grep -Fw R_LARCH_TLS_DESC_LD $t/exe2.objdump
not grep -Fw R_LARCH_TLS_DESC_CALL $t/exe2.objdump

#
# Shared library output: TLSDESC is kept, but pcalau12i+addi.d => pcaddi, so
# the low-part relocation becomes the pcaddi's TLS_DESC_PCREL20_S2 while the
# ld.d/jirl relocations are left unchanged.
#
$CC -B. -shared -o $t/e.so $t/a.o $t/b.o -Wl,--emit-relocs,--relax
$OBJDUMP -dr $t/e.so > $t/e.objdump
grep -Fw R_LARCH_TLS_DESC_PCREL20_S2 $t/e.objdump
grep -Fw R_LARCH_TLS_DESC_LD $t/e.objdump
grep -Fw R_LARCH_TLS_DESC_CALL $t/e.objdump
not grep -Fw R_LARCH_TLS_DESC_PC_HI20 $t/e.objdump
not grep -Fw R_LARCH_TLS_DESC_PC_LO12 $t/e.objdump

#
# Relocatable output: no relaxation happens and TLSDESC is not downgraded, so
# every relocation must be passed through unchanged rather than rewritten as
# if it had been relaxed to LE/IE.
#
$CC -B. -o $t/reloc.o $t/b.o -r
$OBJDUMP -r $t/reloc.o > $t/reloc.objdump
grep -Fw R_LARCH_TLS_DESC_PC_HI20 $t/reloc.objdump
grep -Fw R_LARCH_TLS_DESC_PC_LO12 $t/reloc.objdump
grep -Fw R_LARCH_TLS_DESC_LD $t/reloc.objdump
grep -Fw R_LARCH_TLS_DESC_CALL $t/reloc.objdump
not grep -Fw R_LARCH_TLS_LE_HI20 $t/reloc.objdump
not grep -Fw R_LARCH_TLS_LE_LO12 $t/reloc.objdump
