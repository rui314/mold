#!/bin/bash
source "$(dirname "$0")"/common.inc

# Classic dyld info lists the pointers dyld slides with a small opcode
# program. ld64 packs it in phases: a run of adjacent pointers is one
# DO_REBASE_*_TIMES, a lone pointer plus the step to the next one is one
# DO_REBASE_ADD_ADDR_ULEB, three or more of those with one step are one
# DO_REBASE_ULEB_TIMES_SKIPPING_ULEB, and a small pointer-aligned step
# is an ADD_ADDR_IMM_SCALED. ld-prime writes the same bytes.
cat <<EOF | $CC -o $t/a.o -c -xc - -mmacosx-version-min=11.0
int x;
struct {
  void *a[3];
  long pad1;
  struct { void *p; long n; } q[4];
  long pad2[19];
  void *f;
  long pad3[20];
  void *g[21];
  long pad4[30];
  void *i;
} s = { { &x, &x, &x }, 0, { { &x }, { &x }, { &x }, { &x } }, { 0 }, &x,
        { 0 }, { &x, &x, &x, &x, &x, &x, &x, &x, &x, &x, &x, &x, &x, &x,
                 &x, &x, &x, &x, &x, &x, &x }, { 0 }, &x };
int main() { return s.i != &x; }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -mmacosx-version-min=11.0
$t/exe
dyld_info -opcodes $t/exe | sed -n '/^ *rebase opcodes:/,/REBASE_OPCODE_DONE/p' |
  grep -o 'REBASE_OPCODE_[A-Z_]*([^)]*)' | grep -v SET_SEGMENT > $t/ops
cat > $t/expected <<EOF
REBASE_OPCODE_SET_TYPE_IMM(1)
REBASE_OPCODE_DO_REBASE_IMM_TIMES(3)
REBASE_OPCODE_ADD_ADDR_IMM_SCALED(0x00000008)
REBASE_OPCODE_DO_REBASE_ULEB_TIMES_SKIPPING_ULEB(3, 8)
REBASE_OPCODE_DO_REBASE_ADD_ADDR_ULEB(168)
REBASE_OPCODE_DO_REBASE_ADD_ADDR_ULEB(168)
REBASE_OPCODE_DO_REBASE_ULEB_TIMES(21)
REBASE_OPCODE_ADD_ADDR_ULEB(0x000000F0)
REBASE_OPCODE_DO_REBASE_IMM_TIMES(1)
REBASE_OPCODE_DONE()
EOF
diff $t/expected $t/ops

# ld64 writes no DONE opcode but pads the stream with zeros, which read
# as one: a stream that fills 8 bytes has none.
cat <<EOF | $CC -o $t/b.o -c -xc - -mmacosx-version-min=11.0
int x;
struct { void *a[2]; long pad1; void *b[2]; long pad2; void *c[2]; } s =
  { { &x, &x }, 0, { &x, &x }, 0, { &x, &x } };
int main() { return s.c[1] != &x; }
EOF

$CC --ld-path=$mold -o $t/exe2 $t/b.o -mmacosx-version-min=11.0
$t/exe2
otool -l $t/exe2 | grep -q 'rebase_size 8$'
