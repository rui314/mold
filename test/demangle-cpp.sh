#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' | $CC -c -o $t/a.o -xc -
void _ZN2ns7versionEv();
int main() { _ZN2ns7versionEv(); }
EOF

not $CC -B. -o $t/exe1 $t/a.o |& grep -F 'ns::version()'

cat <<'EOF' | $CC -c -o $t/b.o -xc -
void _ZN2ns7versionEv();
int main() { _ZN2ns7versionEv(); }
__attribute__((section(".comment"))) char str[] = "rustc version x.y.z\n";
EOF

not $CC -B. -o $t/exe2 $t/b.o |& grep -F 'ns::versionv'
