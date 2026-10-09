#!/bin/bash
source "$(dirname "$0")"/common.inc

# The macOS versions are the point of the test.
on_simulator && skip

# Classic dyld info lists the pointers dyld slides with a small opcode
# program: a run of adjacent pointers is one DO_REBASE_*_TIMES, and the
# steps between them ADD_ADDR_*; the program ends with DONE. dyld
# rebases exactly the pointers, wherever they are.
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
int main() {
  for (int i = 0; i < 3; i++)
    if (s.a[i] != &x || s.q[i].p != &x)
      return 1;
  for (int i = 0; i < 21; i++)
    if (s.g[i] != &x)
      return 1;
  return s.q[3].p != &x || s.f != &x || s.i != &x;
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -mmacosx-version-min=11.0
$RUN $t/exe
otool -l $t/exe > $t/lc
grep -q LC_DYLD_INFO_ONLY $t/lc
s=0x$(nm $t/exe | awk '$3 == "_s" { print $1 }')
dyld_info -fixups $t/exe | awk '$4 == "rebase" { print $3 }' | sort > $t/rebases
{
  for off in 0 8 16 32 48 64 80 248 $(seq 416 8 576) 824; do
    printf '0x%X\n' $((s + off))
  done
} | sort > $t/expected
diff $t/expected $t/rebases
dyld_info -opcodes $t/exe | sed -n '/^ *rebase opcodes:/,/REBASE_OPCODE_DONE/p' > $t/ops
grep -q 'REBASE_OPCODE_DONE' $t/ops

# Runs of two pointers with a word between them.
cat <<EOF | $CC -o $t/b.o -c -xc - -mmacosx-version-min=11.0
int x;
struct { void *a[2]; long pad1; void *b[2]; long pad2; void *c[2]; } s =
  { { &x, &x }, 0, { &x, &x }, 0, { &x, &x } };
int main() { return s.a[0] != &x || s.b[1] != &x || s.c[1] != &x; }
EOF

$CC --ld-path=$mold -o $t/exe2 $t/b.o -mmacosx-version-min=11.0
$RUN $t/exe2
[ "$(dyld_info -fixups $t/exe2 | grep -c ' rebase ')" = 6 ]
