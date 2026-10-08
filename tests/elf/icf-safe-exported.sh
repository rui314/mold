#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Other modules may compare the addresses of exported functions, so
# --icf=safe must not fold them even if nothing in the DSO takes them.
cat <<EOF | $CC -c -o $t/a.o -ffunction-sections -fPIC -xc -
int foo(int x) {
  return x * 7 + 3;
}

int bar(int x) {
  return x * 7 + 3;
}
EOF

$CC -B. -shared -o $t/b.so $t/a.o -Wl,-icf=safe
nm -D $t/b.so > $t/log
[ "$(grep -w foo $t/log | cut -d' ' -f1)" != "$(grep -w bar $t/log | cut -d' ' -f1)" ]
