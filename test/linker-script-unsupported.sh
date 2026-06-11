#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

# Commands that mold cannot faithfully execute yet must be rejected
# rather than silently ignored.
cat <<'EOF' > $t/script
SECTIONS {
  OVERLAY 0x1000 : { .o1 { *(.o1) } }
}
EOF

not ./mold $t/a.o -T $t/script -o $t/exe |& grep 'not supported yet'

echo 'OUTPUT(foo.exe)' > $t/script2
not ./mold $t/a.o -T $t/script2 -o $t/exe |& grep 'not supported yet'

cat <<'EOF' > $t/script3
SECTIONS {
  .text : SUBALIGN(8) { *(.text*) }
}
EOF

not ./mold $t/a.o -T $t/script3 -o $t/exe |& grep 'not supported yet'

# A memory region must be declared before use
cat <<'EOF' > $t/script5
SECTIONS {
  .text : { *(.text*) } >ram
}
EOF

not ./mold $t/a.o -T $t/script5 -o $t/exe |& grep 'no such memory region'

echo 'SECTIONS { .text : { *(.text*) } }' > $t/script4
not ./mold $t/a.o -T $t/script4 -r -o $t/r.o |& grep 'not supported in relocatable'
