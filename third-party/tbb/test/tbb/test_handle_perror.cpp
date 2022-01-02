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

//! \file test_handle_perror.cpp
//! \brief Test for [internal] functionality

#if _WIN32 || _WIN64
#define _CRT_SECURE_NO_WARNINGS
#endif

#define __TBB_NO_IMPLICIT_LINKAGE 1

#include "../../src/tbb/assert_impl.h" // Out-of-line TBB assertion handling routines are instantiated here.
#include "common/test.h"
#include "oneapi/tbb/detail/_exception.h"
#include "../../src/tbb/exception.cpp"
#include <stdexcept>
#include <cerrno>
#include <iostream>

namespace tbb {
namespace detail {
namespace r1 {
bool terminate_on_exception() {
    return false;
}
}
}
}

#if TBB_USE_EXCEPTIONS

//! \brief \ref error_guessing
TEST_CASE("test tbb::detail::r1::handle_perror") {
    bool caught = false;

    try {
        tbb::detail::r1::handle_perror(EAGAIN, "apple");
    } catch( std::runtime_error& e ) {
        REQUIRE(std::memcmp(e.what(), "apple: ", 7) == 0);
        REQUIRE_MESSAGE(std::strlen(std::strstr(e.what(), std::strerror(EAGAIN))), "Bad error message");
        caught = true;
    }
    REQUIRE(caught);
}

#endif // TBB_USE_EXCEPTIONS
