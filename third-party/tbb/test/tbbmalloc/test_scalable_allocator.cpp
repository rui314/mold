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


//! \file test_scalable_allocator.cpp
//! \brief Test for [memory_allocation.scalable_allocator] functionality

// Test whether scalable_allocator complies with the requirements in 20.1.5 of ISO C++ Standard (1998).

#define __TBB_NO_IMPLICIT_LINKAGE 1

#if _WIN32 || _WIN64
#define _CRT_SECURE_NO_WARNINGS
#endif

#define __TBB_EXTRA_DEBUG 1 // enables additional checks
#define TBB_PREVIEW_MEMORY_POOL 1
#define HARNESS_TBBMALLOC_THREAD_SHUTDOWN 1

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_assert.h"
#include "common/custom_allocators.h"


#include "tbb/memory_pool.h"
#include "tbb/scalable_allocator.h"

// The actual body of the tests
#include "common/allocator_test_common.h"
#include "common/allocator_stl_test_common.h"

// #include "harness_allocator.h"

#if _MSC_VER
#include <windows.h>
#endif /* _MSC_VER */

typedef StaticCountingAllocator<tbb::memory_pool_allocator<char>> cnt_alloc_t;
typedef LocalCountingAllocator<std::allocator<char> > cnt_provider_t;

class MinimalAllocator : cnt_provider_t {
public:
    typedef char value_type;
    MinimalAllocator() {
        // REMARK("%p::ctor\n", this);
    }
    MinimalAllocator(const MinimalAllocator&s) : cnt_provider_t(s) {
        // REMARK("%p::ctor(%p)\n", this, &s);
    }
    ~MinimalAllocator() {
        /* REMARK("%p::dtor: alloc=%u/%u free=%u/%u\n", this, unsigned(items_allocated),unsigned(allocations),
            unsigned(items_freed), unsigned(frees) ); */
        REQUIRE((allocations==frees && items_allocated==items_freed));
        if( allocations ) { // non-temporal copy
            // TODO: describe consumption requirements
            REQUIRE(items_allocated>cnt_alloc_t::items_allocated);
        }
    }
    void *allocate(size_t sz) {
        void *p = cnt_provider_t::allocate(sz);
        // REMARK("%p::allocate(%u) = %p\n", this, unsigned(sz), p);
        return p;
    }
    void deallocate(void *p, size_t sz) {
        REQUIRE(allocations>frees);
        // REMARK("%p::deallocate(%p, %u)\n", this, p, unsigned(sz));
        cnt_provider_t::deallocate(std::allocator_traits<cnt_provider_t>::pointer(p), sz);
    }
};

class NullAllocator {
public:
    typedef char value_type;
    NullAllocator() { }
    NullAllocator(const NullAllocator&) { }
    ~NullAllocator() { }
    void *allocate(size_t) { return nullptr; }
    void deallocate(void *, size_t) { REQUIRE(false); }
};

void TestZeroSpaceMemoryPool()
{
    tbb::memory_pool<NullAllocator> pool;
    bool allocated = pool.malloc(16) || pool.malloc(9*1024);
    REQUIRE_MESSAGE(!allocated, "Allocator with no memory must not allocate anything.");
}

#if !TBB_USE_EXCEPTIONS
struct FixedPool {
    void  *buf;
    size_t size;
    bool   used;
    FixedPool(void *a_buf, size_t a_size) : buf(a_buf), size(a_size), used(false) {}
};

static void *fixedBufGetMem(intptr_t pool_id, size_t &bytes)
{
    if (((FixedPool*)pool_id)->used)
        return nullptr;

    ((FixedPool*)pool_id)->used = true;
    bytes = ((FixedPool*)pool_id)->size;
    return bytes? ((FixedPool*)pool_id)->buf : nullptr;
}
#endif

/* test that pools in small space are either usable or not created
   (i.e., exception raised) */
