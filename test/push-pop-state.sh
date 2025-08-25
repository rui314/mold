#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -shared -o $t/a.so -xc -
int foo = 1;
EOF

cat <<EOF | $CC -shared -o $t/b.so -xc -
int bar = 1;
EOF

cat <<EOF | $CC -c -o $t/c.o -xc -
int main() {}
EOF

$CC -B. -o $t/exe $t/c.o -Wl,-as-needed \
  -Wl,-push-state -Wl,-no-as-needed $t/a.so -Wl,-pop-state $t/b.so

readelf --dynamic $t/exe > $t/log
grep -F a.so $t/log
not grep -F b.so $t/log

$CC -no-pie -B. -o $t/exe $t/c.o -Wl,-no-as-needed \
  -Wl,-push-state,-Bstatic -lstdc++ -Wl,-pop-state -lc

readelf --dynamic $t/exe > $t/log
not grep -F libstdc++ $t/log
grep -F libc $t/log
