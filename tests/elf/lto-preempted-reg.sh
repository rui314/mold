#!/usr/bin/env bash
. $(dirname $0)/common.inc

# An archive member whose definition preempts an LTO IR definition
# (LDPR_PREEMPTED_REG) must remain in the link after LTO, because the
# compiler backend drops its IR copy and emits a reference instead.

[ $MACHINE = $(uname -m) ] || skip

echo 'int main() {}' | $GXX -B. -flto -o /dev/null -xc++ - >& /dev/null || skip
echo 'int foo() { return 0; }' |
  $GXX -flto -fno-fat-lto-objects -c -o /dev/null -xc++ - >& /dev/null || skip

# weakfn() is emitted as a weak COMDAT definition by every translation unit
# that uses it. noinline keeps GCC from optimizing it away before the linker
# plugin sees it.
cat <<'EOF' > $t/weak.h
inline __attribute__((noinline)) int weakfn(int x) { return x * 3 + 1; }
EOF

# First member of libreg.a: weak-defines weakfn() and has dead code that
# references helper_fn(). Never referenced, so it must never be extracted.
cat <<'EOF' | $GXX -O2 -I$t -c -o $t/dead.o -xc++ -
#include "weak.h"
extern int helper_fn();
int dead_fn() { return weakfn(6) + helper_fn(); }
EOF

# Second member of libreg.a: also weak-defines weakfn(). Referenced from
# main(), so it is extracted before LTO.
cat <<'EOF' | $GXX -O2 -I$t -c -o $t/used.o -xc++ -
#include "weak.h"
int used_fn() { return weakfn(6); }
EOF

ar rcs $t/libreg.a $t/dead.o $t/used.o

# A thin-LTO member that emits weakfn() as an IR definition. Referenced
# from main(), so it is extracted before LTO.
cat <<'EOF' | $GXX -O2 -I$t -flto -fno-fat-lto-objects -c -o $t/ir_used.o -xc++ -
#include "weak.h"
int ir_fn() { return weakfn(6); }
EOF

# A thin-LTO member referenced only by dead.o's dead code, so it is not
# extracted before LTO.
cat <<'EOF' | $GXX -O2 -flto -fno-fat-lto-objects -c -o $t/ir_dead.o -xc++ -
int helper_fn() { return 1; }
EOF

ar rcs $t/libir.a $t/ir_used.o $t/ir_dead.o

# weakfn() resolves to used.o before LTO, so LTO emits a reference to it.
# used.o must still be there after LTO, or the tie is won by dead.o and the
# link fails on the unavailable helper_fn().
cat <<'EOF' | $GXX -O2 -c -o $t/main.o -xc++ -
extern int used_fn();
extern int ir_fn();
int main() { return used_fn() + ir_fn() == 38 ? 0 : 1; }
EOF

$GXX -B. -o $t/exe $t/main.o $t/libreg.a $t/libir.a
$QEMU $t/exe
