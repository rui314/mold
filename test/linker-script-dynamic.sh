#!/bin/bash
. $(dirname $0)/common.inc

# A vDSO-shaped link: a shared library whose script places the
# linker-synthesized dynamic sections and defines the program headers
cat <<EOF | $CC -o $t/a.o -c -xc -fPIC -
int foo() { return 42; }
EOF

cat <<'EOF' > $t/script
PHDRS {
  text PT_LOAD FLAGS(5) FILEHDR PHDRS;
  dyn PT_DYNAMIC FLAGS(4);
}
SECTIONS {
  . = SIZEOF_HEADERS;
  .dynsym : { *(.dynsym) } :text
  .dynstr : { *(.dynstr) }
  .text : { *(.text .text.*) }
  .dynamic : { *(.dynamic) } :text :dyn
}
EOF

./mold -o $t/b.so -shared $t/a.o -T $t/script
readelf -lW $t/b.so > $t/log

# The script's segments, with FILEHDR extending :text back to offset 0
grep -E 'LOAD .*0x000000 .* R E' $t/log
grep -E 'DYNAMIC ' $t/log
grep -E '01 .*\.dynamic' $t/log

# Synthesized sections are placed where the script says: .dynsym
# before .text, .dynamic after
readelf -SW $t/b.so > $t/log2
dynsym=$(grep -F ' .dynsym' $t/log2 | sed 's/.*\[ *\([0-9]*\)\].*/\1/')
text=$(grep -wF ' .text' $t/log2 | sed 's/.*\[ *\([0-9]*\)\].*/\1/')
dynamic=$(grep -F ' .dynamic' $t/log2 | sed 's/.*\[ *\([0-9]*\)\].*/\1/')
test "$dynsym" -lt "$text"
test "$text" -lt "$dynamic"
