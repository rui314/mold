#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void _start() {}
EOF

$CC -B. -o $t/exe1 $t/a.o -nostdlib -Wl,-nmagic
$CC -B. -o $t/exe2 $t/a.o -nostdlib

end1=$(nm $t/exe1 | grep ' end$' | cut -d' ' -f1)
end2=$(nm $t/exe2 | grep ' end$' | cut -d' ' -f1)

[ $end1 -lt $end2 ]
