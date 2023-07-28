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

#include "tbb/cache_aligned_allocator.h"
#include "tbb/tbb_allocator.h"

// the real body of the test is there:
#include "common/allocator_test_common.h"
#include "common/allocator_stl_test_common.h"

//! \file test_allocators.cpp
//! \brief Test for [memory_allocation.cache_aligned_allocator memory_allocation.tbb_allocator memory_allocation.cache_aligned_resource] specifications

#if TBB_USE_EXCEPTIONS
//! Test that cache_aligned_allocate() throws bad_alloc if cannot allocate memory.
//! \brief \ref requirement
TEST_CASE("Test cache_aligned_allocate throws") {
    #if __APPLE__
        // On macOS*, failure to map memory results in messages to stderr;
        // suppress them.
        DisableStderr disableStderr;
    #endif

    using namespace tbb::detail::r1;

    // First, allocate a reasonably big amount of memory, big enough
    // to not cause warp around in system allocator after adding object header
    // during address2 allocation.
    const size_t itemsize = 1024;
    const size_t nitems   = 1024;
    void *address1 = nullptr;
    try {
        address1 = cache_aligned_allocate(nitems * itemsize);
    } catch(...) {
        // intentionally empty
    }
    REQUIRE_MESSAGE(address1, "cache_aligned_allocate unable to obtain 1024*1024 bytes");

    bool exception_caught = false;
    try {
        // Try allocating more memory than left in the address space; should cause std::bad_alloc
        (void)cache_aligned_allocate(~size_t(0) - itemsize * nitems + cache_line_size());
    } catch (std::bad_alloc&) {
        exception_caught = true;
    } catch (...) {
        REQUIRE_MESSAGE(false, "Unexpected exception type (std::bad_alloc was expected)");
        exception_caught = true;
    }
    REQUIRE_MESSAGE(exception_caught, "cache_aligned_allocate did not throw bad_alloc");

    try {
        cache_aligned_deallocate(address1);
    } catch (...) {
        REQUIRE_MESSAGE(false, "cache_aligned_deallocate did not accept the address obtained with cache_aligned_allocate");
    }
}
#endif /* TBB_USE_EXCEPTIONS */

#if TBB_ALLOCATOR_TRAITS_BROKEN
//! Testing allocator types in case std::allocator traits is broken
//! \brief \ref error_guessing
TEST_CASE("Broken allocator concept") {
    TestAllocator<tbb::cache_aligned_allocator<void>>(Broken);
    TestAllocator<tbb::tbb_allocator<void>>(Broken);
}
#endif

//! Testing allocators compatibility with STL containers
//! \brief \ref interface
TEST_CASE("Test allocators with STL containers") {
    TestAllocatorWithSTL<tbb::cache_aligned_allocator<void>>();
    TestAllocatorWithSTL<tbb::tbb_allocator<void>>();
}

#if __TBB_CPP17_MEMORY_RESOURCE_PRESENT
//! Testing memory resources compatibility with STL containers through the
//! std::pmr::polymorphic_allocator
//! \brief \ref interface
TEST_CASE("polymorphic_allocator test") {
    tbb::cache_aligned_resource aligned_resource;
    tbb::cache_aligned_resource equal_aligned_resource(std::pmr::get_default_resource());
    REQUIRE_MESSAGE(aligned_resource.is_equal(equal_aligned_resource),
            "Underlying upstream resources should be equal.");
    REQUIRE_MESSAGE(!aligned_resource.is_equal(*std::pmr::null_memory_resource()),
            "Cache aligned resource upstream shouldn't be equal to the standard resource.");
    TestAllocatorWithSTL(std::pmr::polymorphic_allocator<void>(&aligned_resource));
}
#endif