void TestSmallFixedSizePool()
{
    char *buf;
    bool allocated = false;

    for (size_t sz = 0; sz < 64*1024; sz = sz? 3*sz : 3) {
        buf = (char*)malloc(sz);
#if TBB_USE_EXCEPTIONS
        try {
            tbb::fixed_pool pool(buf, sz);
/* Check that pool is usable, i.e. such an allocation exists,
   that can be fulfilled from the pool. 16B allocation fits in 16KB slabs,
   so it requires at least 16KB. Requirement of 9KB allocation is more modest.
*/
            allocated = pool.malloc( 16 ) || pool.malloc( 9*1024 );
        } catch (std::invalid_argument&) {
            REQUIRE_MESSAGE(!sz, "expect std::invalid_argument for zero-sized pool only");
        } catch (...) {
            REQUIRE_MESSAGE(false, "wrong exception type;");
        }
#else
/* Do not test high-level pool interface because pool ctor emit exception
   on creation failure. Instead test same functionality via low-level interface.
   TODO: add support for configuration with disabled exceptions to pools.
*/
        rml::MemPoolPolicy pol(fixedBufGetMem, nullptr, 0, /*fixedSizePool=*/true,
                               /*keepMemTillDestroy=*/false);
        rml::MemoryPool *pool;
        FixedPool fixedPool(buf, sz);

        rml::MemPoolError ret = pool_create_v1((intptr_t)&fixedPool, &pol, &pool);

        if (ret == rml::POOL_OK) {
            allocated = pool_malloc(pool, 16) || pool_malloc(pool, 9*1024);
            pool_destroy(pool);
        } else
            REQUIRE_MESSAGE(ret == rml::NO_MEMORY, "Expected that pool either valid or have no memory to be created");
#endif
        free(buf);
    }
    REQUIRE_MESSAGE(allocated, "Maximal buf size should be enough to create working fixed_pool");
#if TBB_USE_EXCEPTIONS
    try {
        tbb::fixed_pool pool(nullptr, 10*1024*1024);
        REQUIRE_MESSAGE(false, "Useless allocator with no memory must not be created");
    } catch (std::invalid_argument&) {
    } catch (...) {
        REQUIRE_MESSAGE(false, "wrong exception type; expected invalid_argument");
    }
#endif
}

//! Testing ISO C++ allocator requirements
//! \brief \ref interface \ref requirement
TEST_CASE("Allocator concept") {
#if _MSC_VER && !__TBBMALLOC_NO_IMPLICIT_LINKAGE && !__TBB_WIN8UI_SUPPORT
#ifdef _DEBUG
    REQUIRE_MESSAGE((!GetModuleHandle("tbbmalloc.dll") && GetModuleHandle("tbbmalloc_debug.dll")),
        "test linked with wrong (non-debug) tbbmalloc library");
#else
    REQUIRE_MESSAGE((!GetModuleHandle("tbbmalloc_debug.dll") && GetModuleHandle("tbbmalloc.dll")),
        "test linked with wrong (debug) tbbmalloc library");
#endif // _DEBUG
#endif // _MSC_VER && !__TBBMALLOC_NO_IMPLICIT_LINKAGE
    // allocate/deallocate
    TestAllocator<tbb::scalable_allocator<void>>(Concept);
    {
        tbb::memory_pool<tbb::scalable_allocator<int>> pool;
        TestAllocator(Concept, tbb::memory_pool_allocator<void>(pool));
    }{
        // tbb::memory_pool<MinimalAllocator> pool;
        // cnt_alloc_t alloc( tbb::memory_pool_allocator<char>(pool) ); // double parentheses to avoid function declaration
        // TestAllocator(Concept, alloc);
    }{
        static char buf[1024*1024*4];
        tbb::fixed_pool pool(buf, sizeof(buf));
        const char *text = "this is a test";// 15 bytes
        char *p1 = (char*)pool.malloc( 16 );
        REQUIRE(p1);
        strcpy(p1, text);
        char *p2 = (char*)pool.realloc( p1, 15 );
        REQUIRE_MESSAGE( (p2 && !strcmp(p2, text)), "realloc broke memory" );

        TestAllocator(Concept, tbb::memory_pool_allocator<void>(pool) );

        // try allocate almost entire buf keeping some reasonable space for internals
        char *p3 = (char*)pool.realloc( p2, sizeof(buf)-128*1024 );
        REQUIRE_MESSAGE( p3, "defragmentation failed" );
        REQUIRE_MESSAGE( !strcmp(p3, text), "realloc broke memory" );
        for( size_t sz = 10; sz < sizeof(buf); sz *= 2) {
            REQUIRE( pool.malloc( sz ) );
            pool.recycle();
        }

        TestAllocator(Concept, tbb::memory_pool_allocator<void>(pool) );
    }{
        // Two nested level allocators case with fixed pool allocator as an underlying layer
        // serving allocRawMem requests for the top level scalable allocator
        typedef tbb::memory_pool<tbb::memory_pool_allocator<char, tbb::fixed_pool> > NestedPool;

        static char buffer[8*1024*1024];
        tbb::fixed_pool fixedPool(buffer, sizeof(buffer));
        // Underlying fixed pool allocator
        tbb::memory_pool_allocator<char, tbb::fixed_pool> fixedPoolAllocator(fixedPool);
        // Memory pool that handles fixed pool allocator
        NestedPool nestedPool(fixedPoolAllocator);
        // Top level memory pool allocator
        tbb::memory_pool_allocator<char, NestedPool> nestedAllocator(nestedPool);

        TestAllocator(Concept, nestedAllocator);
    }
    tbb::memory_pool<tbb::scalable_allocator<int>> mpool;

    tbb::memory_pool_allocator<int> mpalloc(mpool);

    TestAllocator<tbb::memory_pool_allocator<int>>(Concept, mpalloc);
    TestAllocator<tbb::memory_pool_allocator<void>>(Concept, mpalloc);

    // operator==
    TestAllocator<tbb::scalable_allocator<void>>(Comparison);
    TestAllocator<tbb::memory_pool_allocator<void>>(Comparison, tbb::memory_pool_allocator<void>(mpool));
    TestAllocator<tbb::memory_pool_allocator<int>>(Comparison, mpalloc);
    TestAllocator<tbb::memory_pool_allocator<void>>(Comparison, mpalloc);
}

