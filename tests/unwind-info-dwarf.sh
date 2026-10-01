#!/bin/bash
source "$(dirname "$0")"/common.inc

# A function whose frame compact unwind can't describe gets a
# DWARF-mode __unwind_info entry pointing at its FDE, whose CIE names
# the personality routine and which holds the LSDA. ld-prime still
# gives the entry the personality's index and the LSDA flag and lists
# the LSDA in the LSDA index, as for a compactly encoded function.
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
$t/exe

# Prints the personality array, then each entry's address and
# encoding, then each LSDA index row's function and LSDA address.
unwind_info() {
  python3 - $1 <<'EOF2'
import struct, subprocess, sys
out = subprocess.run(['otool', '-l', sys.argv[1]], capture_output=True, text=True).stdout.splitlines()
for i, l in enumerate(out):
    if l.strip() == 'sectname __unwind_info':
        size = int(out[i + 3].split()[1], 16); off = int(out[i + 4].split()[1])
d = open(sys.argv[1], 'rb').read()[off:off + size]
_, ceo, cec, po, pc, iso, isc = struct.unpack_from('<7I', d, 0)
base = 0x100000000
print('personalities', *[hex(base + p) for p in struct.unpack_from(f'<{pc}I', d, po)])
common = struct.unpack_from(f'<{cec}I', d, ceo)
idx = [struct.unpack_from('<3I', d, iso + 12 * k) for k in range(isc)]
for k in range(isc - 1):
    first, page, _ = idx[k]
    if struct.unpack_from('<I', d, page)[0] == 3:
        _, eo, ec, eco, ecc = struct.unpack_from('<IHHHH', d, page)
        local = struct.unpack_from(f'<{ecc}I', d, page + eco)
        for e in struct.unpack_from(f'<{ec}I', d, page + eo):
            j = e >> 24
            enc = common[j] if j < cec else local[j - cec]
            print('entry', hex(base + first + (e & 0xffffff)), hex(enc))
    else:
        _, eo, ec = struct.unpack_from('<IHH', d, page)
        for j in range(ec):
            fo, enc = struct.unpack_from('<II', d, page + eo + 8 * j)
            print('entry', hex(base + fo), hex(enc))
for o in range(idx[0][2], idx[-1][2], 8):
    fn, lsda = struct.unpack_from('<2I', d, o)
    print('lsda', hex(base + fn), hex(base + lsda))
EOF2
}
addr() { nm $1 | awk -v s=$2 '$3 == s { print "0x" $1 }' | sed 's/0x0*/0x/'; }
got_slot() { dyld_info -fixups $1 | awk -v s=$2 '$NF ~ "/" s "$" { print tolower($3) }'; }
if [ $ARCH = arm64 ]; then dwarf=3; else dwarf=4; fi

unwind_info $t/exe > $t/info
grep -qx "personalities $(got_slot $t/exe ___gxx_personality_v0)" $t/info
grep -qx "entry $(addr $t/exe __Z7catcheri) 0x5${dwarf}[0-9a-f]\{6\}" $t/info
grep -q "^lsda $(addr $t/exe __Z7catcheri) " $t/info

# Personalities take their indices in address order, the order of
# first use in the table. A C function with cleanups calls through
# ___gcc_personality_v0.
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
$t/exe2
unwind_info $t/exe2 > $t/info2
grep -qx "personalities $(got_slot $t/exe2 ___gxx_personality_v0) $(got_slot $t/exe2 ___gcc_personality_v0)" $t/info2

# Of two FDEs of one function, ld-prime carries both, but the
# function's one entry points at the last, 0x34 into __eh_frame.
{
  printf '.text\n.globl _main\n.p2align 2\n_main:\n  ret\n.section __TEXT,__eh_frame\n'
  printf '%s\n' EH_frame0: '.long 20' '.long 0' '.byte 1, 0x7a, 0x52, 0, 1, 0x78, 30, 1, 0x10, 0x0c, 31, 8, 0, 0, 0, 0'
  for id in 28 56; do
    printf '%s\n' '.long 24' ".long $id" '.quad _main - .' '.quad 1' '.long 0'
  done
  echo .subsections_via_symbols
} | $CC -c -o $t/e.o -xassembler -
$CC --ld-path=$mold -o $t/exe3 $t/e.o
unwind_info $t/exe3 > $t/info3
grep "^entry $(addr $t/exe3 _main) " $t/info3 > $t/entry3
[ "$(cat $t/entry3)" = "entry $(addr $t/exe3 _main) 0x${dwarf}000034" ]
