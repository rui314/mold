#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | c++ -c -o $t/a.o -xc++ -std=c++20 -
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

c++ --ld-path=./ld64 -o $t/exe $t/a.o
( set +e; $t/exe; true ) >& $t/log
grep -q 'terminating with uncaught exception of type Error: ERROR STRING' $t/log

echo OK
