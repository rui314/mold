/*
    Copyright (c) 2025 Intel Corporation
    Copyright (c) 2025 UXL Foundation Contributors

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

//! \file test_tbbbind.cpp
//! \brief Test for TBBbind library, covers [configuration.debug_features]

#define TEST_CUSTOM_ASSERTION_HANDLER_ENABLED 1
#if !defined(_CRT_SECURE_NO_WARNINGS) && (_WIN32 || _WIN64)
#define _CRT_SECURE_NO_WARNINGS
#endif

#include "oneapi/tbb/global_control.h"
#include "oneapi/tbb/task_arena.h"
#include "common/test.h"
#include "common/utils_assert.h"

// no need to do it in the test
#define __TBB_SKIP_DEPENDENCY_SIGNATURE_VERIFICATION 1

#define DYNAMIC_LINK_FIND_LIB_WITH_TBB_RUNTIME_VERSION 1
#include "../../src/tbb/dynamic_link.cpp"
#include "../../src/tbb/load_tbbbind.h"

#if __TBB_WEAK_SYMBOLS_PRESENT
#pragma weak __TBB_internal_deallocate_binding_handler
#endif /* __TBB_WEAK_SYMBOLS_PRESENT */

namespace tbb {
namespace detail {
namespace r1 {
class binding_handler;

extern "C" {
void __TBB_internal_deallocate_binding_handler( binding_handler* handler_ptr );
}
}}}

using BindingHandlerType = void (*)(tbb::detail::r1::binding_handler* handler_ptr);

BindingHandlerType GetDeallocateBindingHandler() {
    using namespace tbb::detail::r1;

    BindingHandlerType deallocate_binding_handler_ptr;

    const dynamic_link_descriptor LinkTable[] = {
        DLD(__TBB_internal_deallocate_binding_handler, deallocate_binding_handler_ptr)
    };

    for (const auto& tbbbind_version : tbbbind_libraries_list) {
        if (dynamic_link(tbbbind_version, LinkTable,
                         sizeof(LinkTable) / sizeof(dynamic_link_descriptor), nullptr,
                         // use DYNAMIC_LINK_GLOBAL because we must not load TBBbind, we need to
                         // find already loaded TBBbind to get the symbol from it
                         DYNAMIC_LINK_GLOBAL)) {
            return deallocate_binding_handler_ptr;
        }
    }
    return nullptr;
}

// Testing can't be done without TBBbind.
#if __TBB_SELF_CONTAINED_TBBBIND || __TBB_HWLOC_VALID_ENVIRONMENT
static bool isTbbBindAvailable() { return true; }
#else
static bool isTbbBindAvailable() { return false; }
#endif

// All assertions in TBBbind are available only in TBB_USE_ASSERT mode.
#if TBB_USE_ASSERT
static bool canTestAsserts() { return true; }
#else
static bool canTestAsserts() { return false; }
#endif

// this code hangs in -DBUILD_SHARED_LIBS=OFF case (#1832)
#if __TBB_DYNAMIC_LOAD_ENABLED
// Must initialize system_topology and so register assertion handler in TBBbind.
// The test harness expects TBBBIND status always to be reported as part of TBB_VERSION output, so
// initialize system_topology even if TBBbind is not available.
struct Init {
    Init() {
        auto constraints = tbb::task_arena::constraints{}.set_max_threads_per_core(1);
        tbb::task_arena arena( constraints );
        arena.initialize();
    }
} init;
#endif

// The test relies on an assumption that system_topology::load_tbbbind_shared_object() find
// same instance of TBBbind as TBB uses internally.
//! Testing that assertions called inside TBBbind are handled correctly
//! \brief \ref interface \ref requirement
TEST_CASE("Using custom assertion handler inside TBBbind"
          * doctest::skip(!isTbbBindAvailable() || !canTestAsserts())) {
    // fills pointers to TBBbind entry points
    BindingHandlerType deallocate_binding_handler = GetDeallocateBindingHandler();
    REQUIRE_MESSAGE(deallocate_binding_handler != nullptr,
                    "Failed to fill deallocate_binding_handler_ptr");

    // we are expecting that deallocate_binding_handler points to TBBbind entry point
    TEST_CUSTOM_ASSERTION_HANDLER(deallocate_binding_handler(nullptr),
                                  "Trying to deallocate nullptr pointer.");
}
