/*
    Copyright (c) 2005-2021 Intel Corporation

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

//! \file test_dynamic_link.cpp
//! \brief Test for [internal] functionality

#if _WIN32 || _WIN64
#define _CRT_SECURE_NO_WARNINGS
#endif

#include "common/test.h"

enum FOO_TYPE {
    FOO_DUMMY,
    FOO_IMPLEMENTATION
};

#if _WIN32 || _WIN64
#define TEST_EXPORT
#else
#define TEST_EXPORT extern "C"
#endif /* _WIN32 || _WIN64 */

// foo "implementations".
TEST_EXPORT FOO_TYPE foo1() { return FOO_IMPLEMENTATION; }
TEST_EXPORT FOO_TYPE foo2() { return FOO_IMPLEMENTATION; }
// foo "dummies".
FOO_TYPE dummy_foo1() { return FOO_DUMMY; }
FOO_TYPE dummy_foo2() { return FOO_DUMMY; }

#include "oneapi/tbb/detail/_config.h"
// Suppress the weak symbol mechanism to avoid surplus compiler warnings.
#ifdef __TBB_WEAK_SYMBOLS_PRESENT
#undef __TBB_WEAK_SYMBOLS_PRESENT
#endif
#include "src/tbb/dynamic_link.h"

#if __TBB_DYNAMIC_LOAD_ENABLED
// Handlers.
static FOO_TYPE (*foo1_handler)() = &dummy_foo1;
static FOO_TYPE (*foo2_handler)() = &dummy_foo2;

// Table describing how to link the handlers.
static const tbb::detail::r1::dynamic_link_descriptor LinkTable[] = {
    { "foo1", (tbb::detail::r1::pointer_to_handler*)(void*)(&foo1_handler) },
    { "foo2", (tbb::detail::r1::pointer_to_handler*)(void*)(&foo2_handler) }
};
#endif

// The direct include since we want to test internal functionality.
#include "src/tbb/dynamic_link.cpp"
#include "common/utils.h"
#include "common/utils_dynamic_libs.h"

void test_dynamic_link(const char* lib_name) {
#if __TBB_DYNAMIC_LOAD_ENABLED
#if !_WIN32
    // Check if the executable exports its symbols.
    REQUIRE_MESSAGE((utils::GetAddress(utils::OpenLibrary(nullptr), "foo1") && utils::GetAddress(utils::OpenLibrary(nullptr), "foo2")),
            "The executable doesn't export its symbols. Is the -rdynamic switch set during linking?");
#endif /* !_WIN32 */
    // We want to link (or fail to link) to the symbols available from the
    // executable so it doesn't matter what the library name is specified in
    // the dynamic_link call - let it be an empty string.
    // Generally speaking the test has sense only on Linux but on Windows it
    // checks the dynamic_link graceful behavior with incorrect library name.
    if (tbb::detail::r1::dynamic_link(lib_name, LinkTable, sizeof(LinkTable) / sizeof(LinkTable[0]))) {
        REQUIRE_MESSAGE((foo1_handler && foo2_handler), "The symbols are corrupted by dynamic_link");
        REQUIRE_MESSAGE((foo1_handler() == FOO_IMPLEMENTATION && foo2_handler() == FOO_IMPLEMENTATION),
                "dynamic_link returned the successful code but symbol(s) are wrong");
    } else {
        REQUIRE_MESSAGE((foo1_handler == dummy_foo1 && foo2_handler == dummy_foo2), "The symbols are corrupted by dynamic_link");
    }
#else
    utils::suppress_unused_warning(lib_name);
#endif
}

//! Testing dynamic_link with non-existing library
//! \brief \ref error_guessing
TEST_CASE("Test dynamic_link with non-existing library") {
    test_dynamic_link("tbb_unrealNAME.so");
}

//! Testing dynamic_link
//! \brief \ref error_guessing
TEST_CASE("Test dynamic_link") {
    test_dynamic_link("");
}
