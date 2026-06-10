#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

cat <<'EOF' > $t/script
text_end = ADDR(.text) + SIZEOF(.text);
big = MAX(0x100, 0x200);
aligned = ALIGN(0x1234, 0x1000);
cond = DEFINED(no_such_symbol) ? no_such_symbol : 7;
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-T,$t/script
readelf -sW $t/exe > $t/log

# text_end must be relative to the .text output section, not absolute
text=$(readelf -SW $t/exe | grep -F ' .text' | head -1 | sed 's/.*\[ *\([0-9]*\)\].*/\1/')
grep -E "text_end" $t/log | grep -vw ABS | grep -w $text
grep -E '200 .* ABS big' $t/log
grep -E '2000 .* ABS aligned' $t/log
grep -E '7 .* ABS cond' $t/log
