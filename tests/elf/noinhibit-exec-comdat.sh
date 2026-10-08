#!/usr/bin/env bash
. $(dirname $0)/common.inc

# bar and baz are defined only in b.o's copy of COMDAT group foo, which
# is discarded in favor of a.o's copy.
cat <<'EOF' | $CC -fPIC -c -o $t/a.o -xc -
__asm__(".pushsection .data.foo,\"awG\",%progbits,foo,comdat\n"
        ".globl foo\n"
        "foo: .long 0\n"
        ".popsection\n");
EOF

cat <<'EOF' | $CC -fPIC -c -o $t/b.o -xc -
__asm__(".pushsection .data.foo,\"awG\",%progbits,foo,comdat\n"
        ".balign 4\n"
        ".globl foo, bar\n"
        "foo: .long 0\n"
        "bar: .long 0\n"
        ".popsection\n"
        ".pushsection .tbss.foo,\"awTG\",%nobits,foo,comdat\n"
        ".balign 4\n"
        ".globl baz\n"
        ".type baz, %tls_object\n"
        "baz: .zero 4\n"
        ".popsection\n");

extern int bar;
extern _Thread_local int baz;
int main() { return bar + baz; }
EOF

not $CC -B. -o $t/exe1 $t/a.o $t/b.o |& grep 'bar refers to a discarded COMDAT section'

not $CC -B. -o $t/exe2 -pie $t/a.o $t/b.o -Wl,-noinhibit-exec |&
  grep 'fatal: .* refers to a discarded COMDAT section'

not $CC -B. -o $t/exe3 -no-pie $t/a.o $t/b.o -Wl,-noinhibit-exec |&
  grep 'fatal: .* refers to a discarded COMDAT section'
