#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
int main() {}
EOF

echo 'ENTRY(foo)' > $t/script

# ENTRY(foo) sets the entry point
$CC -B. -o $t/exe1 $t/a.o -Wl,-T,$t/script
sym=$(readelf -sW $t/exe1 | grep -w foo | awk '{print $2}' | sed 's/^0*//')
readelf -h $t/exe1 | grep "Entry point address:.*0x$sym"

# The -e option takes precedence over ENTRY()
$CC -B. -o $t/exe2 $t/a.o -Wl,-T,$t/script -Wl,-e,main
sym=$(readelf -sW $t/exe2 | grep -w main | awk '{print $2}' | sed 's/^0*//')
readelf -h $t/exe2 | grep "Entry point address:.*0x$sym"
