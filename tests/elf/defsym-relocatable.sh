#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void bar() {}
EOF

# --defsym also applies to a relocatable link. GNU ld and lld define both the
# alias and the absolute symbol.
$CC -B. -r -o $t/b.o $t/a.o -Wl,-defsym=extra=bar -Wl,-defsym=konst=0x1234

nm $t/b.o | grep -q 'T extra$'
nm $t/b.o | grep -q ' A konst$'
