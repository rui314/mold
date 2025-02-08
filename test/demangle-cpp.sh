#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' | $CC -c -o $t/a.o -xc -
void _ZN2ns7versionEv();
int main() { _ZN2ns7versionEv(); }
EOF

not $CC -B. -o $t/exe1 $t/a.o 2> $t/log
grep -F 'ns::version()' $t/log

cat <<'EOF' | $CC -c -o $t/b.o -xc -
void _ZN2ns7versionEv();
int main() { _ZN2ns7versionEv(); }
__attribute__((section(".comment"))) char str[] = "rustc version x.y.z\n";
EOF

not $CC -B. -o $t/exe2 $t/b.o 2> $t/log
grep -F 'ns::versionv' $t/log
