#!/bin/bash
source "$(dirname "$0")"/common.inc

# -order_file_statistics (or LD_PRINT_ORDER_FILE_STATISTICS in the
# environment) reports the -order_file lines that name no live symbol
# of the objects - nor of their object, if they name one -, and how
# many did. (ld-prime also counts a line naming only a C string's label
# or an earlier line's symbol as naming nothing, and warns about names
# given twice or found in several objects.)
cat <<EOF | $CC -o $t/a.o -c -xc -
static int st(void) { return 1; }
int fa(void) { return st(); }
int fb(void);
int main() { return fa() + fb(); }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
static int st(void) { return 2; }
int fb(void) { return st(); }
void unused(void) {}
EOF

cat <<EOF > $t/order
_main
_nosuch
_fa
_printf
b.o:_fb
a.o:_fb
_st
_unused
EOF

link() {
  $CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-order_file,$t/order -Wl,-dead_strip "$@"
}

link 2> $t/log1
not grep -q warning: $t/log1

link -Wl,-order_file_statistics 2> $t/log2
grep "can't find function/data for order_file entry" $t/log2 | sed 's/.*: //' > $t/missing2
diff - $t/missing2 <<EOF
_nosuch
_printf
_fb
_unused
EOF
grep -q 'only 4 out of 8 order_file symbols were applicable' $t/log2

LD_PRINT_ORDER_FILE_STATISTICS=1 link 2> $t/log3
grep -q 'only 4 out of 8 order_file symbols were applicable' $t/log3

# Every line naming a symbol, nothing is reported.
printf '_fa\n_main\n' > $t/order
link -Wl,-order_file_statistics 2> $t/log4
not grep -q warning: $t/log4
