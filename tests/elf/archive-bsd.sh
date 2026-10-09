#!/usr/bin/env bash
. $(dirname $0)/common.inc

# BSD ar, which macOS uses, pads a short member name with spaces instead
# of terminating it with a slash. The archive starts with a symbol table
# member, __.SYMDEF_64, as a 64-bit BSD archive does.

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo() { return 3; }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
int foo();
int main() { printf("%d\n", foo()); }
EOF

# Appends a member named $2 with the contents of the file $3 to the
# archive $1.
add_member() {
  local size=$(wc -c < $3)
  printf '%-16s%-12s%-6s%-6s%-8s%-10s`\n' $2 0 0 0 644 $size >> $1
  cat $3 >> $1
  [ $((size % 2)) = 0 ] || printf '\n' >> $1
}

printf 'not a symbol table' > $t/symdef
printf '!<arch>\n' > $t/c.a
add_member $t/c.a __.SYMDEF_64 $t/symdef
add_member $t/c.a a.o $t/a.o

$CC -B. -Wl,--trace -o $t/exe $t/b.o $t/c.a > $t/log
grep -F "$t/c.a(a.o)" $t/log
$QEMU $t/exe | grep 3
