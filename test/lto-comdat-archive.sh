#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Regression test for https://github.com/rui314/mold/issues/1565.
# COMDAT groups are selected again after LTO because symbol resolution is
# redone. If the first round's winner is an archive member that is not
# re-extracted because LTO eliminated all references to it, another member
# must provide the group in the second round even though its copy was
# discarded in the first round.

[ $MACHINE = $(uname -m) ] || skip

echo 'int main() {}' | $GXX -B. -flto -o /dev/null -xc++ - >& /dev/null || skip
echo 'int foo() { return 0; }' |
  $GXX -flto -fno-fat-lto-objects -c -o /dev/null -xc++ - >& /dev/null || skip

cat <<'EOF' > $t/conv.h
template <typename T>
__attribute__((noinline))
int convert(T x) {
  return (int)x + 1;
}
EOF

cat <<EOF | $GXX -O2 -I$t -c -o $t/a.o -xc++ -
#include "conv.h"
int do_a(int x) { return convert(x); }
EOF

cat <<EOF | $GXX -O2 -I$t -c -o $t/b.o -xc++ -
#include "conv.h"
int do_b(int x) { return convert(x) + 10; }
EOF

# a.o comes first, so it wins the COMDAT group for convert<int> when both
# members are extracted.
ar rcs $t/libutil.a $t/a.o $t/b.o

cat <<'EOF' | $GXX -O2 -flto -fno-fat-lto-objects -c -o $t/mid.o -xc++ -
int do_a(int);
int do_b(int);
int use_a(int x) { return do_a(x); }
int use_b(int x) { return do_b(x); }
EOF

ar rcs $t/libmid.a $t/mid.o

# main() calls only use_b(), so LTO eliminates use_a() and with it the only
# reference to do_a(). a.o is not re-extracted after LTO even though it won
# the COMDAT group before LTO.
cat <<'EOF' | $GXX -O2 -flto -fno-fat-lto-objects -c -o $t/main.o -xc++ -
int use_b(int);
int main() { return use_b(42) == 53 ? 0 : 1; }
EOF

$GXX -B. -O2 -flto -o $t/exe $t/main.o $t/libmid.a $t/libutil.a
$QEMU $t/exe

# Same but with the losing member linked directly instead of via an archive.
ar rcs $t/libutil2.a $t/a.o
$GXX -B. -O2 -flto -o $t/exe2 $t/main.o $t/libmid.a $t/libutil2.a $t/b.o
$QEMU $t/exe2
