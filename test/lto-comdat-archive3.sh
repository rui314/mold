#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Clang variant of lto-comdat-archive2.sh. Unlike GCC, LLVM keeps claimed
# COMDAT groups in its LTO output, so the LTO-generated file itself must be
# allowed to win the group while the archive member extracted after LTO must
# still lose it. Clang emits cnt as a weak symbol, so a wrong link does not
# necessarily fail; the exit code check below catches two live copies too.

[ $MACHINE = $(uname -m) ] || skip

CLANG="${TEST_CLANG:-clang}"
$CLANG --version >& /dev/null || skip
echo 'int main() {}' | $CLANG -B. -flto -o /dev/null -xc - >& /dev/null || skip
echo '__int128 x;' | $CLANG -c -o /dev/null -xc - >& /dev/null || skip

cat <<'EOF' > $t/uniq.h
inline int bump() {
  static int cnt;
  return ++cnt;
}
EOF

# See lto-comdat-archive2.sh for why __divti3.
cat <<'EOF' | $CLANG -O2 -I$t -c -o $t/a.o -xc++ -
#include "uniq.h"
// The casts keep the division 64-bit so that it doesn't recurse.
extern "C" __int128 __divti3(__int128 a, __int128 b) {
  return (long long)a / (long long)b + bump();
}
EOF

ar rcs $t/libutil.a $t/a.o

cat <<'EOF' | $CLANG -O2 -I$t -flto -c -o $t/main.o -xc++ -
#include "uniq.h"
volatile long long lo = 21, d = 7;
int main() {
  bump();
  __int128 a = lo, b = d;
  return a / b == 5 ? 0 : 1;
}
EOF

$CLANG -B. -O2 -flto -o $t/exe $t/main.o $t/libutil.a
$t/exe