#if TBB_USE_EXCEPTIONS
//! Testing exception guarantees
//! \brief \ref requirement
TEST_CASE("Exceptions") {
    TestAllocator<tbb::scalable_allocator<void>>(Exceptions);
}
#endif /* TBB_USE_EXCEPTIONS */

//! Testing allocators thread safety (should not introduce data races)
//! \brief \ref requirements
TEST_CASE("Thread safety") {
    TestAllocator<tbb::scalable_allocator<void>>(ThreadSafety);
}

//! Test that pools in small space are either usable or not created (i.e., exception raised)
//! \brief \ref error_guessing
TEST_CASE("Small fixed pool") {
    TestSmallFixedSizePool();
}

//! Test that allocator with no memory must not allocate anything.
//! \brief \ref error_guessing
TEST_CASE("Zero space pool") {
    TestZeroSpaceMemoryPool();
}

#if TBB_ALLOCATOR_TRAITS_BROKEN
//! Testing allocator traits is broken
//! \brief \ref error_guessing
TEST_CASE("Broken allocator concept") {
    TestAllocator<tbb::scalable_allocator<void>>(Broken);
    
    tbb::memory_pool<tbb::scalable_allocator<int>> mpool; 
    TestAllocator<tbb::memory_pool_allocator<void>>(Broken, tbb::memory_pool_allocator<void>(mpool));
}
#endif


//! Testing allocators compatibility with STL containers
//! \brief \ref interface
TEST_CASE("Integration with STL containers") {
    TestAllocatorWithSTL<tbb::scalable_allocator<void> >();
    tbb::memory_pool<tbb::scalable_allocator<int> > mpool;
    TestAllocatorWithSTL(tbb::memory_pool_allocator<void>(mpool) );
    static char buf[1024*1024*4];
    tbb::fixed_pool fpool(buf, sizeof(buf));
    TestAllocatorWithSTL(tbb::memory_pool_allocator<void>(fpool) );
}

#if __TBB_CPP17_MEMORY_RESOURCE_PRESENT
//! Testing memory resources compatibility with STL containers through std::pmr::polymorphic_allocator
//! \brief \ref interface
TEST_CASE("polymorphic_allocator test") {
    REQUIRE_MESSAGE(!tbb::scalable_memory_resource()->is_equal(*std::pmr::get_default_resource()),
            "Scalable resource shouldn't be equal to standard resource." );
    REQUIRE_MESSAGE(tbb::scalable_memory_resource()->is_equal(*tbb::scalable_memory_resource()),
            "Memory that was allocated by one scalable resource should be deallocated by any other instance.");

    typedef std::pmr::polymorphic_allocator<void> pmr_alloc_t;
    TestAllocatorWithSTL(pmr_alloc_t(tbb::scalable_memory_resource()));
}
#endif

