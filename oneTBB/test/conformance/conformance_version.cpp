/*
    Copyright (c) 2020-2021 Intel Corporation

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

#include "common/test.h"

#include "oneapi/tbb/version.h"
#include <cstring>

//! \file conformance_version.cpp
//! \brief Test for [version_information] specification

//! Testing the match of compile-time oneTBB specification version
//! \brief \ref requirement \ref interface
TEST_CASE("Test specification version") {
    const char* expected = "1.0";
    REQUIRE_MESSAGE(std::strcmp(expected, ONETBB_SPEC_VERSION) == 0,
        "Expected and actual specification versions do not match.");
}

//! Testing the match of compile-time and runtime interface versions
//! \brief \ref requirement \ref interface
TEST_CASE("Test interface version") {
    REQUIRE_MESSAGE(TBB_runtime_interface_version()==TBB_INTERFACE_VERSION,
        "Running with the library of different version than the test was compiled against.");
}

//! Testing the match of compile-time and runtime version strings
//! \brief \ref requirement \ref interface
TEST_CASE("Test version string") {
    REQUIRE_MESSAGE(std::strcmp( TBB_runtime_version(), TBB_VERSION_STRING )==0,
        "Running with the library of different version than the test was compiled against.");
}

//! Testing interface macros
//! \brief \ref requirement
TEST_CASE("Test interface version") {
    REQUIRE(TBB_INTERFACE_VERSION / 1000 == TBB_INTERFACE_VERSION_MAJOR);
    REQUIRE(TBB_INTERFACE_VERSION % 100 / 10 == TBB_INTERFACE_VERSION_MINOR);
}
