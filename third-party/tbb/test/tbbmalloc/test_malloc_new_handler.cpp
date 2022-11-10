/*
    Copyright (c) 2005-2022 Intel Corporation

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

//! \file test_malloc_new_handler.cpp
//! \brief Test for [memory_allocation] functionality

#define __TBB_NO_IMPLICIT_LINKAGE 1

#include "common/test.h"
#include "common/utils.h"

#include "common/allocator_overload.h"

// Under ASAN current approach is not viable as it breaks the ASAN itself as well
#if !HARNESS_SKIP_TEST && TBB_USE_EXCEPTIONS && !__TBB_USE_ADDRESS_SANITIZER

#if _MSC_VER
#pragma warning (push)
// Forcing value to bool 'true' or 'false' (occurred inside tls.h)
#pragma warning (disable: 4800)
#endif //#if _MSC_VER

thread_local bool new_handler_called = false;
void customNewHandler() {
    new_handler_called = true;
    throw std::bad_alloc();
}

// Return true if operator new threw exception
bool allocateWithException(size_t big_mem) {
    bool exception_caught = false;
    try {
        // Allocate big array (should throw exception)
        char* volatile big_array = new char[big_mem];
        // If succeeded, double the size (unless it overflows) and recursively retry
        if (big_mem * 2 > big_mem) {
            exception_caught = allocateWithException(big_mem * 2);
        }
        delete[] big_array;
    } catch (const std::bad_alloc&) {
        bool is_called = new_handler_called;
        REQUIRE_MESSAGE(is_called, "User provided new_handler was not called.");
        exception_caught = true;
    }
    return exception_caught;
}

class AllocLoopBody : utils::NoAssign {
public:
    void operator()(int) const {
        size_t BIG_MEM = 100 * 1024 * 1024;
        new_handler_called = false;
        REQUIRE_MESSAGE(allocateWithException(BIG_MEM), "Operator new did not throw bad_alloc.");
    }
};

//! \brief \ref error_guessing
TEST_CASE("New handler callback") {
#if __TBB_CPP11_GET_NEW_HANDLER_PRESENT
    std::new_handler default_handler = std::get_new_handler();
    REQUIRE_MESSAGE(default_handler == nullptr, "No handler should be set at this point.");
#endif
    // Define the handler for new operations
    std::set_new_handler(customNewHandler);
    // Run the test
    utils::NativeParallelFor(8, AllocLoopBody());
    // Undo custom handler
    std::set_new_handler(nullptr);
}

#if _MSC_VER
#pragma warning (pop)
#endif

#endif // !HARNESS_SKIP_TEST && TBB_USE_EXCEPTIONS
