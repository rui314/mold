#!/bin/bash
source "$(dirname "$0")"/common.inc

# An archive member the link doesn't load is no part of it: a symbol it
# defines that dead stripping would keep (no-dead-strip, as
# __attribute__((used)) marks one) must not make its subsection a root,
# nor keep alive what that subsection references.
cat <<EOF | $CC -o $t/a.o -c -xc -
void helper(void) {}
int main() { return 0; }
EOF
cat <<EOF | $CC -o $t/b.o -c -xc -
void helper(void);
__attribute__((used)) static void keep(void) { helper(); }
int unused(void) { return 1; }
EOF
rm -f $t/lib.a
ar rcs $t/lib.a $t/b.o

$CC --ld-path=$mold -o $t/exe $t/a.o $t/lib.a -Wl,-dead_strip
nm $t/exe > $t/nm
not grep -q '_keep$' $t/nm
not grep -q '_helper$' $t/nm
$RUN $t/exe
