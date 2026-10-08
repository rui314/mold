#!/usr/bin/env bash
. $(dirname $0)/common.inc

# The ELF and program headers are not sections, so they don't get
# __start_ and __stop_ symbols even though their names are C identifiers.
cat <<EOF | $CC -c -o $t/a.o -xc -
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-z,start-stop-visibility=protected
readelf -sW $t/exe | not grep -E '__(start|stop)_(EHDR|PHDR)'
