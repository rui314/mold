#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void leaf() {}
void middle() { leaf(); }
void unused() {}
int main() { middle(); }
EOF

# The chain from _leaf back to the entry point, on stderr.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip -Wl,-why_live,_leaf 2> $t/log
grep -q '^_leaf from .*/a.o' $t/log
grep -q '^  _middle from .*/a.o' $t/log
grep -q '^    _main from .*/a.o' $t/log

# A dead symbol prints nothing; a wildcard matches several.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip -Wl,-why_live,_unused 2> $t/log2
! grep -q _unused $t/log2 || false

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip -Wl,-why_live,'_m*' 2> $t/log3
grep -q '^_middle' $t/log3
grep -q '^_main' $t/log3

# A root says why it is one, but a chain ends at its root without that.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip -Wl,-why_live,_main 2> $t/log4
grep -q '^_main from .*/a.o' $t/log4
grep -q '^  initial-undef$' $t/log4
not grep -q 'initial-undef' $t/log

# ld-prime walks from each root in turn, the -u symbols and then the
# entry point first, and prints a chain each time a reference reaches a
# matching subsection, and a reason each time a root is one; an
# initializer pointer is a mod-init-ptr.
cat <<EOF | $CC -o $t/b.o -c -xc -
int leaf(void) { return 1; }
int middle(void) { return leaf() + 1; }
int other(void) { return leaf() + 2; }
int main(void) { return middle() + other(); }
EOF
cat <<EOF | $CC -o $t/c.o -c -xc -
int leaf(void);
__attribute__((constructor)) static void init(void) { leaf(); }
EOF
dir=$(cd $t && pwd -P)
why_live() {
  $CC --ld-path=$mold -o $t/out $t/b.o -Wl,-dead_strip "$@" 2>&1 >/dev/null |
    grep -v warning | sed -e "s#$dir/##"
}

why_live $t/c.o -Wl,-why_live,_leaf > $t/log5
cat > $t/log5.expected <<EOF
_leaf from b.o
  _middle from b.o
    _main from b.o
_leaf from b.o
  _other from b.o
    _main from b.o
_leaf from b.o
  _init from c.o
    mod-init-ptr from c.o
EOF
diff $t/log5.expected $t/log5

why_live -Wl,-why_live,_main,-u,_main,-export_dynamic > $t/log6
cat > $t/log6.expected <<EOF
_main from b.o
  initial-undef
_main from b.o
  initial-undef
_main from b.o
  global-dont-strip
EOF
diff $t/log6.expected $t/log6

why_live -Wl,-why_live,_leaf,-u,_other > $t/log9
cat > $t/log9.expected <<EOF
_leaf from b.o
  _other from b.o
_leaf from b.o
  _middle from b.o
    _main from b.o
EOF
diff $t/log9.expected $t/log9

why_live -shared -Wl,-why_live,_leaf > $t/log7
cat > $t/log7.expected <<EOF
_leaf from b.o
  global-dont-strip
_leaf from b.o
  _middle from b.o
_leaf from b.o
  _other from b.o
EOF
diff $t/log7.expected $t/log7

# In an object without subsections a section is one subsection, named
# by the symbol at its start, and every other symbol in it one of its
# own that refers to that one (an arm64 ltmpN label as "none"). An
# executable's header is a root of its own.
cat <<EOF | $CC -o $t/d.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
.globl _f1
_f1:
  ret
EOF
$CC --ld-path=$mold -o $t/out $t/d.o -Wl,-dead_strip -Wl,-why_live,'*' 2>&1 >/dev/null |
  grep -v warning | sed -e "s#$dir/##" > $t/log8
{
  echo '_main from d.o'
  echo '  initial-undef'
  echo '_main from d.o'
  echo '  dont-dead-strip'
  if [ $ARCH = arm64 ]; then
    echo 'ltmp0 from d.o'
    echo '  dont-dead-strip'
    echo '_main from d.o'
    echo '  none from d.o'
  fi
  echo '_f1 from d.o'
  echo '  dont-dead-strip'
  echo '_main from d.o'
  echo '  _f1 from d.o'
  echo '__mh_execute_header from boundary-file'
  echo '  dont-dead-strip'
  echo 'segment$start$__TEXT from boundary-file'
  echo '  __mh_execute_header from boundary-file'
} > $t/log8.expected
diff $t/log8.expected $t/log8
