/*
    Copyright (c) 2020-2022 Intel Corporation

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

//! \file test_tbb_header_secondary.cpp
//! \brief Test for [all] specification

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#ifndef NOMINMAX
#define NOMINMAX
#endif // NOMINMAX

#if _MSC_VER && TBB_USE_DEBUG
// Check that there is no conflict with _CRTDBG_MAP_ALLOC
#define _CRTDBG_MAP_ALLOC
#include "crtdbg.h"
#endif

#include <exception>

#define CHECK(x) do { if (!(x)) { std::terminate(); } } while (false)
#define CHECK_MESSAGE(x, y) CHECK(x);

#define __TBB_TEST_SECONDARY 1
#include "test_tbb_header.cpp"
