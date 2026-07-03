/*
    Copyright (c) 2025 Intel Corporation

    Licensed under the Apache License, Version 2.0 (the "License");
    you may not use this file except in compliance with the License.
    You may obtain a copy of the License at

        http://www.apache.org/licenses/LICENSE-2.0

    Unless required by applicable law or agreed to in writing, software
    distributed under the License is distributed on an "AS IS" BASIS,
    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
    See the License for the specific language governing permissions and
    limitations under the License.
*/

//! \file stub_unsigned_lib.cpp
//! \brief Test for [internal] functionality

#if _WIN32 || _WIN64
// Make sure the symbol is visible
#define STUB_EXPORT __declspec(dllexport)
#else
// On other platforms check the branch when symbol cannot be resolved by hiding it
#define STUB_EXPORT __attribute__ ((visibility ("hidden")))
#endif

#include <cstdlib>

extern "C" {

static int dummy = []() {
#if _WIN32
    // Make sure the library is not loaded on Windows in test dynamic link
    std::abort();
#endif
    return 1;
}();

STUB_EXPORT void foo() {
    std::abort();               // Test dynamic link should never call this function
}
}
