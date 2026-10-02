#!/bin/bash
source "$(dirname "$0")"/common.inc

# A final image lists every name at a subsection's start - its labels,
# alternate entry points, the private externals it demotes, and the
# names of the functions -deduplicate folded into it - all at its
# address. (ld-prime lists the names in an order of its own, the
# subsection's best name last.) -map lists every name there, one with
# the subsection's size.
if [ $ARCH = arm64 ]; then
  body() { echo 'mov w0, #7'; echo ret; }
  jump() { echo "b $1"; }
else
  body() { echo 'movl $7, %eax'; echo ret; echo nop; echo nop; }
  jump() { echo "jmp $1"; }
fi

gen() { # file label...: "name:a" is an alternate entry, "name:p" a private extern
  local f=$1; shift
  {
    echo '.subsections_via_symbols'
    echo '.text'
    echo ".globl _call_$f"
    echo '.p2align 2'
    echo "_call_$f:"
    jump ${1%%:*}
    echo '.p2align 2'
    for spec in "$@"; do
      n=${spec%%:*}; s=${spec#*:}; [ "$s" = "$spec" ] && s=
      case $s in *p*) echo ".globl $n"; echo ".private_extern $n";; esac
      case $s in *a*) echo ".alt_entry $n";; esac
      echo "$n:"
    done
    body
  } > $t/$f.s
  $CC -o $t/$f.o -c $t/$f.s
}

gen a _k1 _j1:a
gen b _x7:p _b7 _c7:a
gen c _w6 _x6
gen d _a1 _z1 _y1 _b1:a _x1:p

cat <<EOF | $CC -o $t/main.o -c -xc -
int call_a(void), call_b(void), call_c(void), call_d(void);
int main() { return call_a() + call_b() + call_c() + call_d() != 28; }
EOF

# The names, sorted, and that they are at one address.
names() {
  nm -p $1 | grep -E ' _[a-z][0-9]$' > $1.names &&
    [ "$(awk '{ print $1 }' $1.names | sort -u | wc -l)" -eq 1 ] &&
    sed 's/.* _//' $1.names | sort | tr '\n' ' '
}
# The map's rows of the names: $2 of them, at one address, one sized.
maprows() {
  grep -E '\] _[a-z][0-9]$' $1 > $1.rows &&
    [ "$(wc -l < $1.rows)" -eq $2 ] &&
    [ "$(cut -f1 $1.rows | sort -u | wc -l)" -eq 1 ] &&
    [ "$(cut -f2 $1.rows | grep -vc 0x00000000)" -eq 1 ]
}

# Without folding.
echo 'int call_d(void); int main() { return call_d() != 7; }' | $CC -o $t/main1.o -c -xc -
$CC --ld-path=$mold -o $t/exe1 $t/main1.o $t/d.o -Wl,-map,$t/map1
$t/exe1
[ "$(names $t/exe1)" = "a1 b1 x1 y1 z1 " ]
maprows $t/map1 5

# With b's, c's and d's functions folded into a's.
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/a.o $t/b.o $t/c.o $t/d.o \
  -Wl,-deduplicate -Wl,-map,$t/map2
$t/exe2
[ "$(names $t/exe2)" = "a1 b1 b7 c7 j1 k1 w6 x1 x6 x7 y1 z1 " ]
maprows $t/map2 12
