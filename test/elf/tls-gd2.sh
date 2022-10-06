#!/bin/bash
. $(dirname $0)/common.inc

echo '{ global: bar; local: *; };' > $t/a.ver

cat <<EOF | $GCC $mtls -fPIC -c -o $t/b.o -xc -
__attribute__((tls_model("global-dynamic"))) _Thread_local int foo;

int bar() {
  return foo;
}
EOF

$CC -B. -shared -o $t/c.so $t/b.o -Wl,--version-script=$t/a.ver \
  -Wl,--no-relax

readelf -W --dyn-syms $t/c.so | grep -Eq 'TLS     LOCAL  DEFAULT .* foo'
