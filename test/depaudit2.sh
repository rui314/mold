#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' | $CC -B. -shared -o $t/a.so -xc - -fPIC
#include <stdio.h>

unsigned la_version(unsigned v) {
  fprintf(stderr, "version=%d\n", v);
  return 0;
}

void foo() {}
EOF

cat <<'EOF' | $CC -B. -shared -o $t/b.so -xc - -fPIC -Wl,--audit=$t/a.so
void foo();
void bar() { foo(); }
EOF

cat <<'EOF' | $CC -c -o $t/c.o -xc -
void bar();
int main() { bar(); }
EOF

$CC -B. -o $t/exe1 $t/c.o $t/b.so
readelf --dynamic $t/exe1 | grep 'Dependency audit library:..*/a.so'

$CC -B. -o $t/exe2 $t/c.o $t/b.so -Wl,--depaudit=foo
readelf --dynamic $t/exe2 | grep 'Dependency audit library:..*foo:.*/a.so'
