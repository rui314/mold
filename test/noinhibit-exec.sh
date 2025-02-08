#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fno-PIC
int main() {}
EOF

$CC -B. -shared -o $t/b.so $t/a.o

not $CC -B. -o $t/b.so $t/a.o -Wl,-require-defined=no-such-sym |&
  grep -q 'undefined symbol: no-such-sym'

$CC -B. -shared -o $t/b.o $t/a.o -Wl,-require-defined=no-such-sym,-noinhibit-exec |&
  grep -q 'undefined symbol: no-such-sym'
