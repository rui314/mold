#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Some compilers such as clang 18 don't attach R_RISCV_RELAX to TLSDESC
# relocations. mold relaxes such TLSDESC sequences to initial-exec or
# local-exec without deleting any instruction.

# GNU as may not support TLSDESC for RISC-V, so we use clang instead.
clang_args=()
[ "$TRIPLE" = "" ] || clang_args+=(--target=$TRIPLE)
echo 'auipc a0, %tlsdesc_hi(foo)' | clang "${clang_args[@]}" -c -o /dev/null -xassembler - ||
  skip

cat <<'EOF' | $CC -o $t/a.o -c -xc - -fPIC
_Thread_local char foo[4] = "foo";
_Thread_local char padding[100000] = "pad";
_Thread_local char bar[4] = "bar";
EOF

cat <<'EOF' | clang "${clang_args[@]}" -o $t/b.o -c -xassembler -
.globl get_foo, get_bar
get_foo:
.Ltlsdesc_hi0:
  auipc a0, %tlsdesc_hi(foo)
  ld t0, %tlsdesc_load_lo(.Ltlsdesc_hi0)(a0)
  addi a0, a0, %tlsdesc_add_lo(.Ltlsdesc_hi0)
  jalr t0, 0(t0), %tlsdesc_call(.Ltlsdesc_hi0)
  add a0, a0, tp
  ret

get_bar:
.Ltlsdesc_hi1:
  auipc a0, %tlsdesc_hi(bar)
  ld t0, %tlsdesc_load_lo(.Ltlsdesc_hi1)(a0)
  addi a0, a0, %tlsdesc_add_lo(.Ltlsdesc_hi1)
  jalr t0, 0(t0), %tlsdesc_call(.Ltlsdesc_hi1)
  add a0, a0, tp
  ret
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>
char *get_foo();
char *get_bar();
int main() { printf("%s %s\n", get_foo(), get_bar()); }
EOF

# TLS defined in a shared library: TLSDESC => IE
$CC -B. -shared -o $t/d.so $t/a.o
$CC -B. -o $t/exe1 $t/b.o $t/c.o $t/d.so -Wl,-rpath=$t
$QEMU $t/exe1 | grep -F 'foo bar'

# Local TLS in an executable: TLSDESC => LE
$CC -B. -o $t/exe2 $t/a.o $t/b.o $t/c.o
$QEMU $t/exe2 | grep -F 'foo bar'

# A static executable relaxes TLSDESC to LE even with --no-relax
$CC -B. -o $t/exe3 $t/a.o $t/b.o $t/c.o -static -Wl,--no-relax
$QEMU $t/exe3 | grep -F 'foo bar'
