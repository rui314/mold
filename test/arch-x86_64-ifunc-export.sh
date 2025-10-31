#!/bin/bash
. $(dirname $0)/common.inc

supports_ifunc || skip

cat <<EOF | $CXX -c -fPIC -o $t/a.o -xc++ -
__attribute__((target("default"))) void myfunc(int) {}
__attribute__((target("avx512f"))) void myfunc(int) {}

template <typename T>
struct MyClass {
  MyClass(T data) { myfunc(data); }
};

template struct MyClass<int>;
EOF

$CXX -shared -o $t/b.so $t/a.o

cat <<EOF | $CXX -c -fPIC -o $t/c.o -xc++ -
__attribute__((target("default"))) void myfunc(int);
__attribute__((target("avx512f"))) void myfunc(int);

template <typename T>
struct MyClass {
  MyClass(T data) { myfunc(data); }
};

extern template struct MyClass<int>;

int main() {
  MyClass<int> obj(3);
}
EOF

$CXX -B. -o $t/exe $t/c.o $t/b.so -fno-PIE
$QEMU $t/exe
