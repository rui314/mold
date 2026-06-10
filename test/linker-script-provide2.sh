#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -fPIC -
extern char chain_a;
void *use() { return &chain_a; }
int main() { return 0; }
EOF

# chain_a is referenced by the program; chain_b and chain_c only by
# other PROVIDE expressions. All three must be defined transitively,
# while unused (and its bogus expression) must not be evaluated.
cat <<'EOF' > $t/script
PROVIDE(chain_a = chain_b + 1);
PROVIDE(chain_b = chain_c + 1);
PROVIDE(chain_c = 0x1000);
PROVIDE(unused = no_such_symbol);
EOF

./mold -o $t/exe $t/a.o -T $t/script
readelf -sW $t/exe > $t/log
grep -E '1002 .* chain_a' $t/log
grep -E '1001 .* chain_b' $t/log
grep -E '1000 .* chain_c' $t/log
not grep -w unused $t/log
