#!/bin/bash
. $(dirname $0)/common.inc

# Depending on the symbol, mold relaxes a TLSDESC sequence into a local-exec
# (LE) or initial-exec (IE) sequence. The four-instruction sequence is rewritten
# in place: the ADRP and LDR become NOPs, and the ADD and BLR become the two
# instructions that materialize the address. With --emit-relocs, the emitted
# relocations must be rewritten to match whichever relaxation was applied.

cat <<'EOF' | $CC -o $t/a.o -c -xc - -fPIC
_Thread_local char foo[4] = "foo";
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
$QEMU $t/exe1 | grep -F 'foo bar'

$OBJDUMP -dr $t/exe1 > $t/exe1.objdump

# The ADD/BLR became MOVZ/MOVK that build the TP-relative offset.
grep -Fw R_AARCH64_TLSLE_MOVW_TPREL_G1 $t/exe1.objdump
grep -Fw R_AARCH64_TLSLE_MOVW_TPREL_G0_NC $t/exe1.objdump

# No TLSDESC relocation survives.
not grep -Fw R_AARCH64_TLSDESC_ADR_PAGE21 $t/exe1.objdump
not grep -Fw R_AARCH64_TLSDESC_LD64_LO12 $t/exe1.objdump
not grep -Fw R_AARCH64_TLSDESC_ADD_LO12 $t/exe1.objdump
not grep -Fw R_AARCH64_TLSDESC_CALL $t/exe1.objdump

# The G1 relocation sits on a movz and the G0_NC on a movk, not on the leading
# adrp/ldr (which are now nops).
awk '
  $3 == "movz" || $3 == "movk" || $3 == "nop" { insn[$1] = $3 }
  $2 == "R_AARCH64_TLSLE_MOVW_TPREL_G1"    { if (insn[$1] != "movz") { bad=1; print "bad offset: " $0 } }
  $2 == "R_AARCH64_TLSLE_MOVW_TPREL_G0_NC" { if (insn[$1] != "movk") { bad=1; print "bad offset: " $0 } }
  END { exit bad+0 }
' $t/exe1.objdump

#
# TLS defined in a shared library: TLSDESC => IE
#
$CC -B. -shared -o $t/d.so $t/a.o -Wl,--relax
$CC -B. -o $t/exe2 $t/b.o $t/c.o $t/d.so -Wl,--emit-relocs,--relax -Wl,-rpath=$t
$QEMU $t/exe2 | grep -F 'foo bar'

$OBJDUMP -dr $t/exe2 > $t/exe2.objdump

# The ADD/BLR became ADRP/LDR that load the address from the GOT.
grep -Fw R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21 $t/exe2.objdump
grep -Fw R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC $t/exe2.objdump
not grep -Fw R_AARCH64_TLSDESC_ADD_LO12 $t/exe2.objdump
not grep -Fw R_AARCH64_TLSDESC_CALL $t/exe2.objdump

awk '
  $3 == "adrp" || $3 == "ldr" || $3 == "nop" { insn[$1] = $3 }
  $2 == "R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21"  { if (insn[$1] != "adrp") { bad=1; print "bad offset: " $0 } }
  $2 == "R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC" { if (insn[$1] != "ldr") { bad=1; print "bad offset: " $0 } }
  END { exit bad+0 }
' $t/exe2.objdump

#
# Relocatable output: no relaxation happens and TLSDESC is not downgraded, so
# every relocation must be passed through unchanged rather than rewritten as if
# it had been relaxed to LE/IE.
#
$CC -B. -o $t/reloc.o $t/b.o -r
$OBJDUMP -r $t/reloc.o > $t/reloc.objdump
grep -Fw R_AARCH64_TLSDESC_ADR_PAGE21 $t/reloc.objdump
grep -Fw R_AARCH64_TLSDESC_LD64_LO12 $t/reloc.objdump
grep -Fw R_AARCH64_TLSDESC_ADD_LO12 $t/reloc.objdump
grep -Fw R_AARCH64_TLSDESC_CALL $t/reloc.objdump
not grep -Fw R_AARCH64_TLSLE_MOVW_TPREL_G1 $t/reloc.objdump
not grep -Fw R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21 $t/reloc.objdump
