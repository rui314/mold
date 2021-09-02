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

#include <tbb/version.h>

//! \file test_implicit_linkage_on_windows.cpp
//! \brief Test for [internal] functionality

//! Testing the library is implicitly linked on Windows platforms
//! \brief \ref error_guessing \ref interface
TEST_CASE("Test implicit linkage") {
    // Pulling something from the library so that it is indeed required during the linkage
    REQUIRE_MESSAGE(
        TBB_runtime_interface_version()==TBB_INTERFACE_VERSION,
        "Running with the library of different version than the test was compiled against."
    );
}
