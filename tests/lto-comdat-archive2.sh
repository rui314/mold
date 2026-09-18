#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Regression test for https://github.com/rui314/mold/issues/1637.
# In the pre-LTO round, the only reachable file holding the COMDAT group for
# `cnt` is an IR file, so mold reports the IR definition as prevailing, and
# GCC emits `cnt` into the LTO result as a plain global with no COMDAT group.
# In the post-LTO round, a.o becomes reachable because the LTO-generated code
# calls __divti3. a.o must not win the COMDAT group it carries: its copy of
# `cnt` would collide with the definition mold asked GCC to emit.

[ $MACHINE = $(uname -m) ] || skip

echo 'int main() {}' | $GXX -B. -flto -o /dev/null -xc++ - >& /dev/null || skip
echo 'int foo() { return 0; }' |
  $GXX -flto -fno-fat-lto-objects -c -o /dev/null -xc++ - >& /dev/null || skip
# The scheme below needs a target that lowers 128-bit division to a
# __divti3 call. Some lower it inline (e.g. POWER10), and 32-bit targets
# don't support __int128 at all.
echo 'volatile long long x, y; __int128 f() { return (__int128)x / y; }' |
  $GXX -O2 -S -o - -xc++ - 2> /dev/null | grep -q __divti3 || skip

# GCC emits the function-local static of an inline function as an
# STB_GNU_UNIQUE symbol in a COMDAT group. The non-weak binding is what turns
# a surviving second copy into a duplicate symbol error.
cat <<'EOF' > $t/uniq.h
inline int bump() {
  static int cnt;
  return ++cnt;
}
EOF

# Non-LTO archive member. Its only entry point is __divti3, which nothing
# references until the LTO backend expands main()'s 128-bit division to a
# libcall, so the member is extracted only in the post-LTO round. The archive
# precedes libgcc on the command line, so this copy shadows libgcc's.
# __udivti3 would not work: libstdc++.so carries an undefined reference to
# it, which would extract the member before LTO.
cat <<'EOF' | $GXX -O2 -I$t -c -o $t/a.o -xc++ -
#include "uniq.h"
// The casts keep the division 64-bit so that it doesn't recurse.
extern "C" __int128 __divti3(__int128 a, __int128 b) {
  return (long long)a / (long long)b + bump();
}
EOF

readelf -sW $t/a.o | grep -q 'UNIQUE.*_ZZ4bumpvE3cnt' || skip

ar rcs $t/libutil.a $t/a.o

# The 128-bit division compiles to a __divti3 call. Both bump() calls must
# increment the same cnt, so a / b is 21 / 7 plus the second increment.
# A wrong link either fails with a duplicate symbol error or leaves two
# copies of cnt, in which case a / b evaluates to 4.
cat <<'EOF' | $GXX -O2 -I$t -flto -fno-fat-lto-objects -c -o $t/main.o -xc++ -
#include "uniq.h"
volatile long long lo = 21, d = 7;
int main() {
  bump();
  __int128 a = lo, b = d;
  return a / b == 5 ? 0 : 1;
}
EOF

$GXX -B. -O2 -flto -o $t/exe $t/main.o $t/libutil.a
$t/exe
