#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CXX -c -o $t/a.o -xc++ -mmacosx-version-min=10.1 -
int main() {
  try {
    throw 0;
  } catch (int x) {
    return x;
  }
  return 1;
}
EOF

$CXX --ld-path=$mold -o $t/exe $t/a.o
$RUN $t/exe

# -no_compact_unwind, which GCC's driver passes on every link, leaves
# out __unwind_info: the image unwinds by its __eh_frame alone, which
# keeps every FDE, those of functions with a compact unwind record too.
# The records are dropped, not turned into FDEs, so each function needs
# one of its own (-femit-dwarf-unwind=always). A -r link ignores it.
cat <<EOF | $CXX -c -o $t/b.o -xc++ - -fasynchronous-unwind-tables -femit-dwarf-unwind=always
__attribute__((noinline)) void thrower(int x) { if (x) throw x; }
int main(int argc, char **) {
  try { thrower(argc); } catch (int x) { return x - 1; }
  return 1;
}
EOF

$CXX --ld-path=$mold -o $t/exe2 $t/b.o -Wl,-no_compact_unwind
otool -l $t/exe2 > $t/exe2.lc
not grep -q __unwind_info $t/exe2.lc
# (clang gives an x86_64 simulator's object an FDE only for a function
# compact unwind can't describe, -femit-dwarf-unwind=always or not.)
if [ $ARCH = arm64 ] || ! on_simulator; then
  $RUN $t/exe2
  dwarfdump --eh-frame $t/b.o | grep -c ' FDE ' > $t/b.fdes
  dwarfdump --eh-frame $t/exe2 | grep -c ' FDE ' > $t/exe2.fdes
  diff $t/b.fdes $t/exe2.fdes
fi

$mold -arch $ARCH -r -no_compact_unwind -o $t/c.o $t/b.o
objdump --unwind-info $t/c.o | grep -q 'compact encoding'
