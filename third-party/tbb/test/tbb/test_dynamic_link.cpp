/*
    Copyright (c) 2005-2025 Intel Corporation
    Copyright (c) 2026 UXL Foundation Contributors

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

#if !defined(_CRT_SECURE_NO_WARNINGS) && (_WIN32 || _WIN64)
#define _CRT_SECURE_NO_WARNINGS
#endif

#define __TBB_NO_IMPLICIT_LINKAGE 1

#include "common/test.h"

// Out-of-line TBB assertion handling routines are instantiated here
#include "../../src/tbb/assert_impl.h"

enum FOO_TYPE {
    FOO_DUMMY,
    FOO_IMPLEMENTATION
};

#if _WIN32 || _WIN64
#define TEST_EXPORT
#else
#define TEST_EXPORT extern "C" __TBB_EXPORT
#endif /* _WIN32 || _WIN64 */

// foo "implementations".
TEST_EXPORT FOO_TYPE foo1() { return FOO_IMPLEMENTATION; }
TEST_EXPORT FOO_TYPE foo2() { return FOO_IMPLEMENTATION; }
// foo "dummies".
FOO_TYPE dummy_foo1() { return FOO_DUMMY; }
FOO_TYPE dummy_foo2() { return FOO_DUMMY; }

#include "oneapi/tbb/detail/_config.h"
#include "oneapi/tbb/version.h"

// Suppress the weak symbol mechanism to avoid surplus compiler warnings.
#ifdef __TBB_WEAK_SYMBOLS_PRESENT
#undef __TBB_WEAK_SYMBOLS_PRESENT
#endif


#ifndef TBB_DYNAMIC_LINK_WARNING
#define TBB_DYNAMIC_LINK_WARNING 1
#endif
#ifdef __TBB_SKIP_DEPENDENCY_SIGNATURE_VERIFICATION
#warning "Signature verification must not be turned off to fully test dynamic link functionality"
#endif
// The direct include since we want to test internal functionality
#include "src/tbb/dynamic_link.cpp"


#include "common/utils.h"
#include "common/utils_dynamic_libs.h"

#include <cstdio>               // for std::snprintf
#include <cstring>              // for std::strcmp

void test_dynamic_link(const char* lib_name) {
#if __TBB_DYNAMIC_LOAD_ENABLED
    // Handlers.
    static FOO_TYPE (*foo1_handler)() = nullptr;
    static FOO_TYPE (*foo2_handler)() = nullptr;
    foo1_handler = &dummy_foo1;
    foo2_handler = &dummy_foo2;

    // Table describing how to link the handlers.
    static const tbb::detail::r1::dynamic_link_descriptor LinkTable[] = {
        { "foo1", (tbb::detail::r1::pointer_to_handler*)(void*)(&foo1_handler) },
        { "foo2", (tbb::detail::r1::pointer_to_handler*)(void*)(&foo2_handler) }
    };
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
        REQUIRE_MESSAGE((foo1_handler == &dummy_foo1 && foo2_handler == &dummy_foo2),
                        "The symbols are corrupted by dynamic_link");
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
TEST_CASE("Test dynamic_link corner cases") {
    test_dynamic_link(nullptr);
    test_dynamic_link("");
}


#if __TBB_DYNAMIC_LOAD_ENABLED

#if !__TBB_WIN8UI_SUPPORT
//! Testing dynamic_link with existing library
//! \brief \ref requirement
TEST_CASE("Test dynamic_link with existing library") {
  // Check that not-yet-linked library is found and loaded even without specifying absolute path to
  // it. On Windows, signature validation is performed in addition.
#if _WIN32
    // Well known signed Windows library that exist in every distribution
    const char* lib_name = "user32.dll";
    const char* symbol = "MessageBoxA";
    int (*handler)(void * /*hWnd*/, const char * /*lpText*/, const char * /*lpCaption*/,
                   unsigned int /*uType*/) = nullptr;
#else
    const char* lib_name = TBBLIB_NAME; // On Linux it is the name of the library itself
    const char* symbol = "TBB_runtime_version";
    static const char* (*handler)() = nullptr;
#endif
    static const tbb::detail::r1::dynamic_link_descriptor table[] = {
        { symbol, (tbb::detail::r1::pointer_to_handler*)(void*)(&handler) },
    };

    constexpr int load_flags =
        tbb::detail::r1::DYNAMIC_LINK_DEFAULT & ~tbb::detail::r1::DYNAMIC_LINK_BUILD_ABSOLUTE_PATH;
    const bool link_result = tbb::detail::r1::dynamic_link(lib_name, table,
                                                           sizeof(table) / sizeof(table[0]),
                                                           /*handle*/nullptr, load_flags);
    char msg[128] = {0};
    std::snprintf(msg, sizeof(msg) / sizeof(msg[0]), "The library \"%s\" was not loaded", lib_name);
    REQUIRE_MESSAGE(link_result, msg);
    REQUIRE_MESSAGE(handler, "The symbol was not found.");
#if !_WIN32
    REQUIRE_MESSAGE(0 == std::strcmp(handler(), TBB_VERSION_STRING),
                    "dynamic_link returned successful code but symbol returned incorrect result");
#endif
}
#endif                          // !__TBB_WIN8UI_SUPPORT

//! Testing dynamic_link with stub library known to be unsigned (on Windows) and having no exported
//! symbols (on Linux)
// \brief \ref requirement
TEST_CASE("Test dynamic_link with bad library") {
    const int size = PATH_MAX + 1;
    const char* lib_name = TEST_LIBRARY_NAME("stub_unsigned");
    char path[size] = {0};
    const int msg_size = size + 128; // Path to the file + message
    char msg[msg_size] = {0};

#if !__TBB_WIN8UI_SUPPORT
    const std::size_t len = tbb::detail::r1::abs_path(lib_name, path, sizeof(path));
    REQUIRE_MESSAGE((0 < len && len <= PATH_MAX), "The path to the library is not built");
    std::snprintf(msg, msg_size, "Test prerequisite is not held - the path \"%s\" must exist", path);
    REQUIRE_MESSAGE(tbb::detail::r1::file_exists(path), msg);
#endif

    // The library exists, check that it will not be loaded.
    void (*handler)() = nullptr;
    static const tbb::detail::r1::dynamic_link_descriptor table[] = {
        { "foo", (tbb::detail::r1::pointer_to_handler*)(void*)(&handler) },
    };

    // Specify library name without absolute path to test that it still can be found because it
    // resides in the application (i.e. test) directory
    constexpr int load_flags =
        tbb::detail::r1::DYNAMIC_LINK_DEFAULT & ~tbb::detail::r1::DYNAMIC_LINK_BUILD_ABSOLUTE_PATH;
    const bool link_result = tbb::detail::r1::dynamic_link(lib_name, table,
                                                           sizeof(table) / sizeof(table[0]),
                                                           /*handle*/nullptr, load_flags);
    std::snprintf(msg, msg_size, "The library \"%s\" was loaded but should not have been.", path);

    // Expectation is that the library will not be loaded because:
    // a) On Windows the library is unsigned
    // b) On Linux the library does not have exported symbols
    const bool expected_link_result = false;

    REQUIRE_MESSAGE(expected_link_result == link_result, msg);
    REQUIRE_MESSAGE(nullptr == handler, "The symbol should not be changed.");
    // TODO: Verify the warning message contains "TBB dynamic link warning: The module
    // \".*stub_unsigned.*.dll\" is unsigned or has invalid signature."
}

#endif // __TBB_DYNAMIC_LOAD_ENABLED
