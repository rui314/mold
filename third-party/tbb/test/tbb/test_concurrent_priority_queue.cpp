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

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#include <common/concurrent_priority_queue_common.h>
#include <common/containers_common.h>

//! \file test_concurrent_priority_queue.cpp
//! \brief Test for [containers.concurrent_priority_queue] specification

void test_cpq_with_smart_pointers() {
    const int NUMBER = 10;

    utils::FastRandom<> rnd(1234);

    std::vector<std::shared_ptr<int>> shared_pointers;
    for (int i = 0; i < NUMBER; ++i ) {
        const int rnd_get = rnd.get();
        shared_pointers.emplace_back(std::make_shared<int>(rnd_get));
    }
    std::vector<std::weak_ptr<int>> weak_pointers;
    std::copy(shared_pointers.begin(), shared_pointers.end(), std::back_inserter(weak_pointers));

    type_tester(shared_pointers, LessForSmartPointers{});
    type_tester(weak_pointers, LessForSmartPointers{});

    std::vector<int> arrInt;
    for (int i = 0; i < NUMBER; ++i)
        arrInt.emplace_back(rnd.get());

    type_tester_unique_ptr(arrInt); // Test std::unique_ptr
}

struct MyDataType {
    std::size_t priority;
    char padding[tbb::detail::max_nfs_size - sizeof(int) % tbb::detail::max_nfs_size];

    MyDataType() = default;
    MyDataType( int val ) : priority(std::size_t(val)) {}

    const MyDataType operator+( const MyDataType& other ) const {
        return MyDataType(int(priority + other.priority));
    }

    bool operator==(const MyDataType& other) const {
        return this->priority == other.priority;
    }
}; // struct MyDataType

const MyDataType DATA_MIN(INT_MIN);
const MyDataType DATA_MAX(INT_MAX);

struct MyLess {
    bool operator()( const MyDataType d1, const MyDataType d2 ) const {
        return d1.priority < d2.priority;
    }
}; // struct MyLess

void test_concurrent( std::size_t n ) {
    test_parallel_push_pop<MyLess>(n, DATA_MAX, DATA_MIN);
    test_flogger<MyLess, MyDataType>(n);
}

void test_multithreading() {
    for (std::size_t n = utils::MinThread; n != utils::MaxThread; ++n) {
        test_concurrent(n);
    }
}

struct MyThrowingType : public MyDataType {
    static int throw_flag;
    MyThrowingType() = default;
    MyThrowingType( const MyThrowingType& src ) : MyDataType(src) {
        if (throw_flag) {
            TBB_TEST_THROW(42);
        }
    }

    MyThrowingType& operator=( const MyThrowingType& other ) {
        priority = other.priority;
        return *this;
    }
};

int MyThrowingType::throw_flag = 0;

using CPQExTestType = tbb::concurrent_priority_queue<MyThrowingType, MyLess>;

