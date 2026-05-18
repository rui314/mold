#!/bin/bash
. $(dirname $0)/common.inc

# Bug only triggers on DW_FORM_rnglistx, which clang emits but gcc through 13 doesn't.
[ $MACHINE = $(uname -m) ] || skip
clang++ --version >& /dev/null || skip
echo 'int main() {}' | clang++ -gdwarf-5 -g -fexceptions -c -o /dev/null -xc++ - 2>/dev/null || skip

# Throwing destructor + nested scopes => non-contiguous lexical_blocks => rnglistx.
cat <<'EOF' > $t/a.cc
void may_throw(int x) { if (x < 0) throw x; }
struct Guard {
  int val;
  explicit Guard(int v) : val(v) {}
  ~Guard() { may_throw(val); }
};
int work(int x) {
  int r = 0;
  { int a = x + 1;
    { Guard g(a);
      { int b = x * 2; may_throw(b); r = b; }
      r += a; }
    r += x; }
  return r;
}
int main() { return work(3); }
EOF

clang++ -c -o $t/a.o $t/a.cc -gdwarf-5 -ggdb3 -fexceptions -O0 -fno-inline
readelf --debug-dump=info $t/a.o | grep -E 'DW_AT_ranges.*\(index:' > /dev/null || skip

clang++ -B. -o $t/exe $t/a.o -gdwarf-5 -ggdb3 -ggnu-pubnames -fexceptions \
  -O0 -fno-inline -Wl,--gdb-index

readelf -WS $t/exe | grep -F .gdb_index

readelf --debug-dump=gdb_index $t/exe |
  awk '/^Address table:/ { in_tab=1; next }
       /^Symbol table:/ { in_tab=0 }
       in_tab && NF==3 && $1 ~ /^[0-9a-f]+$/ { print $1, $2 }' |
  sort > $t/addrtab

# 16-char zero-padded hex; lexicographic compare == numeric compare.
awk '{
    if (NR > 1 && $1 < prev_hi) {
      printf "overlap: previous range ends 0x%s, current starts 0x%s\n", prev_hi, $1
      exit 1
    }
    prev_hi = $2
  }' $t/addrtab

# A wrong-index regression would be overlap-free but miss main's address.
main_addr=$(readelf -s $t/exe | awk '$NF == "main" { print $2; exit }')
awk -v a="$main_addr" '
  BEGIN { found = 0 }
  $1 <= a && a < $2 { found = 1; print "found main 0x" a " in [0x" $1 ", 0x" $2 ")"; exit }
  END { if (!found) { print "main 0x" a " not covered"; exit 1 } }
' $t/addrtab
