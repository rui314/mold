#!/usr/bin/env bash
. $(dirname $0)/common.inc

# See icf.sh.
[ $MACHINE = ppc64 ] && skip

# These targets store addends in the relocated places, so the contents of
# f1 and f2 differ. GNU as does so for Power10's PC-relative instructions
# as well.
[[ $MACHINE = i686 || $MACHINE = arm* || $MACHINE = sh4* || $CPU = power10 ]] && skip

# On LoongArch, f1 and f2 have R_LARCH_ALIGN relocations, which refer to a
# placeholder symbol that GNU as defines once per file, in whichever
# section first needs it. ICF doesn't ignore the symbol yet.
[[ $MACHINE = loongarch* ]] && skip

# f1 and f2 return the same string literal, which is at different offsets
# in the two files' mergeable string sections. ICF must merge them anyway.
cat <<EOF | $CC -c -o $t/a.o -O2 -ffunction-sections -xc -
const char *g(void) { return "a longer string"; }
const char *f1(void) { return "foo"; }
EOF

cat <<EOF | $CC -c -o $t/b.o -O2 -ffunction-sections -xc -
const char *f2(void) { return "foo"; }
EOF

cat <<EOF | $CC -c -o $t/c.o -xc -
#include <stdio.h>

const char *f1(void);
const char *f2(void);

int main() {
  printf("%d %s\n", (long)f1 == (long)f2, f2());
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o $t/c.o -Wl,--icf=all
$QEMU $t/exe | grep '^1 foo$'
