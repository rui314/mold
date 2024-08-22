#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' | $CC -o $t/a.o -c -xc - -fPIC
_Thread_local char foo[4] = "foo";
_Thread_local char padding[100000] = "padding";
EOF

cat <<'EOF' | $CC -o $t/b.o -c -xc - -fPIC
_Thread_local char bar[4] = "bar";
EOF

cat <<'EOF' | $CC -o $t/c.o -c -xc - -fPIC -mtls-dialect=desc -O2
extern _Thread_local char foo[4];
extern _Thread_local char bar[4];

char *get_foo() { return foo; }
char *get_bar() { return bar; }
EOF

cat <<EOF | $CC -o $t/d.o -c -xc - -mtls-dialect=desc
#include <stdio.h>
char *get_foo();
char *get_bar();

int main() {
  printf("%s %s\n", get_foo(), get_bar());
}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o $t/c.o $t/d.o -Wl,--no-relax
$QEMU $t/exe1 | grep -q 'foo bar'

$OBJDUMP -d $t/exe1 > $t/exe1.objdump
grep -A6 '<get_foo>:' $t/exe1.objdump | grep -Fq pcaddi
grep -A6 '<get_bar>:' $t/exe1.objdump | grep -Fq pcaddi

$CC -B. -o $t/exe2 $t/a.o $t/b.o $t/c.o $t/d.o -Wl,--relax
$QEMU $t/exe2 | grep -q 'foo bar'

$OBJDUMP -d $t/exe2 > $t/exe2.objdump
grep -A6 '<get_foo>:' $t/exe2.objdump | grep -Fq li.w
grep -A6 '<get_bar>:' $t/exe2.objdump | grep -Fq lu12i.w
