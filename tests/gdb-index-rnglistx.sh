#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Regression test for https://github.com/rui314/mold/issues/1578.
# When DWARF 5 .debug_rnglists' offset table contains entries for both
# the CU root and child DIEs, mold must read only the entry indexed by
# the CU's DW_FORM_rnglistx. Otherwise child DIE ranges leak into the
# .gdb_index address table, overlapping the CU's enclosing range.

[ $MACHINE = $(uname -m) ] || skip
echo 'int main() {}' | clang++ -gdwarf-5 -g -o /dev/null -xc++ - 2> /dev/null || skip

cat <<EOF > $t/a.cc
void may_throw(int x) {
  if (x < 0)
    throw x;
}

struct Guard {
  int v;
  ~Guard() { may_throw(v); }
};

int main() {
  {
    Guard g{1};
    may_throw(2);
  }
}
EOF

clang++ -B. -O0 -ggdb3 -ggnu-pubnames -gdwarf-5 \
  -Wl,--gdb-index $t/a.cc -o $t/exe

readelf --debug-dump=gdb_index $t/exe |
  sed -n '/^Address table:/,/^$/p' |
  grep -E '^[0-9a-f]+ [0-9a-f]+' |
  sort |
  awk 'NR > 1 && $1 < prev { exit 1 } { prev = $2 }'
