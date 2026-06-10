#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section(".foo"))) char foo[8] = {1};
__attribute__((section(".mydata"))) int d = 1;
__attribute__((section(".mydata2"))) int d2 = 2;
int main() { return 0; }
EOF

cat <<'EOF' > $t/script
PHDRS {
  text PT_LOAD FLAGS(5);
  data PT_LOAD FLAGS(6);
  note PT_NOTE FLAGS(4);
}
SECTIONS {
  . = 0x200000;
  .text : { *(.text .text.*) } :text
  .foo : { *(.foo) } :text :note
  . = 0x400000;
  .mydata : { *(.mydata) } :data
  .mydata2 : { *(.mydata2) }
}
EOF

./mold -o $t/exe $t/a.o -T $t/script
readelf -lW $t/exe > $t/log

# Exactly the segments the script asks for, with the given flags
grep -E 'LOAD .* R E' $t/log
grep -E 'LOAD .* RW ' $t/log
grep -E 'NOTE ' $t/log
test $(grep -c LOAD $t/log) = 2

# .foo is in both :text and :note; .mydata2 inherits :data from the
# previous section
grep -E '00 .*\.text .*\.foo' $t/log
grep -E '01 .*\.mydata \.mydata2' $t/log
grep -E '02 .*\.foo' $t/log
