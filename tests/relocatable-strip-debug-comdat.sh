#!/usr/bin/env bash
. $(dirname $0)/common.inc

# -g3 puts macro information into COMDAT groups whose members are all
# debug sections. --strip-debug discards the members, and the group must
# go with them, as other tools reject an empty group section.
cat <<EOF | $CC -c -g3 -o $t/a.o -xc -
#include <stdio.h>
#define FOO 1
int main() { return FOO; }
EOF

readelf --section-groups $t/a.o | grep -q debug_macro || skip

./mold -r --strip-debug -o $t/b.o $t/a.o
readelf --section-groups $t/b.o > $t/log
not grep -q 'contains 0 sections' $t/log
$OBJDUMP -h $t/b.o > /dev/null
