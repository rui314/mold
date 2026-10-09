#!/bin/bash
source "$(dirname "$0")"/common.inc

# -trace_symbol_layout reports on stdout the output section each symbol
# of a subsection went to, after -move_to_rw_segment,
# -move_to_ro_segment, -dirty_data_list and the renames;
# -trace_symbol_layout_file writes the report into a file instead.
# (ld-prime reports each step that moved a symbol instead, in words of
# its own.)
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
sort $t/log1 > $t/sorted1
diff - $t/sorted1 <<EOF
symbol '_bss1', mapped to __DATA_DIRTY/__common
symbol '_const1', mapped to __QUX/__const
symbol '_data1', mapped to __BAZ/__baz
symbol '_data2', mapped to __DATA_DIRTY/__data
symbol '_func1', mapped to __BAR/__text
symbol '_main', mapped to __TEXT/__text
EOF

# The sections are the image's.
nm -m $t/exe1 > $t/nm1
grep -q '(__BAZ,__baz) external _data1$' $t/nm1
grep -q '(__BAR,__text) external _func1$' $t/nm1

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-move_to_rw_segment,__FOO,$t/rw.txt \
  -Wl,-trace_symbol_layout_file,$t/trace -Wl,-trace_symbol_layout > $t/log2 2> /dev/null
not grep -q symbol $t/log2
grep -qx "symbol '_data1', mapped to __FOO/__data" $t/trace

# A file that can't be written is a warning, and nothing is reported
# then.
$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-move_to_rw_segment,__FOO,$t/rw.txt \
  -Wl,-trace_symbol_layout_file,$t/none/trace -Wl,-trace_symbol_layout > $t/log3 2> $t/err3
grep -q "warning: could not open -trace_symbol_layout_file $t/none/trace for writing (2)" $t/err3
not grep -q symbol $t/log3

# The linker's own moves count too (__DATA_CONST), as do thread-local
# variables and the symbols a module defines but no file exports. A -r
# link reports nothing.
cat <<EOF | $CC -o $t/b.o -c -xc -
int gdata = 1;
int *const cptr = &gdata;
__thread int tlv = 3;
static int sfn(void) { return tlv; }
int main() { return *cptr + sfn(); }
EOF
$CC --ld-path=$mold -o $t/exe4 $t/b.o -Wl,-rename_section,__DATA,__data,__DATA,__d2 \
  -Wl,-rename_segment,__DATA,__D -Wl,-trace_symbol_layout > $t/log4 2> /dev/null
grep -qx "symbol '_main', mapped to __TEXT/__text" $t/log4
grep -qx "symbol '_gdata', mapped to __D/__d2" $t/log4
grep -qx "symbol '_cptr', mapped to __DATA_CONST/__const" $t/log4
grep -qx "symbol '_tlv', mapped to __D/__thread_vars" $t/log4
grep -qx "symbol '_sfn', mapped to __TEXT/__text" $t/log4
not grep -q ltmp $t/log4

$mold -r -arch $ARCH -o $t/r.o $t/b.o -trace_symbol_layout > $t/log6
[ ! -s $t/log6 ]
