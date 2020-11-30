// RUN: cc -o %t1.o -c %s
// RUN: echo 'int fn1() { return 42; }' | cc -o %t2.so -shared -fPIC -Wl,-soname,libfoo.so -xc -
// RUN: echo 'int fn2() { return 42; }' | cc -o %t3.so -shared -fPIC -Wl,-soname,libbar.so -xc -

// RUN: mold -o %t.exe %t1.o %t2.so %t3.so
// RUN: readelf --dynamic %t.exe | FileCheck --check-prefix=CHECK1 %s
// RUN: mold -o %t.exe %t1.o --as-needed --no-as-needed %t2.so %t3.so
// RUN: readelf --dynamic %t.exe | FileCheck --check-prefix=CHECK1 %s
// CHECK1: 0x0000000000000001 (NEEDED) Shared library: [libfoo.so]
// CHECK1: 0x0000000000000001 (NEEDED) Shared library: [libbar.so]

// RUN: mold -o %t.exe %t1.o --as-needed %t2.so %t3.so
// RUN: readelf --dynamic %t.exe | FileCheck --check-prefix=CHECK2 %s
// CHECK2: 0x0000000000000001 (NEEDED) Shared library: [libfoo.so]
// CHECK2-NOT: 0x0000000000000001 (NEEDED) Shared library: [libbar.so]

        .text
        .globl _start
_start:
        call fn1@PLT
