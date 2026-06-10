#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

# Commands that mold cannot faithfully execute yet must be rejected
# rather than silently ignored.
cat <<'EOF' > $t/script
SECTIONS {
  .text : { *(.text .text.*) }
}
EOF

not ./mold $t/a.o -T $t/script -o $t/exe |& grep 'not supported yet'

echo 'OUTPUT(foo.exe)' > $t/script2
not ./mold $t/a.o -T $t/script2 -o $t/exe |& grep 'not supported yet'

echo '. = 0x1000;' > $t/script3
not ./mold $t/a.o -T $t/script3 -o $t/exe |& grep 'not supported yet'
