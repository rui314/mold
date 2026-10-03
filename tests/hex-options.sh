#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64's numeric option arguments are hexadecimal, with or without a 0x
# or 0X prefix. Anything else is refused, naming the option.
cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

sdk=$(xcrun --show-sdk-path)
link() { $mold -arch $ARCH -syslibroot "$sdk" -lSystem $t/a.o -o $t/exe "$@"; }

for opt in -image_base -seg1addr -pagezero_size -headerpad; do
  not link $opt 0x0x10 2> $t/log
  grep -q -- "$opt: not a hexadecimal number: 0x0x10" $t/log
done
not link -segaddr __DATA 12g 2> $t/log
grep -q -- '-segaddr: not a hexadecimal number: 12g' $t/log
not link -sectalign __TEXT __text '10 ' 2> $t/log
grep -q -- '-sectalign: not a hexadecimal number: 10 ' $t/log
not link -stack_size 0x 2> $t/log
grep -q -- '-stack_size must specify an integer size' $t/log

link -headerpad 0X1000
otool -l $t/exe | grep -A6 'sectname __text' | awk '/offset/{print $2}' > $t/off
[ "$(cat $t/off)" -gt 4096 ]
link -headerpad 1000
otool -l $t/exe | grep -A6 'sectname __text' | awk '/offset/{print $2}' > $t/off
[ "$(cat $t/off)" -gt 4096 ]

# A size is rounded up to a page.
link -pagezero_size 1001 2> $t/log
grep -q -- '-pagezero_size not aligned, rounded up to: 0x[0-9a-f]*000, use -segalign' $t/log

# The sizes and alignments must fit in 32 bits.
not link -headerpad 100000000 2> $t/log
grep -q -- '-headerpad size too large' $t/log
not link -sectalign __TEXT __text 100000000 2> $t/log
grep -q -- '-sectalign 4294967296: alignment too big' $t/log
not link -seg_page_size __TEXT 100000000 2> $t/log
grep -q -- '-seg_page_size 4294967296: size too big' $t/log
