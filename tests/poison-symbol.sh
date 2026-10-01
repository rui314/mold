#!/bin/bash
source "$(dirname "$0")"/common.inc

# -poison_symbol and -poison_symbols_list (wildcards allowed) fail the
# link on any live reference to a symbol they name: ld-prime lists each
# reference as the function it is in and its file's leaf name.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo() { return 1; }
int bar() { return 2; }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -O0
int foo(void), bar(void);
int main() { return foo() + foo() + bar(); }
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
int bar(void);
int baz() { return bar(); }
EOF

$CC --ld-path=$mold -o $t/liba.dylib -shared $t/a.o

not $mold -o $t/exe $t/b.o $t/c.o $t/liba.dylib -poison_symbol _bar \
  -poison_symbol _foo 2> $t/log
grep -v '^+' $t/log | sed -E 's/^(ld|mold): (error: )?//' > $t/msgs
cat > $t/expected <<EOF
Use of poisoned symbols:
  _bar, referenced from:
      _main in b.o
      _baz in c.o
  _foo, referenced from:
      _main in b.o
      _main in b.o

EOF
# (ld-prime lists the symbols in no stable order.)
grep -q '_bar, referenced from:' $t/msgs
grep -A2 '_bar, referenced from:' $t/msgs | diff - <(sed -n 2,4p $t/expected)
grep -A2 '_foo, referenced from:' $t/msgs | diff - <(sed -n 5,7p $t/expected)

echo '_b*' > $t/list
not $mold -o $t/exe $t/b.o $t/c.o $t/liba.dylib -poison_symbols_list $t/list 2> $t/log
grep -q '_bar, referenced from:' $t/log
not grep -q _foo $t/log

# A dead reference is fine, as is a symbol nothing refers to.
$mold -o $t/exe $t/b.o $t/c.o $t/liba.dylib -poison_symbol _baz -lSystem \
  -syslibroot "$(xcrun --show-sdk-path)" -dead_strip

not $mold -o $t/exe $t/b.o -poison_symbol 2> $t/log
grep -q -- '-poison_symbol missing <name>' $t/log
