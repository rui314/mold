// RUN: mold -o %t.exe /usr/lib/x86_64-linux-gnu/crt1.o
// RUN:   /usr/lib/x86_64-linux-gnu/crti.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtbegin.o \
// RUN:   /home/ruiu/mold/test/Output/hello-dynamic.s.tmp.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crtn.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/libgcc.a \
// RUN:   /usr/lib/x86_64-linux-gnu/libgcc_s.so.1 \
// RUN:   /lib/x86_64-linux-gnu/libc.so.6 \
// RUN:   /usr/lib/x86_64-linux-gnu/libc_nonshared.a \
// RUN:   /lib/x86_64-linux-gnu/ld-linux-x86-64.so.2
