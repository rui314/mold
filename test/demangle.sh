#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc++ -
int foo(int, int);
int main() {
  foo(3, 4);
}
EOF

not $CC -B. -o $t/exe $t/a.o -Wl,-no-demangle |&
  grep 'undefined symbol: _Z3fooii$'

not $CC -B. -o $t/exe $t/a.o -Wl,-demangle |&
  grep -E 'undefined symbol: foo\(int, int\)$'

not $CC -B. -o $t/exe $t/a.o |&
  grep -E 'undefined symbol: foo\(int, int\)$'

cat <<EOF | $CC -c -o $t/b.o -xc -
extern int Pi;
int main() {
  return Pi;
}
EOF

not $CC -B. -o $t/exe $t/b.o -Wl,-demangle |&
  grep 'undefined symbol: Pi$'
