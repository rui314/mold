#!/bin/bash
source "$(dirname "$0")"/common.inc

# -trace_symbol_layout reports where -move_to_rw_segment,
# -move_to_ro_segment and -dirty_data_list put each symbol of the atoms
# they move, on stdout, and the renames that then apply to its section;
# -trace_symbol_layout_file writes the report into a file instead.
# (ld-prime reports every other symbol too, which mold doesn't.)
cat <<EOF | $CC -o $t/a.o -c -xc -
int data1 = 1;
int data2 = 2;
int bss1;
const int const1 = 3;
int func1(void) { return data1 + data2 + bss1 + const1; }
int main() { return func1() != 6; }
EOF

printf '_data1\n_const1\n_func1\n' > $t/rw.txt
printf '_func1\n' > $t/ro.txt
printf '_data2\n_bss1\n' > $t/dirty.txt
$CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-move_to_rw_segment,__FOO,$t/rw.txt \
  -Wl,-move_to_ro_segment,__BAR,$t/ro.txt -Wl,-dirty_data_list,$t/dirty.txt \
  -Wl,-rename_section,__FOO,__data,__BAZ,__baz -Wl,-rename_segment,__FOO,__QUX \
  -Wl,-trace_symbol_layout > $t/log1 2> /dev/null
grep -qx "symbol '_func1', -move_to_ro_segment mapped it to __BAR/__text" $t/log1
grep -qx "symbol '_data1', -move_to_rw_segment mapped it to __FOO/__data" $t/log1
grep -qx "symbol '_data1', -rename_section mapped it to __BAZ/__baz" $t/log1
grep -qx "symbol '_const1', -move_to_rw_segment mapped it to __FOO/__const" $t/log1
grep -qx "symbol '_const1', -rename_segment mapped it to __QUX/__const" $t/log1
grep -qx "symbol '_data2', -dirty_data_list mapped it to __DATA_DIRTY/__data" $t/log1
grep -qx "symbol '_bss1', -dirty_data_list mapped it to __DATA_DIRTY/__common" $t/log1

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-move_to_rw_segment,__FOO,$t/rw.txt \
  -Wl,-trace_symbol_layout_file,$t/trace -Wl,-trace_symbol_layout > $t/log2 2> /dev/null
not grep -q symbol $t/log2
grep -qx "symbol '_data1', -move_to_rw_segment mapped it to __FOO/__data" $t/trace

# ld-prime warns about a file it can't write, and reports nothing then.
$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-move_to_rw_segment,__FOO,$t/rw.txt \
  -Wl,-trace_symbol_layout_file,$t/none/trace -Wl,-trace_symbol_layout > $t/log3 2> $t/err3
grep -q "warning: could not open -trace_symbol_layout_file $t/none/trace for writing (2)" $t/err3
not grep -q symbol $t/log3
