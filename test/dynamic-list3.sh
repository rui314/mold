#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' > $t/dyn
{
  xyz;
  foo*bar*[abc]x;
};
EOF

cat <<EOF | $CXX -fPIC -c -o $t/b.o -xc -
void xyz() {}
void foobarzx() {}
void foobarcx() {}
void foo123bar456bx() {}
void foo123bar456c() {}
void foo123bar456x() {}
int main() {}
EOF

$CC -B. -Wl,--dynamic-list=$t/dyn -o $t/exe1 $t/b.o

readelf --dyn-syms $t/exe1 > $t/log1
grep ' xyz' $t/log1
not grep ' foobarzx' $t/log1
grep ' foobarcx' $t/log1
grep ' foo123bar456bx' $t/log1
not grep ' foo123bar456c' $t/log1
not grep ' foo123bar456x' $t/log1

$CC -B. -Wl,--export-dynamic-symbol-list=$t/dyn -o $t/exe2 $t/b.o

readelf --dyn-syms $t/exe2 > $t/log2
grep ' xyz' $t/log2
not grep ' foobarzx' $t/log2
grep ' foobarcx' $t/log2
grep ' foo123bar456bx' $t/log2
not grep ' foo123bar456c' $t/log2
not grep ' foo123bar456x' $t/log2
