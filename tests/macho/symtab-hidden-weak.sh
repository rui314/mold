#!/bin/bash
source "$(dirname "$0")"/common.inc

# A weak definition with hidden visibility (an inline function's
# linkonce_odr copy) is a private external; a final image lists it
# among its locals, and ld-prime keeps N_WEAK_DEF (0x80) in its n_desc.
cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((weak, visibility("hidden"))) int hidden_weak(void) { return 1; }
__attribute__((visibility("hidden"))) int hidden(void) { return 2; }
int main(void) { return hidden_weak() + hidden(); }
EOF
$CC --ld-path=$mold -o $t/exe $t/a.o

# Prints the n_type and n_desc of symbol $2 in $1.
type_and_desc() {
  python3 - "$@" <<'EOF'
import sys, macho
sym = next(s for s in macho.MachO(sys.argv[1]).symbols() if s.name == sys.argv[2].encode())
print(hex(sym.type), hex(sym.desc))
EOF
}

[ "$(type_and_desc $t/exe _hidden_weak)" = '0x1e 0x80' ]
[ "$(type_and_desc $t/exe _hidden)" = '0x1e 0x0' ]

# A -r output makes it local too, and keeps N_WEAK_DEF there as well.
$mold -arch $ARCH -r $t/a.o -o $t/r.o
[ "$(type_and_desc $t/r.o _hidden_weak)" = '0x1e 0x80' ]
