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

// Basic testing of an allocator
// Tests against requirements in 20.1.5 of ISO C++ Standard (1998).
// Does not check for thread safety or false sharing issues.
//
// Tests for compatibility with the host's STL are in
// test_Allocator_STL.h.  Those tests are in a separate file
// because they bring in lots of STL headers, and the tests here
// are supposed to work in the abscense of STL.

#ifndef __TBB_test_common_allocator_test_common_H_
#define __TBB_test_common_allocator_test_common_H_

#include "common/test.h"
#include "common/utils.h"
#include <utility> //for std::pair
#include <cstring>

//! Compile-time error if x and y have different types
template<typename T>
void AssertSameType( const T& /*x*/, const T& /*y*/ ) {}

//! The function to zero-initialize arrays; useful to avoid warnings
template <typename T>
void zero_fill(void* array, size_t n) {
    memset(array, 0, sizeof(T)*n);
}

template<typename A>
struct is_zero_filling {
    static const bool value = false;
};

int NumberOfFoo;

template<typename T, size_t N>
struct Foo {
    T foo_array[N];
    Foo() {
        zero_fill<T>(foo_array, N);
        ++NumberOfFoo;
    }
    Foo( const Foo& x ) {
        *this = x;
        //Internal call of assignment
    }
    Foo& operator=( const Foo& x ) {
        for (size_t i = 0; i < N; i++)
            foo_array[i] = x.foo_array[i];
        ++NumberOfFoo;
        return *this;
    }

    ~Foo() {
        --NumberOfFoo;
    }
};

inline char PseudoRandomValue( size_t j, size_t k ) {
    return char(j*3 ^ j>>4 ^ k);
}

#if __APPLE__
#include <fcntl.h>
#include <unistd.h>

// A RAII class to disable stderr in a certain scope. It's not thread-safe.
class DisableStderr {
    int stderrCopy;
    static void dupToStderrAndClose(int fd) {
        int ret = dup2(fd, STDERR_FILENO); // close current stderr
        REQUIRE(ret != -1);
        ret = close(fd);
        REQUIRE(ret != -1);
    }
public:
    DisableStderr() {
        int devNull = open("/dev/null", O_WRONLY);
        REQUIRE(devNull != -1);
        stderrCopy = dup(STDERR_FILENO);
        REQUIRE(stderrCopy != -1);
        dupToStderrAndClose(devNull);
    }
    ~DisableStderr() {
        dupToStderrAndClose(stderrCopy);
    }
};
#endif

//! T is type and A is allocator for that type
template<typename T, typename A>
void TestBrokenAllocator(A& a) {
    T x;
    const T cx = T();
    // See Table 32 in ISO ++ Standard
    typename A::pointer px = &x;
    typename A::const_pointer pcx = &cx;

    typename A::reference rx = x;
    REQUIRE(&rx == &x);

    typename A::const_reference rcx = cx;
    REQUIRE(&rcx==&cx);

    typename A::value_type v = x;

    typename A::size_type size;
    size = 0;
    --size;
    REQUIRE_MESSAGE(size > 0, "not an unsigned integral type?");

    typename A::difference_type difference;
    difference = 0;
    --difference;
    REQUIRE_MESSAGE(difference<0, "not an signed integral type?");

    // "rebind" tested by our caller

    REQUIRE(a.address(rx) == px);

    REQUIRE(a.address(rcx) == pcx);

    // Test "a.max_size()"
    AssertSameType(a.max_size(), typename A::size_type(0));
    // Following assertion catches case where max_size() is so large that computation of
    // number of bytes for such an allocation would overflow size_type.
    REQUIRE_MESSAGE((a.max_size() * typename A::size_type(sizeof(T)) >= a.max_size()), "max_size larger than reasonable");

    // Test "a.construct(p,t)"
    int n = NumberOfFoo;
    typename A::pointer p = a.allocate(1);
    a.construct(p, cx);
    REQUIRE_MESSAGE(NumberOfFoo == n + 1, "constructor for Foo not called?");

    // Test "a.destroy(p)"
    a.destroy(p);
    REQUIRE_MESSAGE(NumberOfFoo == n, "destructor for Foo not called?");
    a.deallocate(p, 1);

    {
        typedef typename A::template rebind<std::pair<typename A::value_type, typename A::value_type> >::other pair_allocator_type;
        pair_allocator_type pair_allocator(a);
        int NumberOfFooBeforeConstruct = NumberOfFoo;
        typename pair_allocator_type::pointer pair_pointer = pair_allocator.allocate(1);
        pair_allocator.construct(pair_pointer, cx, cx);
        REQUIRE_MESSAGE(NumberOfFoo == NumberOfFooBeforeConstruct+2, "constructor for Foo not called appropriate number of times?");

        pair_allocator.destroy(pair_pointer);
        REQUIRE_MESSAGE(NumberOfFoo == NumberOfFooBeforeConstruct, "destructor for Foo not called appropriate number of times?");
        pair_allocator.deallocate(pair_pointer, 1);
    }
}

