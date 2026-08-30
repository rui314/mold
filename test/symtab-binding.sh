#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A symbol that is merely not exported to the dynamic symbol table must
# remain global in .symtab. https://github.com/rui314/mold/issues/906

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
__attribute__((visibility("hidden"))) void hidden_fn() {}
__attribute__((weak)) void weak_fn() {}
void global_fn() {}
int main() {}
EOF

$CC -B. -o $t/exe1 $t/a.o
readelf -sW $t/exe1 > $t/log1

grep -E 'FUNC    GLOBAL DEFAULT .* main$' $t/log1
grep -E 'FUNC    GLOBAL DEFAULT .* global_fn$' $t/log1
grep -E 'FUNC    WEAK   DEFAULT .* weak_fn$' $t/log1
grep -E 'FUNC    LOCAL  HIDDEN .* hidden_fn$' $t/log1

# A symbol localized by a version script becomes local.
echo '{ global: global_fn; local: *; };' > $t/b.ver
$CC -B. -shared -o $t/c.so $t/a.o -Wl,--version-script=$t/b.ver
readelf -sW $t/c.so > $t/log2

grep -E 'FUNC    GLOBAL DEFAULT .* global_fn$' $t/log2
grep -E 'FUNC    LOCAL  DEFAULT .* weak_fn$' $t/log2
