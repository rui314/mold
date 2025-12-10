#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -shared -o $t/a.so -xc - -fPIC
int foo = 1;
EOF

cat <<EOF | $CC -shared -o $t/b.so -xc - -fPIC
int bar = 1;
EOF

cat <<EOF | $CC -c -o $t/c.o -xc - -fPIC
int main() {}
EOF

$CC -B. -o $t/exe1 $t/c.o -Wl,-as-needed \
  -Wl,-push-state -Wl,-no-as-needed $t/a.so -Wl,-pop-state $t/b.so

readelf --dynamic $t/exe1 > $t/log1
grep -F a.so $t/log1
not grep -F b.so $t/log1

if test_cflags -static; then
  $CC -B. -o $t/exe2 $t/c.o -no-pie -static
  readelf --dynamic $t/exe2 | grep -F 'no dynamic section'

  $CC -B. -o $t/exe3 $t/c.o -no-pie -Wl,-push-state,-static,-pop-state
  readelf --dynamic $t/exe3 | grep -F libc
fi
