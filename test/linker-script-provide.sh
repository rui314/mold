#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -fPIC -
int foo = 3;
extern char provided;
extern char hidden_sym;
void *use1() { return &provided; }
void *use2() { return &hidden_sym; }
EOF

cat <<'EOF' > $t/script
PROVIDE(foo = 0x999);
PROVIDE(provided = 0x42);
PROVIDE_HIDDEN(hidden_sym = 0x43);
PROVIDE(unreferenced = no_such_symbol);
EOF

$CC -B. -o $t/b.so -shared $t/a.o -Wl,-T,$t/script
readelf -sW $t/b.so > $t/log

# A PROVIDE'd symbol must yield to a definition in an input file
grep -w foo $t/log | grep -v 999

# A referenced PROVIDE'd symbol is defined with the given value
grep -E '42 .* ABS provided' $t/log
grep -E '43 .* ABS hidden_sym' $t/log

# An unreferenced one is not defined, and its value expression is
# not even evaluated
not grep -w unreferenced $t/log

# A PROVIDE'd symbol is exported from a shared library while a
# PROVIDE_HIDDEN'd one is not
readelf --dyn-syms -W $t/b.so > $t/log2
grep -w provided $t/log2
not grep -w hidden_sym $t/log2
