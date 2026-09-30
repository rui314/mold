#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A NUL byte in a response file is an error. Here it is in the path of
# the LTO plugin, which the link needs because b.o has the LLVM IR magic.

echo 'int main() {}' | $CC -c -o $t/a.o -xc -
printf 'BC\xc0\xde' > $t/b.o
printf -- '-plugin foo\0bar' > $t/rsp

{ ./mold -o $t/exe $t/a.o $t/b.o @$t/rsp 2>&1; [ $? = 1 ]; } |
  grep -F "$t/rsp: response file contains a NUL byte"
