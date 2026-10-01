#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime names an atom by the best label at its start that is not an
# alternate entry point, and makes an atom of no size of each other
# label. In the symbol table it lists those first - object by object in
# input order, each object's by rank (private extern, local, weak) and
# descending name, its alternate entry points last -, then the aliases
# of the functions -deduplicate folded into the atom, in input order,
# and the atom's own name last.
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

order() { nm -p $1 | sed -n 's/.* _\([a-z][0-9]\)$/\1/p' | tr '\n' ' '; }

# Without folding.
echo 'int call_d(void); int main() { return call_d() != 7; }' | $CC -o $t/main1.o -c -xc -
$CC --ld-path=$mold -o $t/exe1 $t/main1.o $t/d.o
$t/exe1
[ "$(order $t/exe1)" = "z1 y1 a1 b1 x1 " ]

# With b's, c's and d's functions folded into a's.
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/a.o $t/b.o $t/c.o $t/d.o \
  -Wl,-deduplicate
$t/exe2
[ "$(order $t/exe2)" = "j1 b7 c7 w6 z1 y1 a1 b1 x7 x6 x1 k1 " ]
