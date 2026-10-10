#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void _start() {}
EOF

$CC -B. -o $t/exe1 $t/a.o -nostdlib -Wl,-nmagic
$CC -B. -o $t/exe2 $t/a.o -nostdlib
$CC -B. -o $t/exe3 $t/a.o -nostdlib -Wl,-n

end1=$(nm $t/exe1 | grep ' end$' | cut -d' ' -f1)
end2=$(nm $t/exe2 | grep ' end$' | cut -d' ' -f1)
end3=$(nm $t/exe3 | grep ' end$' | cut -d' ' -f1)

[ $((0x$end1)) -lt $((0x$end2)) ]
[ $end1 = $end3 ]
