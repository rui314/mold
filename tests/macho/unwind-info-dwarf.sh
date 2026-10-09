#!/bin/bash
source "$(dirname "$0")"/common.inc

# A function whose frame compact unwind can't describe gets a
# DWARF-mode __unwind_info entry pointing at its FDE, whose CIE names
# the personality routine and which holds the LSDA: exceptions unwind
# through it and are caught in it.
cat <<EOF > $t/a.cc
#include <stdexcept>
struct Guard { ~Guard(); };
Guard::~Guard() {}
__attribute__((noinline)) void thrower(int x) {
  if (x) throw std::runtime_error("x");
}
__attribute__((noinline)) int catcher(int x) {
  try { Guard g; thrower(x); } catch (const std::exception &) { return 1; }
  return 0;
}
int main(int argc, char **) { return catcher(argc) - 1; }
EOF
# A DW_CFA_nop, which compact unwind can't express, forces DWARF mode.
$CXX -O1 -S -o $t/a.s $t/a.cc
perl -pi -e 's/^(\s*\.cfi_startproc.*)$/$1\n\t.cfi_escape 0x0/' $t/a.s
$CXX -c -o $t/a.o $t/a.s
$CXX --ld-path=$mold -o $t/exe $t/a.o
$RUN $t/exe

# Prints the personality routine's GOT slot the unwinder finds for a
# function, from the personality index of its encoding.
personality() {
  enc=$(unwind_lookup $1 $2)
  python3 - $1 $enc <<'EOF2'
import struct, sys, macho
m = macho.MachO(sys.argv[1])
d = m.contents(m.section('__unwind_info'))
_, _, _, po, pc = struct.unpack_from('<5I', d, 0)
idx = (int(sys.argv[2], 16) >> 28) & 3
print(hex(0x100000000 + struct.unpack_from('<I', d, po + 4 * (idx - 1))[0]) if idx else 'none')
EOF2
}
got_slot() { dyld_info -fixups $1 | awk -v s=$2 '$NF ~ "/" s "$" { print tolower($3) }'; }
if [ $ARCH = arm64 ]; then dwarf=3; else dwarf=4; fi

mode() { echo $(( ($(unwind_lookup $1 $2) >> 24) & 0xf )); }
[ $(mode $t/exe __Z7catcheri) = $dwarf ]

# Each function finds its own personality routine: a C function with
# cleanups calls through ___gcc_personality_v0 (which its FDE names if
# the function is in DWARF mode: the unwinder then reads its CIE).
cat <<EOF | $CC -fexceptions -c -o $t/b.o -xc -
void ext(void);
static void cleanup(int *p) { ext(); }
int cfun(void) { int x __attribute__((cleanup(cleanup))) = 0; ext(); return x; }
EOF
cat <<EOF | $CXX -c -o $t/c.o -xc++ -
extern "C" void ext();
struct G { ~G(); };
G::~G() { ext(); }
extern "C" int cxxfun() { G g; ext(); return 0; }
extern "C" int cfun(void);
int main() { return cfun() + cxxfun(); }
EOF
echo 'void ext(void) {}' | $CC -c -o $t/d.o -xc -
printf '_cxxfun\n' > $t/order
$CXX --ld-path=$mold -o $t/exe2 $t/b.o $t/c.o $t/d.o -Wl,-order_file,$t/order
$RUN $t/exe2
[ "$(personality $t/exe2 _cxxfun)" = "$(got_slot $t/exe2 ___gxx_personality_v0)" ]
[ $(mode $t/exe2 _cfun) = $dwarf ] ||
  [ "$(personality $t/exe2 _cfun)" = "$(got_slot $t/exe2 ___gcc_personality_v0)" ]

# Of two FDEs of one function, both are carried, and the function's
# one entry points at one of them, 0x18 or 0x34 into __eh_frame.
{
  printf '.text\n.globl _main\n.p2align 2\n_main:\n  ret\n.section __TEXT,__eh_frame\n'
  printf '%s\n' EH_frame0: '.long 20' '.long 0' '.byte 1, 0x7a, 0x52, 0, 1, 0x78, 30, 1, 0x10, 0x0c, 31, 8, 0, 0, 0, 0'
  for id in 28 56; do
    printf '%s\n' '.long 24' ".long $id" '.quad _main - .' '.quad 1' '.long 0'
  done
  echo .subsections_via_symbols
} | $CC -c -o $t/e.o -xassembler -
$CC --ld-path=$mold -o $t/exe3 $t/e.o
[ $(mode $t/exe3 _main) = $dwarf ]
fde=$(( $(unwind_lookup $t/exe3 _main) & 0xffffff ))
[ $fde = 24 ] || [ $fde = 52 ]
