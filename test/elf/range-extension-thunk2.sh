#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -ffunction-sections
#include <stdio.h>

void f0();
void f1();
void f2();
void f3();
void f4();
void f5();
void f6();
void f7();
void f8();
void f9();
void f10();
void f11();
void f12();
void f13();
void f14();
void f15();
void f16();
void f17();
void f18();
void f19();

void f0(int x) { printf("0 "); if (!x) f9(); }
void space0() { __asm__(".space 1024*1024"); }

void f1(int x) { printf("1 "); f8(x); }
void space1() { __asm__(".space 1024*1024"); }

void f2(int x) { printf("2 "); f7(x); }
void space2() { __asm__(".space 1024*1024"); }

void f3(int x) { printf("3 "); f6(x); }
void space3() { __asm__(".space 1024*1024"); }

void f4(int x) { printf("4 "); f5(x); }
void space4() { __asm__(".space 1024*1024"); }

void f5(int x) { printf("5 "); f10(x); }
void space5() { __asm__(".space 1024*1024"); }

void f6(int x) { printf("6 "); f4(x); }
void space6() { __asm__(".space 1024*1024"); }

void f7(int x) { printf("7 "); f3(x); }
void space7() { __asm__(".space 1024*1024"); }

void f8(int x) { printf("8 "); f2(x); }
void space8() { __asm__(".space 1024*1024"); }

void f9(int x) { printf("9 "); f1(x); }
void space9() { __asm__(".space 1024*1024"); }

void f10(int x) { printf("10 "); f19(x); }
void space10() { __asm__(".space 8*1024*1024"); }

void f11(int x) { printf("11 "); f18(x); }
void space11() { __asm__(".space 8*1024*1024"); }

void f12(int x) { printf("12 "); f17(x); }
void space12() { __asm__(".space 8*1024*1024"); }

void f13(int x) { printf("13 "); f16(x); }
void space13() { __asm__(".space 8*1024*1024"); }

void f14(int x) { printf("14 "); f15(x); }
void space14() { __asm__(".space 8*1024*1024"); }

void f15(int x) { printf("15 "); f0(x + 1); }
void space15() { __asm__(".space 8*1024*1024"); }

void f16(int x) { printf("16 "); f14(x); }
void space16() { __asm__(".space 8*1024*1024"); }

void f17(int x) { printf("17 "); f13(x); }
void space17() { __asm__(".space 8*1024*1024"); }

void f18(int x) { printf("18 "); f12(x); }
void space18() { __asm__(".space 8*1024*1024"); }

void f19(int x) { printf("19 "); f11(x); }
void space19() { __asm__(".space 8*1024*1024"); }

int main() {
  f0(0);
  printf("\n");
}
EOF

$CC -B. -o $t/exe $t/a.o
$QEMU $t/exe | grep -Eq '^0 9 1 8 2 7 3 6 4 5 10 19 11 18 12 17 13 16 14 15 0 $'