//! T is type and A is allocator for that type
template<typename T, typename A>
void TestAllocatorConcept(A& a) {
    // Test "a.allocate(p,n)
    typename std::allocator_traits<A>::pointer array[100];
    std::size_t sizeof_T = sizeof(T);
    for(std::size_t k = 0; k < 100; ++k) {
        array[k] = a.allocate(k);
        char* s = reinterpret_cast<char*>(reinterpret_cast<void*>(array[k]));
        for(std::size_t j=0; j < k * sizeof_T; ++j)
            s[j] = PseudoRandomValue(j, k);
    }

    // Test "a.deallocate(p,n)
    for(std::size_t k = 0; k < 100; ++k) {
        char* s = reinterpret_cast<char*>(reinterpret_cast<void*>(array[k]));
        for(std::size_t j = 0; j < k * sizeof_T; ++j)
            REQUIRE(s[j] == PseudoRandomValue(j, k));
        a.deallocate(array[k], k);
    }
}

//! T is type and A is allocator for that type
template<typename T, typename A>
void TestAllocatorExceptions(A& a) {
#if TBB_USE_EXCEPTIONS
    volatile size_t too_big = (~std::size_t(0) - 1024 * 1024) / sizeof(T);
    bool exception_caught = false;
    typename std::allocator_traits<A>::pointer p1 = nullptr;
    try {
#if __APPLE__
        // On macOS*, failure to map memory results in messages to stderr;
        // suppress them.
        DisableStderr disableStderr;
#endif
        p1 = a.allocate(too_big);
    } catch (std::bad_alloc&) {
        exception_caught = true;
    }
    REQUIRE_MESSAGE(exception_caught, "allocate expected to throw bad_alloc");
    a.deallocate(p1, too_big);
#endif // TBB_USE_EXCEPTIONS
    utils::suppress_unused_warning(a);
}

#if _MSC_VER && !defined(__INTEL_COMPILER)
    // Workaround for erroneous "conditional expression is constant" warning in method check_allocate.
    #pragma warning (disable: 4127)
#endif

// A is an allocator for some type
template<typename A>
struct Body: utils::NoAssign {
    using pointer_type = typename std::allocator_traits<A>::pointer;
    using value_type = typename std::allocator_traits<A>::value_type;
    // For the int types and above this test runs too long
    static const std::size_t max_k = sizeof(value_type) < sizeof(int) ? 100000 : 5000;
    A &a;
    Body(A &a_) : a(a_) {}
    void check_allocate(pointer_type array[], std::size_t i, std::size_t t) const
    {
        REQUIRE(array[i] == nullptr);
        std::size_t size = i * (i & 3);
        array[i] = a.allocate(size);
        REQUIRE_MESSAGE(array[i] != nullptr, "allocator returned null");
        char* s = reinterpret_cast<char*>(reinterpret_cast<void*>(array[i]));
        for(std::size_t j = 0; j < size * sizeof(value_type); ++j) {
            if(is_zero_filling<typename std::allocator_traits<A>::template rebind_alloc<void>>::value)
                REQUIRE(!s[j]);
            s[j] = PseudoRandomValue(i, t);
        }
    }

