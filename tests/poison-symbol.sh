#!/bin/bash
source "$(dirname "$0")"/common.inc

# -poison_symbol and -poison_symbols_list (wildcards allowed) fail the
# link on any live reference to a symbol they name, listing each
# function that refers to it once. (ld-prime lists each reference.)
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

EOF
# (ld-prime lists the symbols in no stable order.)
grep -q '_bar, referenced from:' $t/msgs
grep -A2 '_bar, referenced from:' $t/msgs | diff - <(sed -n 2,4p $t/expected)
grep -A1 '_foo, referenced from:' $t/msgs | diff - <(sed -n 5,6p $t/expected)

echo '_b*' > $t/list
not $mold -o $t/exe $t/b.o $t/c.o $t/liba.dylib -poison_symbols_list $t/list 2> $t/log
grep -q '_bar, referenced from:' $t/log
not grep -q _foo $t/log

# A dead reference is fine, as is a symbol nothing refers to.
$mold -o $t/exe $t/b.o $t/c.o $t/liba.dylib -poison_symbol _baz -lSystem \
  -syslibroot "$(xcrun --show-sdk-path)" -dead_strip

not $mold -o $t/exe $t/b.o -poison_symbol 2> $t/log
grep -q -- '-poison_symbol.*missing' $t/log

# A subtracted symbol is no reference.
cat <<EOF2 | $CC -o $t/d.o -c -xassembler -
.data
.globl _d, _garply, _grault
.p2align 3
_d:
  .quad _garply - _d
  .quad _d - _grault
_garply: .quad 0
_grault: .quad 0
.subsections_via_symbols
EOF2
not $CC --ld-path=$mold -shared -o $t/d.dylib $t/d.o -Wl,-poison_symbol,_garply \
  -Wl,-poison_symbol,_grault 2> $t/log
grep -A1 '_garply, referenced from:' $t/log | grep -q '^ *_d in d.o$'
[ $(grep -c ' in d.o$' $t/log) = 1 ]
not grep -q '_grault, referenced' $t/log

if [ $ARCH = arm64 ]; then
  cat <<EOF2 | $CC -o $t/e.o -c -xassembler -
.text
.globl _f, _g, _h
_f:
  adrp x0, _qux@GOTPAGE
  nop
  ldr x0, [x0, _qux@GOTPAGEOFF]
  ret
_g:
  adrp x1, _quux@PAGE
  add x1, x1, _quux@PAGEOFF
  ldr x2, [x1, _quux@PAGEOFF]
  ret
_h:
  adrp x0, _corge@PAGE
  adrp x1, _corge@PAGE
  ldr x0, [x0, _corge@PAGEOFF]
  ret
.data
.globl _qux, _quux, _corge
.p2align 3
_qux: .quad 0
_quux: .quad 0
_corge: .quad 0
.subsections_via_symbols
EOF2
  not $CC --ld-path=$mold -shared -o $t/e.dylib $t/e.o -Wl,-poison_symbol,_qux \
    -Wl,-poison_symbol,_quux -Wl,-poison_symbol,_corge 2> $t/log
  [ $(grep -A3 '_qux, referenced from:' $t/log | grep -c ' _f in e.o$') = 1 ]
  [ $(grep -A3 '_quux, referenced from:' $t/log | grep -c ' _g in e.o$') = 1 ]
  [ $(grep -A4 '_corge, referenced from:' $t/log | grep -c ' _h in e.o$') = 1 ]
fi
