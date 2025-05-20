#!/bin/bash
. $(dirname $0)/common.inc

nm mold | grep '__tsan_init' && skip

cat <<EOF | $CC -o $t/a.o -c -xc - -g -gdwarf64 || skip
void foo() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -g -gdwarf32
void bar() {}
EOF

./mold -r -o $t/c.o $t/a.o $t/b.o

cat <<EOF | $CC -o $t/d.o -c -xc - -g -gdwarf64
int main() {}
EOF

MOLD_DEBUG=1 $CC -B. -o $t/exe1 $t/c.o $t/d.o -g -Wl,-Map=$t/map1
sed -n '/\/c\.o:(.debug_info)/,$p' $t/map1 | grep -F '/d.o:(.debug_info)'

MOLD_DEBUG=1 $CC -B. -o $t/exe2 $t/d.o $t/c.o -g -Wl,-Map=$t/map2
sed -n '/\/c\.o:(.debug_info)/,$p' $t/map2 | grep -F '/d.o:(.debug_info)'
