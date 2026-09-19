#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CXX -c -o $t/a.o -xc++ -std=c++20 -
#include <exception>

class Error : public std::exception {
public:
  const char *what() const noexcept override {
    return "ERROR STRING";
  }
};

static int foo() {
  throw Error();
  return 1;
}

static inline int bar = foo();

int main() {}
EOF

$CXX --ld-path=$mold -o $t/exe $t/a.o
( set +e; $t/exe; true ) >& $t/log
grep -q 'terminating .* uncaught exception of type Error: ERROR STRING' $t/log
