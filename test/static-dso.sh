#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A shared object explicitly named on the command line cannot be linked
# into a static executable. https://github.com/rui314/mold/issues/1616

cat <<EOF | $CC -fPIC -shared -o $t/a.so -xc -
int fn(int a, int b) { return a + b; }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int fn(int, int);
int main() { return fn(1, 2) == 3 ? 0 : 1; }
EOF

! ./mold -o $t/exe1 -static $t/b.o $t/a.so 2> $t/log
grep 'attempted static link of a dynamic object' $t/log

# -Bdynamic cancels a preceding -Bstatic.
./mold -o $t/exe2 $t/b.o -Bstatic -Bdynamic $t/a.so
