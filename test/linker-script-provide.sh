#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo = 3;
void bar() {}
EOF

cat <<'EOF' > $t/script
PROVIDE(foo = 0x999);
PROVIDE(provided = 0x42);
PROVIDE_HIDDEN(hidden_sym = 0x43);
EOF

$CC -B. -o $t/b.so -shared $t/a.o -Wl,-T,$t/script
readelf -sW $t/b.so > $t/log

# A PROVIDE'd symbol must yield to a definition in an input file
grep -w foo $t/log | grep -v 999

# Otherwise it is defined with the given value. A PROVIDE'd symbol is
# exported from a shared library while a PROVIDE_HIDDEN'd one is not.
grep -E '42 .* ABS provided' $t/log
grep -E '43 .* ABS hidden_sym' $t/log

readelf --dyn-syms -W $t/b.so > $t/log2
grep -w provided $t/log2
not grep -w hidden_sym $t/log2
