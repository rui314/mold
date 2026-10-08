#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
extern char foo_used __attribute__((weak));
extern __thread int foo __attribute__((weak, tls_model("local-exec")));
__thread int present;  // Ensure that the output has a PT_TLS segment.

void set_foo() {
  if (&foo_used)
    foo = 1;
}

int main() { set_foo(); }
EOF

$CC -B. -o $t/exe $t/a.o
$QEMU $t/exe
