#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

static void ctor1() { printf("ctor1 "); }
static void ctor2() { printf("ctor2 "); }
static void ctor3() { printf("ctor3 "); }
static void ctor4() { printf("ctor4 "); }

static void dtor1() { printf("dtor1 "); }
static void dtor2() { printf("dtor2 "); }
static void dtor3() { printf("dtor3 "); }
static void dtor4() { printf("dtor4 "); }

__attribute__((aligned(sizeof(void *)), section(".ctors.65435")))
void (*ctors65435[])()  = { ctor1 };

__attribute__((aligned(sizeof(void *)), section(".ctors.65433")))
void (*ctors65433[])()  = { ctor2 };

__attribute__((aligned(sizeof(void *)), section(".ctors")))
void (*ctors[])()  = { ctor4, ctor3 };

__attribute__((aligned(sizeof(void *)), section(".dtors")))
void (*dtors[])()  = { dtor1, dtor2 };

__attribute__((aligned(sizeof(void *)), section(".dtors.65433")))
void (*dtors65433[])()  = { dtor3 };

__attribute__((aligned(sizeof(void *)), section(".dtors.65435")))
void (*dtors65435[])()  = { dtor4 };
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>

__attribute__((constructor(101)))
static void init1() { printf("init1 "); }

__attribute__((constructor))
static void init2() { printf("init2 "); }

__attribute__((destructor(101)))
static void fini1() { printf("fini1 "); }

__attribute__((destructor))
static void fini2() { printf("fini2 "); }

int main() {}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -q 'ctor1 init1 ctor2 ctor3 ctor4 init2 fini2 dtor1 dtor2 dtor3 fini1 dtor4'
