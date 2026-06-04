#!/bin/bash
. $(dirname $0)/common.inc

# This test is only applicable to x86 and x86_64 because they use the lowest
# bit of member function pointers for virtuality.
case $MACHINE in
  x86_64 | i686) ;;
  *) skip ;;
esac

# We use assembly to define functions to have full control over alignment.
# We swap the order so that regular_func (alignment 1) comes first and
# is more likely to be chosen as the leader by ICF.
# We make pre_func size 2 to try to make regular_func start at an odd address.
cat <<EOF | $CC -c -o $t/fns.o -x assembler -
  .text

  .section .text.pre_func, "ax", @progbits
  .globl pre_func
  .p2align 0
pre_func:
  .byte 0x90, 0x90 # two nops (size 2)

  .section .text.regular_func, "ax", @progbits
  .globl regular_func
  .p2align 0 # 1-byte alignment
regular_func:
  ret

  .section .text.member_func, "ax", @progbits
  .globl _ZN3Foo11member_funcEv
  .p2align 2 # 4-byte alignment
_ZN3Foo11member_funcEv:
  ret
EOF

cat <<EOF | $CXX -c -o $t/main.o -xc++ -
#include <iostream>
#include <stdint.h>

struct Foo {
  void member_func();
};

extern "C" void regular_func();

int main() {
  void (Foo::*ptr)() = &Foo::member_func;
  union {
    void (Foo::*mem_ptr)();
    struct {
      void* ptr;
      ptrdiff_t adj;
    } parts;
  } u;
  u.mem_ptr = ptr;

  std::cout << "member_func address:  " << u.parts.ptr << std::endl;
  std::cout << "regular_func address: " << (void*)&regular_func << std::endl;

  if (u.parts.ptr != (void*)&regular_func) {
    std::cerr << "FAIL: functions were not folded!" << std::endl;
    return 2;
  }

  if (reinterpret_cast<uintptr_t>(u.parts.ptr) & 1) {
    std::cerr << "FAIL: member function pointer has odd address (treated as virtual)" << std::endl;
    return 1;
  }
  return 0;
}
EOF

$CXX -B. -o $t/exe $t/main.o $t/fns.o -Wl,--icf=all
$QEMU $t/exe
