#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -xc - -o $t/a.o
void missing();
void unused() { missing(); }
int main() { return 0; }
EOF
$CC --ld-path=$mold $t/a.o -Wl,-dead_strip -o $t/exe
$t/exe
for flags in '' -dead_strip,-u,_unused -dead_strip,-u,_missing; do
  opts=
  if [ -n "$flags" ]; then opts=-Wl,$flags; fi
  ! $CC --ld-path=$mold $t/a.o $opts -o $t/bad 2> $t/log || false
  grep -q 'undefined symbol: .*_missing' $t/log
done
$CC --ld-path=$mold $t/a.o -Wl,-dead_strip,-undefined,dynamic_lookup -o $t/dynamic
nm -u $t/dynamic > $t/syms
! grep -q _missing $t/syms || false

# A dead function's CIE must not leave an undefined personality GOT slot.
cat <<EOF | $CXX -S -xc++ - -o $t/unwind.s
void unused_unwind() { try { throw 5; } catch (...) {} }
EOF
sed 's/___gxx_personality_v0/_missing_personality/g' $t/unwind.s > $t/missing.s
$CC -c $t/missing.s -o $t/unwind.o
echo 'int main() { return 0; }' | $CC -c -xc - -o $t/main.o
$CC --ld-path=$mold $t/main.o $t/unwind.o -Wl,-dead_strip -o $t/unwind
$t/unwind

# Initializers remain roots even when they cannot become local offsets.
cat <<EOF | $CC -c -xassembler - -o $t/init.o
.section __DATA,__mod_init_func,mod_init_funcs
.p2align 3
.quad _missing_init
EOF
! $CC --ld-path=$mold $t/main.o $t/init.o -Wl,-dead_strip,-fixup_chains -o $t/init 2> $t/log || false
grep -q 'undefined symbol: .*_missing_init' $t/log
