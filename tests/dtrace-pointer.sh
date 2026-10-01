#!/bin/bash
source "$(dirname "$0")"/common.inc

# A pointer in data to a DTrace symbol, which nothing defines, is left
# to dyld as a bind by flat lookup, addend and all, chained or classic;
# the symbol gets no symbol table entry.
cat <<EOF | $CC -o $t/a.o -c -xc -
extern void __dtrace_probe\$foo\$bar\$v1(void);
extern int __dtrace_isenabled\$foo\$bar\$v1(void);
void *ptrs[] = {
  (char *)&__dtrace_probe\$foo\$bar\$v1 + 8,
  &__dtrace_isenabled\$foo\$bar\$v1,
};
int main() { return 0; }
EOF

for opt in -Wl,-fixup_chains -Wl,-no_fixup_chains; do
  $CC --ld-path=$mold -o $t/exe $t/a.o $opt
  dyld_info -fixups $t/exe > $t/fixups
  grep -q 'bind  <flat-namespace>/___dtrace_probe$foo$bar$v1 + 0x8$' $t/fixups
  grep -q 'bind  <flat-namespace>/___dtrace_isenabled$foo$bar$v1$' $t/fixups
  not grep -q rebase $t/fixups
  nm -m $t/exe > $t/syms
  not grep -q dtrace $t/syms
done
