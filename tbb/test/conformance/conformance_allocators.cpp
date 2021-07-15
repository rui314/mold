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

#include "oneapi/tbb/cache_aligned_allocator.h"
#include "oneapi/tbb/tbb_allocator.h"

// the real body of the test is there:
#include "common/allocator_test_common.h"
#include "common/allocator_stl_test_common.h"

//! \file conformance_allocators.cpp
//! \brief Test for [memory_allocation.cache_aligned_allocator memory_allocation.tbb_allocator memory_allocation.cache_aligned_resource] specifications

//! Testing ISO C++ allocator requirements
//! \brief \ref interface \ref requirement
TEST_CASE("Allocator concept") {
    // allocate/deallocate
    TestAllocator<oneapi::tbb::cache_aligned_allocator<void>>(Concept);
    TestAllocator<oneapi::tbb::tbb_allocator<void>>(Concept);

    // max_size case for cache_aligned allocator
    using Allocator = oneapi::tbb::cache_aligned_allocator<int>;
    Allocator allocator;
    AssertSameType(allocator.max_size(), typename std::allocator_traits<Allocator>::size_type(0));
    // Following assertion catches case where max_size() is so large that computation of
    // number of bytes for such an allocation would overflow size_type.
    REQUIRE_MESSAGE((allocator.max_size() * typename std::allocator_traits<Allocator>::size_type(sizeof(int)) >= allocator.max_size()), "max_size larger than reasonable");

    // operator==
    TestAllocator<oneapi::tbb::cache_aligned_allocator<void>>(Comparison);
    TestAllocator<oneapi::tbb::tbb_allocator<void>>(Comparison);
}

#if TBB_USE_EXCEPTIONS
//! Testing exception guarantees
//! \brief \ref requirement
TEST_CASE("Exceptions") {
    TestAllocator<oneapi::tbb::cache_aligned_allocator<void>>(Exceptions);
    TestAllocator<oneapi::tbb::tbb_allocator<void>>(Exceptions);
}
#endif /* TBB_USE_EXCEPTIONS */

//! Testing allocators thread safety (should not introduce data races)
//! \brief \ref requirement
TEST_CASE("Thread safety") {
    TestAllocator<oneapi::tbb::cache_aligned_allocator<void>>(ThreadSafety);
    TestAllocator<oneapi::tbb::tbb_allocator<void>>(ThreadSafety);
    oneapi::tbb::tbb_allocator<int> tbb_alloc;
#if _MSC_VER && _MSC_VER <= 1900 && !__INTEL_COMPILER
    utils::suppress_unused_warning(tbb_alloc);
#endif
    tbb_alloc.allocator_type();
}

//! Testing tbb_allocator to return the type of allocation library used
//! \brief \ref requirement
TEST_CASE("tbb_allocator allocator type") {
    using Allocator = oneapi::tbb::tbb_allocator<int>; Allocator tbb_alloc;
#if _MSC_VER && _MSC_VER <= 1900 && !__INTEL_COMPILER
    utils::suppress_unused_warning(tbb_alloc);
#endif
    Allocator::malloc_type a_type = tbb_alloc.allocator_type();
    bool is_available_type = (a_type == Allocator::malloc_type::scalable) || (a_type == Allocator::standard);
    REQUIRE(is_available_type);
}

#if __TBB_CPP17_MEMORY_RESOURCE_PRESENT
//! Testing memory resources compatibility with STL containers through the
//! std::pmr::polymorphic_allocator
//! \brief \ref interface \ref requirement
TEST_CASE("polymorphic_allocator test") {
    oneapi::tbb::cache_aligned_resource aligned_resource;
    TestAllocator<std::pmr::polymorphic_allocator<void>>(Concept, std::pmr::polymorphic_allocator<void>(&aligned_resource));
}
#endif

