#!/bin/bash
source "$(dirname "$0")"/common.inc

# Under -dead_strip ld-prime reports only the duplicate symbols whose
# kept definition is live, though it counts all, and
# -allow_dead_duplicates lets one stay whose other definitions are all
# dead.
cat <<EOF | $CC -o $t/a.o -c -xc -
int dup1() { return 1; }
int dup2() { return 2; }
int main() { return dup1(); }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int dup1() { return 3; }
int dup2() { return 4; }
EOF

not $CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-dead_strip 2> $t/log
grep "duplicate symbol '_dup1' in:" $t/log
not grep -q "duplicate symbol '_dup2'" $t/log
grep -q '2 duplicate symbols' $t/log

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-dead_strip -Wl,-allow_dead_duplicates
nm $t/exe | grep -q _dup1

# Without -dead_strip nothing is dead.
not $CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-allow_dead_duplicates 2> $t/log
grep -q '2 duplicate symbols' $t/log

# -duplicate_symbols is taken, and changes nothing.
not $CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-dead_strip \
  -Wl,-duplicate_symbols,warning 2> $t/log
grep -q '2 duplicate symbols' $t/log
not $mold -o $t/exe $t/a.o -duplicate_symbols suppress 2> $t/log
grep -q -- '-duplicate_symbols invalid option (warning | error)' $t/log
not $mold -o $t/exe $t/a.o -duplicate_symbols 2> $t/log
grep -q -- '-duplicate_symbols missing <option>' $t/log
