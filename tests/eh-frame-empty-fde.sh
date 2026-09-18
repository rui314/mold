#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Compilers may emit an FDE covering a zero-length address range, e.g.
# for a function fragment that GCC's hot/cold splitting has entirely
# optimized away. Such an FDE begins at the same address as the FDE of
# the next real function fragment and can shadow it in .eh_frame_hdr's
# binary search table, so that the unwinder misses an existing exception
# handler and aborts the process.
#
# The zero-length FDEs below collapse onto the beginning of run(), which
# catches an exception and throws another one. Looking up run()'s frame
# during unwinding may then find a zero-length FDE instead of the real
# one, in which case an exception cannot propagate to main() and the
# process aborts. Which entry a search returns among the ones with the
# same address depends on how the sort orders them, so we create enough
# zero-length FDEs that a search practically never finds the real one.
# run() is alone in its file so that its fragments are the first ones
# after the zero-length fragments in the output.

cat <<EOF | $CC -o $t/a.o -c -x assembler -
.section .text.unlikely,"ax",%progbits
.p2align 4
empty_cold_00: .cfi_startproc; .cfi_endproc
empty_cold_01: .cfi_startproc; .cfi_endproc
empty_cold_02: .cfi_startproc; .cfi_endproc
empty_cold_03: .cfi_startproc; .cfi_endproc
empty_cold_04: .cfi_startproc; .cfi_endproc
empty_cold_05: .cfi_startproc; .cfi_endproc
empty_cold_06: .cfi_startproc; .cfi_endproc
empty_cold_07: .cfi_startproc; .cfi_endproc
empty_cold_08: .cfi_startproc; .cfi_endproc
empty_cold_09: .cfi_startproc; .cfi_endproc
empty_cold_10: .cfi_startproc; .cfi_endproc
empty_cold_11: .cfi_startproc; .cfi_endproc
empty_cold_12: .cfi_startproc; .cfi_endproc
empty_cold_13: .cfi_startproc; .cfi_endproc
empty_cold_14: .cfi_startproc; .cfi_endproc
empty_cold_15: .cfi_startproc; .cfi_endproc
empty_cold_16: .cfi_startproc; .cfi_endproc
empty_cold_17: .cfi_startproc; .cfi_endproc
empty_cold_18: .cfi_startproc; .cfi_endproc
empty_cold_19: .cfi_startproc; .cfi_endproc
empty_cold_20: .cfi_startproc; .cfi_endproc
empty_cold_21: .cfi_startproc; .cfi_endproc
empty_cold_22: .cfi_startproc; .cfi_endproc
empty_cold_23: .cfi_startproc; .cfi_endproc
empty_cold_24: .cfi_startproc; .cfi_endproc
empty_cold_25: .cfi_startproc; .cfi_endproc
empty_cold_26: .cfi_startproc; .cfi_endproc
empty_cold_27: .cfi_startproc; .cfi_endproc
empty_cold_28: .cfi_startproc; .cfi_endproc
empty_cold_29: .cfi_startproc; .cfi_endproc
empty_cold_30: .cfi_startproc; .cfi_endproc
empty_cold_31: .cfi_startproc; .cfi_endproc
EOF

cat <<EOF | $CXX -o $t/b.o -c -xc++ -O2 -
#include <stdexcept>

void maybe_throw(int x);
void rethrower();

__attribute__((noinline)) int run(int x) {
  try {
    maybe_throw(x);
    return 0;
  } catch (const std::exception &) {
    rethrower();
    return 1;
  }
}
EOF

cat <<EOF | $CXX -o $t/c.o -c -xc++ -O2 -
#include <stdexcept>

__attribute__((noinline)) void maybe_throw(int x) {
  if (x < 0)
    throw std::runtime_error("first");
}

__attribute__((noinline)) void rethrower() {
  throw std::runtime_error("second");
}
EOF

cat <<EOF | $CXX -o $t/d.o -c -xc++ -O2 -
#include <cstdio>
#include <stdexcept>

int run(int x);

int main() {
  try {
    run(-1);
  } catch (const std::exception &e) {
    std::printf("outer caught %s\n", e.what());
  }
}
EOF

$CXX -B. -o $t/exe $t/a.o $t/b.o $t/c.o $t/d.o
$QEMU $t/exe | grep 'outer caught'
