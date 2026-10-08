#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
extern int foo;
extern _Thread_local int bar;
extern _Thread_local int baz __attribute__((tls_model("initial-exec")));
int main() { return foo + bar + baz; }
EOF

not $CC -B. -o $t/exe1 $t/a.o |& grep 'undefined symbol: foo'

$CC -B. -o $t/exe2 -pie $t/a.o -Wl,-noinhibit-exec |& grep 'undefined symbol: foo'
$CC -B. -o $t/exe3 -no-pie $t/a.o -Wl,-noinhibit-exec |& grep 'undefined symbol: foo'