#if TBB_USE_EXCEPTIONS
void test_exceptions() {
    // TODO: TBB_USE_EXCEPTIONS?
    const std::size_t TOO_LARGE_SZ = std::vector<MyThrowingType, typename CPQExTestType::allocator_type>{}.max_size() + 1;

    REQUIRE(TOO_LARGE_SZ < std::numeric_limits<std::size_t>::max());
    MyThrowingType elem;

    // Allocation of empty queue should not throw
    REQUIRE_NOTHROW([]{
        MyThrowingType::throw_flag = 1;
        CPQExTestType q;
    }());

    // Allocation of small queue should not throw for reasonably sized type
    REQUIRE_NOTHROW([]{
        MyThrowingType::throw_flag = 1;
        CPQExTestType(42);
    }());

    // Allocate a queue with too large initial size
    REQUIRE_THROWS([&]{
        MyThrowingType::throw_flag = 0;
        CPQExTestType q(TOO_LARGE_SZ);
    }());

    // Test copy ctor exceptions
    MyThrowingType::throw_flag = 0;
    CPQExTestType src_q(42);
    elem.priority = 42;
    for (std::size_t i = 0; i < 42; ++i) src_q.push(elem);

    REQUIRE_THROWS_MESSAGE([&]{
        MyThrowingType::throw_flag = 1;
        CPQExTestType q(src_q);
    }(), "Copy ctor did not throw exception");

    // Test assignment
    MyThrowingType::throw_flag = 0;
    CPQExTestType assign_q(24);

    REQUIRE_THROWS_MESSAGE([&]{
        MyThrowingType::throw_flag = 1;
        assign_q = src_q;
    }(), "Assignment did not throw exception");
    REQUIRE(assign_q.empty());

    for (std::size_t i = 0; i < push_selector_variants; ++i) {
        MyThrowingType::throw_flag = 0;
        CPQExTestType pq(3);
        REQUIRE_NOTHROW([&]{
            push_selector(pq, elem, i);
            push_selector(pq, elem, i);
            push_selector(pq, elem, i);
        }());

        try {
            MyThrowingType::throw_flag = 1;
            push_selector(pq, elem, i);
        } catch(...) {
            REQUIRE_MESSAGE(!pq.empty(), "Failed: pq should not be empty");
            REQUIRE_MESSAGE(pq.size() == 3, "Failed: pq should contain only three elements");
            REQUIRE_MESSAGE(pq.try_pop(elem), "Failed: pq is not functional");
        }

        MyThrowingType::throw_flag = 0;
        CPQExTestType pq2(3);
        REQUIRE_NOTHROW([&]{
            push_selector(pq2, elem, i);
            push_selector(pq2, elem, i);
        }());

        try {
            MyThrowingType::throw_flag = 1;
            push_selector(pq2, elem, i);
        } catch(...) {
            REQUIRE_MESSAGE(!pq2.empty(), "Failed: pq should not be empty");
            REQUIRE_MESSAGE(pq2.size() == 2, "Failed: pq should contain only two elements");
            REQUIRE_MESSAGE(pq2.try_pop(elem), "Failed: pq is not functional");
        }
    }
}
#endif

void test_scoped_allocator() {
    using allocator_data_type = AllocatorAwareData<std::scoped_allocator_adaptor<std::allocator<int>>>;
    using basic_allocator_type = std::scoped_allocator_adaptor<std::allocator<allocator_data_type>>;
    using allocator_type = std::allocator_traits<basic_allocator_type>::template rebind_alloc<allocator_data_type>;
    using container_type = tbb::concurrent_priority_queue<allocator_data_type, std::less<allocator_data_type>, allocator_type>;

    allocator_type allocator;
    allocator_data_type data1(1, allocator);
    allocator_data_type data2(1, allocator);

    container_type c1(allocator);
    container_type c2(allocator);

    allocator_data_type::activate();

    c1.push(data1);
    c2.push(std::move(data2));

    // TODO: support uses allocator construction in this place
    // c1.emplace(data1);

    c1 = c2;
    c2 = std::move(c1);

    allocator_data_type::deactivate();
}

// Testing concurrent_priority_queue with smart pointers and other special types
//! \brief \ref error_guessing
TEST_CASE("concurrent_priority_queue with smart_pointers") {
    test_cpq_with_smart_pointers();
}

//! Testing push-pop operations in concurrent_priority_queue with multithreading and specific value type
//! \brief \ref error_guessing
TEST_CASE("multithreading support in concurrent_priority_queue with specific value type") {
    test_multithreading();
}

#if TBB_USE_EXCEPTIONS
//! Testing exceptions support in concurrent_priority_queue
//! \brief \ref stress \ref error_guessing
TEST_CASE("exception handling in concurrent_priority_queue") {
    test_exceptions();
}
#endif

//! \brief \ref error_guessing
TEST_CASE("concurrent_priority_queue with std::scoped_allocator_adaptor") {
    test_scoped_allocator();
}
