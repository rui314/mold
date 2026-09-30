#!/usr/bin/env bash
. $(dirname $0)/common.inc

# An I/O error is reported with the text strerror gives for it.

echo 'int main() {}' | $CC -c -o $t/a.o -xc -

not ./mold -o $t/exe $t/a.o $t/nonexistent.o |&
  grep -Fx "mold: fatal: cannot open $t/nonexistent.o: No such file or directory"

not ./mold -o $t/exe $t/a.o -Map $t/nonexistent/map |&
  grep -Fx "mold: fatal: --print-map: cannot open $t/nonexistent/map: No such file or directory"

not ./mold -o $t/nonexistent/exe $t/a.o |&
  grep -E "^mold: fatal: cannot open $t/nonexistent/\.exe\.[0-9]+: No such file or directory$"