    void check_deallocate(pointer_type array[], std::size_t i, std::size_t t) const
    {
        REQUIRE(array[i] != nullptr);
        size_t size = i * (i & 3);
        char* s = reinterpret_cast<char*>(reinterpret_cast<void*>(array[i]));
        for(std::size_t j=0; j < size * sizeof(value_type); ++j)
            REQUIRE_MESSAGE(s[j] == PseudoRandomValue(i, t), "Thread safety test failed");
        a.deallocate(array[i], size);
        array[i] = nullptr;
    }

    void operator()(std::size_t thread_id) const {
        pointer_type array[256];

        for(std::size_t k = 0; k < 256; ++k)
            array[k] = nullptr;
        for(std::size_t k = 0; k < max_k; ++k) {
            std::size_t i = static_cast<unsigned char>(PseudoRandomValue(k, thread_id));
            if(!array[i]) check_allocate(array, i, thread_id);
            else check_deallocate(array, i, thread_id);
        }
        for(std::size_t k = 0; k < 256; ++k)
            if(array[k])
                check_deallocate(array, k, thread_id);
    }
};

template<typename A>
void TestThreadSafety(A &a) {
    utils::NativeParallelFor(4, Body<A>(a));
}

enum TestName { Concept, Broken, Exceptions, ThreadSafety, Comparison };

template<typename Allocator>
void TestAllocator(TestName name, const Allocator &a = Allocator()) {

    using FooChar = Foo<char, 1>;
    using FooDouble = Foo<double, 1>;
    using FooInt = Foo<int, 17>;
    using FooFloat = Foo<float, 23>;
    #if TBB_ALLOCATOR_TRAITS_BROKEN
        using AllocatorFooChar = typename Allocator::template rebind<FooChar>::other;
        using AllocatorFooDouble = typename Allocator::template rebind<FooDouble>::other;
        using AllocatorFooInt = typename AllocatorFooChar::template rebind<FooInt>::other;
        using AllocatorFooFloat = typename AllocatorFooDouble::template rebind<FooFloat>::other;
    #else
        using AllocatorFooChar = typename std::allocator_traits<Allocator>::template rebind_alloc<FooChar>;
        using AllocatorFooDouble = typename std::allocator_traits<Allocator>::template rebind_alloc<FooDouble>;
        using AllocatorFooInt = typename std::allocator_traits<AllocatorFooChar>::template rebind_alloc<FooInt>;
        using AllocatorFooFloat = typename std::allocator_traits<AllocatorFooDouble>::template rebind_alloc<FooFloat>;
    #endif

    NumberOfFoo = 0;
    Allocator a_cpy(a);
    AllocatorFooChar a1(a);
    AllocatorFooDouble a2(a);
    AllocatorFooInt b1(a1);
    AllocatorFooFloat b2(a2);

    switch(name) {
    case Comparison:
        REQUIRE(a_cpy == a);
        REQUIRE(a1 == b1);
        REQUIRE(!(a2 != b2));
        break;
    case Concept:
        TestAllocatorConcept<FooInt>(b1);
        TestAllocatorConcept<typename AllocatorFooChar::value_type>(a1);
        TestAllocatorConcept<FooFloat>(b2);
        TestAllocatorConcept<typename AllocatorFooDouble::value_type>(a2);
        break;
    case Broken:
    #if TBB_ALLOCATOR_TRAITS_BROKEN
        TestBrokenAllocator<FooInt>(b1);
        TestBrokenAllocator<typename AllocatorFooChar::value_type>(a1);
        TestBrokenAllocator<FooFloat>(b2);
        TestBrokenAllocator<typename AllocatorFooDouble::value_type>(a2);
    #endif
        break;
    case Exceptions:
        TestAllocatorExceptions<FooInt>(b1);
        TestAllocatorExceptions<typename AllocatorFooChar::value_type>(a1);
        TestAllocatorExceptions<FooFloat>(b2);
        TestAllocatorExceptions<typename AllocatorFooDouble::value_type>(a2);
        break;
    case ThreadSafety:
        TestThreadSafety(a1);
        TestThreadSafety(a2);
        break;
    }
    REQUIRE_MESSAGE(NumberOfFoo == 0, "Allocate/deallocate count mismatched");
}
#endif // __TBB_test_common_allocator_test_common_H_
