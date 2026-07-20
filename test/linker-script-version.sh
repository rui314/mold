#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Symbol versions must be defined in the command line order even
# though input files are read in parallel.

cat <<EOF > $t/a.script
VERSION {
  ver_a { global: foo; };
}
EOF

cat <<EOF > $t/b.script
VERSION {
  ver_b { global: bar; };
}
EOF

cat <<EOF | $CC -xc -fPIC -c -o $t/c.o -
void foo() {}
void bar() {}
EOF

$CC -B. -shared -o $t/d.so $t/a.script $t/b.script $t/c.o

readelf --version-info $t/d.so > $t/log
grep -o 'Name: ver_[ab]' $t/log > $t/order

diff $t/order - <<EOF
Name: ver_a
Name: ver_b
EOF
