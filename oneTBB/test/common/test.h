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

#ifndef __TBB_test_common_test_H
#define __TBB_test_common_test_H

#include "config.h"

#if !defined(DOCTEST_CONFIG_IMPLEMENT)
#define DOCTEST_CONFIG_IMPLEMENT_WITH_MAIN
#endif
#if TBB_USE_EXCEPTIONS
#define TBB_TEST_THROW(x) throw x
#else
#define DOCTEST_CONFIG_NO_EXCEPTIONS_BUT_WITH_ALL_ASSERTS
#define TBB_TEST_THROW(x) FAIL("Exceptions are disabled")
#endif

#if _MSC_VER
// Due to race between exiting the process and starting of a new detached thread in Windows, thread
// local variables, which constructors or destructors have calls to runtime functions (e.g. free()
// function) can cause access violation since TBB along with runtime library may have been unloaded
// by the time these variables are constructed or destroyed. The workaround is do not call constructors
// and/or destructors for such variables if they are not used.
#include <type_traits>
template <typename T>
struct doctest_thread_local_wrapper {
    doctest_thread_local_wrapper() {
        // Default definition is ill-formed in case of non-trivial type T.
    }
    T& get() {
        if( !initialized ) {
            new(&value) T;
            initialized = true;
        }
        return value;
    }
    ~doctest_thread_local_wrapper() {
        if( initialized )
            value.~T();
    }
private:
    union { T value; };
    bool initialized = false;
};
#else  // _MSC_VER
template <typename T>
struct doctest_thread_local_wrapper {
    T& get() { return value; }
private:
    T value{};
};
#endif // _MSC_VER

#include "doctest.h"

#define CHECK_FAST(x) do { if (!(x)) { CHECK(false); } } while((void)0, 0)
#define CHECK_FAST_MESSAGE(x, y) do { if (!(x)) { CHECK_MESSAGE(false, y); } } while((void)0, 0)

#endif // __TBB_test_common_test_H
