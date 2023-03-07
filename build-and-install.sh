#!/bin/sh

mkdir -p build

(cd build; cmake -DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_COMPILER=c++ ..)

cmake --build ./build -j $(nproc)

sudo cmake --install ./build