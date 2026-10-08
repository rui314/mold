#!/usr/bin/env bash
. $(dirname $0)/common.inc

test_cflags -mbranch-protection=bti || skip

cat <<EOF | $CC -fPIC -mbranch-protection=bti -c -o $t/a.o -xc -
int foo() { return 1; }
int bar() { return 2; }
int baz() { return 3; }
int call(int (*fn)()) { return fn(); }
EOF

$CC -B. -shared -nostdlib -o $t/b.so $t/a.o

# All PLT entries jump to the PLT header indirectly. Other than that, a
# PLT entry needs a landing pad only if it's reached by a range extension
# thunk (foo and call), if it's a canonical PLT (baz) or if it belongs to
# an ifunc (qux).
cat <<EOF | $CC -fno-PIC -mbranch-protection=bti -c -o $t/c.o -xc -
int foo();
int bar();
int baz();
int call(int (*fn)());

static int impl() { return 4; }
static void *resolve() { return impl; }
int qux() __attribute__((ifunc("resolve")));

__attribute__((section(".low"))) int low() { return bar(); }

void _start() {
  register long x0 __asm__("x0") = foo() + low() + call(baz) + call(qux) == 10 ? 0 : 1;
  register long x8 __asm__("x8") = 93; // exit
  __asm__ volatile("svc 0" :: "r"(x0), "r"(x8));
}
EOF

$CC -B. -no-pie -nostdlib -o $t/exe $t/c.o $t/b.so \
  -Wl,--section-start=.low=0x10000000,--section-start=.text=0x20000000

readelf -n $t/exe | grep 'AArch64 feature: BTI'
readelf --dynamic $t/exe | grep AARCH64_BTI_PLT

$OBJDUMP -d -j .plt -j .plt.got $t/exe > $t/log
grep -A1 '<_PROCEDURE_LINKAGE_TABLE_>:' $t/log | grep -w bti
grep -A1 -E '<foo[@$]plt>:' $t/log | grep -w bti
grep -A1 -E '<bar[@$]plt>:' $t/log | not grep -w bti
grep -A1 -E '<baz[@$]plt>:' $t/log | grep -w bti
grep -A1 -E '<call[@$]plt>:' $t/log | grep -w bti
grep -A1 '<qux$pltgot>:' $t/log | grep -w bti

$QEMU $t/exe
