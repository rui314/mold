#!/bin/bash
source "$(dirname "$0")"/common.inc

# -trace_symbol_layout reports on stdout where each symbol of a
# subsection goes: where -move_to_rw_segment, -move_to_ro_segment and
# -dirty_data_list put the subsections they move, and the renames that
# then apply to its section; -trace_symbol_layout_file writes the report
# into a file instead.
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

# The other symbols get the default mapping, or ld-prime's own moves
# and the renames - both as one step, -rename_segment's if it applied.
# The subsections ld-prime makes itself, such as the thread-local
# variables' descriptors, come last, and the symbols at one place
# last-defined first. A -r link reports nothing.
cat <<EOF | $CC -o $t/b.o -c -xc -
int gdata = 1;
int *const cptr = &gdata;
__thread int tlv = 3;
int get(void) { return tlv; }
int main() { return *cptr + get(); }
EOF
$CC --ld-path=$mold -o $t/exe4 $t/b.o -Wl,-rename_section,__DATA,__data,__DATA,__d2 \
  -Wl,-rename_segment,__DATA,__D -Wl,-trace_symbol_layout > $t/log4 2> /dev/null
grep -qx "symbol '_main', use default mapping to __TEXT/__text" $t/log4
grep -qx "symbol '_gdata', -rename_segment mapped it to __D/__d2" $t/log4
grep -qx "symbol '_cptr', -data_const mapped it to __DATA_CONST/__const" $t/log4
tail -1 $t/log4 | grep -qx "symbol '_tlv', -rename_segment mapped it to __D/__thread_vars"

cat <<EOF | $CC -o $t/c.o -c -xassembler -
.globl _main, _zb, _ab
.p2align 2
_main:
  ret
lc:
_zb:
_ab:
  ret
.subsections_via_symbols
EOF
$CC --ld-path=$mold -o $t/exe5 $t/c.o -Wl,-trace_symbol_layout > $t/log5 2> /dev/null
[ "$(sed 's/,.*//' $t/log5 | tr '\n' ' ')" = "symbol '_main' symbol '_zb' symbol '_ab' symbol 'lc' " ]

$mold -r -arch $ARCH -o $t/r.o $t/b.o -trace_symbol_layout > $t/log6
[ ! -s $t/log6 ]

# Where -rename_section takes a section to another segment and
# -rename_segment then moves that one on, ld-prime gives the line of
# the move before them the segment -rename_section named (it renames
# the segment in place first), and then the renames as one step.
$CC --ld-path=$mold -o $t/exe7 $t/a.o -Wl,-move_to_rw_segment,__FOO,$t/rw.txt \
  -Wl,-rename_section,__FOO,__data,__BAR,__d2 -Wl,-rename_segment,__BAR,__QUX \
  -Wl,-trace_symbol_layout > $t/log7 2> /dev/null
grep -qx "symbol '_data1', -move_to_rw_segment mapped it to __BAR/__data" $t/log7
grep -qx "symbol '_data1', -rename_segment mapped it to __QUX/__d2" $t/log7
grep -qx "symbol '_const1', -move_to_rw_segment mapped it to __FOO/__const" $t/log7

$CC --ld-path=$mold -o $t/exe8 $t/b.o -Wl,-rename_section,__DATA_CONST,__const,__X,__c \
  -Wl,-rename_segment,__X,__Y -Wl,-trace_symbol_layout > $t/log8 2> /dev/null
grep -qx "symbol '_cptr', -data_const mapped it to __X/__const" $t/log8
grep -qx "symbol '_cptr', -rename_segment mapped it to __Y/__c" $t/log8
