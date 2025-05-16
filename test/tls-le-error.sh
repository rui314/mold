#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
__attribute__((tls_model("local-exec"))) static _Thread_local int foo = 5;
int bar() { return foo; }
EOF

not $CC -B. -shared -o $t/b.so $t/a.o |&
  grep 'relocation .* against `foo` can not be used when making a shared object; recompile with -fPIC'
