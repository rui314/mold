#!/bin/bash
source "$(dirname "$0")"/common.inc

# -order_file_statistics (or LD_PRINT_ORDER_FILE_STATISTICS in the
# environment) reports the -order_file lines that order no atom: a
# symbol no atom is named after - not a dylib's, nor one dead stripped,
# nor a C string's label -, or one an earlier line named. A symbol named
# again without an object is ambiguous, and one without an object that
# several objects define needs one.
cat <<EOF | $CC -o $t/a.o -c -xc -
static int st(void) { return 1; }
int fa(void) { return st(); }
int fb(void);
int main() { return fa() + fb(); }
const char *str(void) { return "hello"; }
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
_main
_printf
b.o:_fb
a.o:_fb
_st
l_.str
_unused
EOF

link() {
  $CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-order_file,$t/order -Wl,-dead_strip "$@"
}

link 2> $t/log1
not grep -q warning: $t/log1

link -Wl,-order_file_statistics 2> $t/log2
grep -q "position of '_main' ambiguous, entry specified multiple times in the order file" $t/log2
grep -q '_st specified in order_file but it exists in multiple .o files. Prefix symbol with .o filename in order_file to disambiguate' $t/log2
[ "$(grep -c "can't find function/data for order_file entry" $t/log2)" = 6 ]
for sym in _nosuch _main _printf _fb l_.str _unused; do
  grep -q "can't find function/data for order_file entry: $sym$" $t/log2
done
grep -q 'only 4 out of 10 order_file symbols were applicable' $t/log2

LD_PRINT_ORDER_FILE_STATISTICS=1 link 2> $t/log3
grep -q 'only 4 out of 10 order_file symbols were applicable' $t/log3

# Every line ordering an atom, nothing is reported.
printf '_fa\n_main\n' > $t/order
link -Wl,-order_file_statistics 2> $t/log4
not grep -q warning: $t/log4
