#!/bin/bash
source "$(dirname "$0")"/common.inc

# Dead stripping keeps an -alias base, which ld-prime counts among the
# initial undefines (-why_live says so), but drops the alias itself
# where nothing exports or refers to it: an executable's.
cat <<EOF | $CC -c -xc - -o $t/a.o
int base(void) { return 3; }
int main() { return 0; }
EOF
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip,-alias,_base,_other \
  -Wl,-why_live,_base 2> $t/log
nm -m $t/exe > $t/nm
grep -q 'external _base$' $t/nm
not grep -q _other $t/nm
grep -A1 '^_base from ' $t/log | grep -q '^  initial-undef$'

# An exported alias stays.
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-dead_strip,-alias,_base,_other,-export_dynamic
nm -m $t/exe2 > $t/nm2
grep -q 'external _base$' $t/nm2
grep -q 'external _other$' $t/nm2
