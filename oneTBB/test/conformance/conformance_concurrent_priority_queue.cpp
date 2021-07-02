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
#include <common/initializer_list_support.h>
#include <common/container_move_support.h>
#include <common/containers_common.h>
#include <common/test_comparisons.h>
#include <scoped_allocator>

//! \file conformance_concurrent_priority_queue.cpp
//! \brief Test for [containers.concurrent_priority_queue] specification

void test_to_vector() {
    using equality_comparison_helpers::toVector;
    int array[] = {1, 5, 6, 8, 4, 7};
    oneapi::tbb::blocked_range<int*> range = utils::make_blocked_range(array);
    std::vector<int> source(range.begin(), range.end());

    oneapi::tbb::concurrent_priority_queue<int> q(source.begin(), source.end());
    std::vector<int> from_cpq = toVector(q);

    std::sort(source.begin(), source.end());
    REQUIRE_MESSAGE(source == from_cpq, "equality_comparison_helpers::toVector incorrectly copied items from CPQ");
}

void test_basic() {
    const int NUMBER = 10;
    utils::FastRandom<> rnd(1234);

    std::vector<int> arrInt;
    for (int i = 0; i < NUMBER; ++i)
        arrInt.emplace_back(rnd.get());

    type_tester(arrInt); // Test integers
}

void test_initializer_list() {
    using namespace initializer_list_support_tests;
    test_initializer_list_support<oneapi::tbb::concurrent_priority_queue<char>>({1, 2, 3, 4, 5});
    test_initializer_list_support<oneapi::tbb::concurrent_priority_queue<int>>({});
}

struct SpecialMemberCalls {
    std::size_t copy_ctor_called_times;
    std::size_t move_ctor_called_times;
    std::size_t copy_assign_called_times;
    std::size_t move_assign_called_times;
}; // struct SpecialMemberCalls

bool operator==( const SpecialMemberCalls& lhs, const SpecialMemberCalls& rhs ) {
    return lhs.copy_ctor_called_times == rhs.copy_ctor_called_times &&
           lhs.move_ctor_called_times == rhs.move_ctor_called_times &&
           lhs.copy_assign_called_times == rhs.copy_assign_called_times &&
           lhs.move_assign_called_times == rhs.move_assign_called_times;
}

template <typename CounterType>
struct MoveOperationTrackerBase {
    static CounterType copy_ctor_called_times;
    static CounterType move_ctor_called_times;
    static CounterType copy_assign_called_times;
    static CounterType move_assign_called_times;

    static SpecialMemberCalls special_member_calls() {
        return SpecialMemberCalls{copy_ctor_called_times, move_ctor_called_times, copy_assign_called_times, move_assign_called_times};
    }
    static CounterType value_counter;
    std::size_t value;

    MoveOperationTrackerBase() : value(++value_counter) {}
    explicit MoveOperationTrackerBase( const std::size_t val ) : value(val) {}
    ~MoveOperationTrackerBase() { value = 0; }

    MoveOperationTrackerBase( const MoveOperationTrackerBase& other ) : value(other.value) {
        REQUIRE_MESSAGE(other.value, "The object has been moved or destroyed");
        ++copy_ctor_called_times;
    }

    MoveOperationTrackerBase( MoveOperationTrackerBase&& other ) noexcept : value(other.value) {
        REQUIRE_MESSAGE(other.value, "The object has been moved or destroyed");
        other.value = 0;
        ++move_ctor_called_times;
    }

    MoveOperationTrackerBase& operator=( const MoveOperationTrackerBase& other ) {
        REQUIRE_MESSAGE(other.value, "The object has been moved or destroyed");
        value = other.value;
        ++copy_assign_called_times;
        return *this;
    }

    MoveOperationTrackerBase& operator=( MoveOperationTrackerBase&& other ) noexcept {
        REQUIRE_MESSAGE(other.value, "The object has been moved or destroyed");
        value = other.value;
        other.value = 0;
        ++move_assign_called_times;
        return *this;
    }

    bool operator<( const MoveOperationTrackerBase& other ) const {
        REQUIRE_MESSAGE(value, "The object has been moved or destroyed");
        REQUIRE_MESSAGE(other.value, "The object has been moved or destroyed");
        return value < other.value;
    }
}; // struct MoveOperationTrackerBase

template<typename CounterType>
bool operator==( const MoveOperationTrackerBase<CounterType>& lhs, const MoveOperationTrackerBase<CounterType>& rhs ) {
    return !(lhs < rhs) && !(rhs < lhs);
}

using MoveOperationTracker = MoveOperationTrackerBase<std::size_t>;
using MoveOperationTrackerConc = MoveOperationTrackerBase<std::atomic<std::size_t>>;

template <typename CounterType> CounterType MoveOperationTrackerBase<CounterType>::copy_ctor_called_times(0);
template <typename CounterType> CounterType MoveOperationTrackerBase<CounterType>::move_ctor_called_times(0);
template <typename CounterType> CounterType MoveOperationTrackerBase<CounterType>::copy_assign_called_times(0);
template <typename CounterType> CounterType MoveOperationTrackerBase<CounterType>::move_assign_called_times(0);
template <typename CounterType> CounterType MoveOperationTrackerBase<CounterType>::value_counter(0);

template <typename Allocator = std::allocator<MoveOperationTracker>>
struct CPQSrcFixture {
    CPQSrcFixture& operator=( const CPQSrcFixture& ) = delete;

    enum {default_container_size = 100};
    using cpq_compare_type = std::less<MoveOperationTracker>;
    using cpq_allocator_type = typename std::allocator_traits<Allocator>::template rebind_alloc<MoveOperationTracker>;
    using cpq_type = oneapi::tbb::concurrent_priority_queue<MoveOperationTracker, cpq_compare_type, cpq_allocator_type>;

    cpq_type cpq_src;
    const std::size_t container_size;

    void init() {
        std::size_t& mcct = MoveOperationTracker::move_ctor_called_times;
        std::size_t& ccct = MoveOperationTracker::copy_ctor_called_times;
        std::size_t& cact = MoveOperationTracker::copy_assign_called_times;
        std::size_t& mact = MoveOperationTracker::move_assign_called_times;
        mcct = ccct = cact = mact = 0;

        for (std::size_t i = 1; i <= container_size; ++i) {
            cpq_src.push(MoveOperationTracker(i));
        }
        REQUIRE_MESSAGE(cpq_src.size() == container_size, "Error in test setup");
    }

    CPQSrcFixture( std::size_t size = default_container_size )
        : CPQSrcFixture(typename cpq_type::allocator_type(), size) {}

    CPQSrcFixture( const typename cpq_type::allocator_type& a, std::size_t size = default_container_size )
        : cpq_src(a), container_size(size)
    {
        init();
    }
}; // struct CPQSrcFixture

void test_steal_move_ctor() {
    using fixture_type = CPQSrcFixture<>;
    using container_type = typename fixture_type::cpq_type;
    fixture_type fixture;
    container_type src_copy{fixture.cpq_src};

    SpecialMemberCalls previous = MoveOperationTracker::special_member_calls();
    container_type dst{std::move(fixture.cpq_src)};
    REQUIRE_MESSAGE(previous == MoveOperationTracker::special_member_calls(), "Steal move ctor should not create any new elements");
    REQUIRE_MESSAGE(dst == src_copy, "cpq content changed during steal move");
    REQUIRE_MESSAGE(!(dst != src_copy), "cpq content changed during steal move");
}

void test_steal_move_ctor_with_allocator() {
    using arena_fixture_type = move_support_tests::TwoMemoryArenasFixture<MoveOperationTracker>;
    using fixture_type = CPQSrcFixture<arena_fixture_type::allocator_type>;

    arena_fixture_type arena_fixture(8 * fixture_type::default_container_size);
    fixture_type fixture(arena_fixture.source_allocator);
    fixture_type::cpq_type src_copy(fixture.cpq_src);

    SpecialMemberCalls previous = MoveOperationTracker::special_member_calls();
    fixture_type::cpq_type dst(std::move(fixture.cpq_src), arena_fixture.source_allocator);
    REQUIRE_MESSAGE(previous == MoveOperationTracker::special_member_calls(), "Steal move ctor should not create any new elements");
    REQUIRE_MESSAGE(dst == src_copy, "cpq content changed during steal move");
    REQUIRE_MESSAGE(!(dst != src_copy), "cpq content changed during steal move");
}

void test_per_element_move_ctor_with_allocator() {
    using arena_fixture_type = move_support_tests::TwoMemoryArenasFixture<MoveOperationTracker>;
    using fixture_type = CPQSrcFixture<arena_fixture_type::allocator_type>;

    arena_fixture_type arena_fixture(8 * fixture_type::default_container_size);
    fixture_type fixture(arena_fixture.source_allocator);
    fixture_type::cpq_type src_copy(fixture.cpq_src);

    SpecialMemberCalls move_ctor_called_cpq_size_times = MoveOperationTracker::special_member_calls();
    move_ctor_called_cpq_size_times.move_ctor_called_times += fixture.container_size;

    fixture_type::cpq_type dst(std::move(fixture.cpq_src), arena_fixture.dst_allocator);
    REQUIRE_MESSAGE(move_ctor_called_cpq_size_times == MoveOperationTracker::special_member_calls(),
                    "Per element move ctor should move initialize all new elements");
    REQUIRE_MESSAGE(dst == src_copy, "cpq content changed during move");
    REQUIRE_MESSAGE(!(dst != src_copy), "cpq content changed during move");
}

void test_steal_move_assign_operator() {
    using fixture_type = CPQSrcFixture<>;

    fixture_type fixture;
    fixture_type::cpq_type src_copy(fixture.cpq_src);

    fixture_type::cpq_type dst;
    SpecialMemberCalls previous = MoveOperationTracker::special_member_calls();
    dst = std::move(fixture.cpq_src);

    REQUIRE_MESSAGE(previous == MoveOperationTracker::special_member_calls(), "Steal move assign operator should not create any new elements");
    REQUIRE_MESSAGE(dst == src_copy, "cpq content changed during steal move assignment");
    REQUIRE_MESSAGE(!(dst != src_copy), "cpq content changed during steal move assignment");
}

void test_steal_move_assign_operator_with_stateful_allocator() {
    // Use stateful allocator which is propagated on move assignment
    using arena_fixture_type = move_support_tests::TwoMemoryArenasFixture<MoveOperationTracker, /*POCMA = */std::true_type>;
    using fixture_type = CPQSrcFixture<arena_fixture_type::allocator_type>;

    arena_fixture_type arena_fixture(8 * fixture_type::default_container_size);
    fixture_type fixture(arena_fixture.source_allocator);
    fixture_type::cpq_type src_copy(fixture.cpq_src);
    fixture_type::cpq_type dst(arena_fixture.dst_allocator);

    SpecialMemberCalls previous = MoveOperationTracker::special_member_calls();
    dst = std::move(fixture.cpq_src);
    REQUIRE_MESSAGE(previous == MoveOperationTracker::special_member_calls(), "Steal move assign operator should not create any new elements");
    REQUIRE_MESSAGE(dst == src_copy, "cpq content changed during steal move assignment");
    REQUIRE_MESSAGE(!(dst != src_copy), "cpq content changed during steal move assignment");
}

void test_per_element_move_assign_operator() {
    // Use stateful allocator which is not prepagated on move assignment
    using arena_fixture_type = move_support_tests::TwoMemoryArenasFixture<MoveOperationTracker, /*POCMA = */std::false_type>;
    using fixture_type = CPQSrcFixture<arena_fixture_type::allocator_type>;

    arena_fixture_type arena_fixture(8 * fixture_type::default_container_size);
    fixture_type fixture(arena_fixture.source_allocator);
    fixture_type::cpq_type src_copy(fixture.cpq_src);
    fixture_type::cpq_type dst(arena_fixture.dst_allocator);

    SpecialMemberCalls move_ctor_called_cpq_size_times = MoveOperationTracker::special_member_calls();
    move_ctor_called_cpq_size_times.move_ctor_called_times += fixture.container_size;
    dst = std::move(fixture.cpq_src);
    REQUIRE_MESSAGE(move_ctor_called_cpq_size_times == MoveOperationTracker::special_member_calls(),
                    "Per element move assignment should move initialize all new elements");
    REQUIRE_MESSAGE(dst == src_copy, "cpq content changed during per element move assignment");
    REQUIRE_MESSAGE(!(dst != src_copy), "cpq content changed during per element move assignment");
}

void test_cpq_move_constructor() {
    test_steal_move_ctor();
    test_steal_move_ctor_with_allocator();
    test_per_element_move_ctor_with_allocator();
}

void test_cpq_move_assignment() {
    test_steal_move_assign_operator();
    test_steal_move_assign_operator_with_stateful_allocator();
    test_per_element_move_assign_operator();
}


struct NoDefaultCtorType {
    NoDefaultCtorType() = delete;

    NoDefaultCtorType( std::size_t val1, std::size_t val2 ) : value1(val1), value2(val2) {}
    bool operator<(const NoDefaultCtorType& other) const {
        return value1 + value2 < other.value1 + other.value2;
    }

    std::size_t value1, value2;
}; // struct NoDefaultCtorType

struct ForwardInEmplaceTester {
    int a;
    static bool move_ctor_called;
    static bool move_assign_called;

    ForwardInEmplaceTester( int val ) : a(val) {}
    ForwardInEmplaceTester( const ForwardInEmplaceTester& ) = default;
    ForwardInEmplaceTester( ForwardInEmplaceTester&& ) = default;

    ForwardInEmplaceTester( ForwardInEmplaceTester&& obj, int val ) : a(obj.a) {
        move_ctor_called = true;
        obj.a = val;
    }

    ForwardInEmplaceTester& operator=( const ForwardInEmplaceTester& ) = default;

    ForwardInEmplaceTester& operator=( ForwardInEmplaceTester&& obj ) {
        a = obj.a;
        move_assign_called = true;
        return *this;
    }

    bool operator<( const ForwardInEmplaceTester& ) const { return true; }
}; // struct ForwardInEmplaceTester

bool ForwardInEmplaceTester::move_ctor_called = false;
bool ForwardInEmplaceTester::move_assign_called = false;

void test_move_support_in_push_pop() {
    std::size_t& mcct = MoveOperationTracker::move_ctor_called_times;
    std::size_t& ccct = MoveOperationTracker::copy_ctor_called_times;
    std::size_t& cact = MoveOperationTracker::copy_assign_called_times;
    std::size_t& mact = MoveOperationTracker::move_assign_called_times;
    mcct = ccct = cact = mact = 0;

    oneapi::tbb::concurrent_priority_queue<MoveOperationTracker> q1;

    REQUIRE_MESSAGE(mcct == 0, "Value must be zero-initialized");
    REQUIRE_MESSAGE(ccct == 0, "Value must be zero-initialized");

    q1.push(MoveOperationTracker{});
    REQUIRE_MESSAGE(mcct > 0, "Not working push(T&&)");
    REQUIRE_MESSAGE(ccct == 0, "Copying of arg occurred during push(T&&)");

    MoveOperationTracker ob;
    const std::size_t prev_mcct = mcct;
    q1.push(std::move(ob));
    REQUIRE_MESSAGE(mcct > prev_mcct, "Not working push(T&&)");
    REQUIRE_MESSAGE(ccct == 0, "Copying of arg occurred during push(T&&)");

    REQUIRE_MESSAGE(cact == 0, "Copy assignment called during push(T&&)");
    const std::size_t prev_mact = mact;
    q1.try_pop(ob);
    REQUIRE_MESSAGE(cact == 0, "Copy assignment called during try_pop(T&)");
    REQUIRE_MESSAGE(mact > prev_mact, "Move assignment was not called during try_pop(T&)");

    oneapi::tbb::concurrent_priority_queue<NoDefaultCtorType> q2;
    q2.emplace(15, 3);
    q2.emplace(2, 35);
    q2.emplace(8, 8);

    NoDefaultCtorType o(0, 0);
    q2.try_pop(o);
    REQUIRE_MESSAGE((o.value1 == 2 && o.value2 == 35), "Unexpected data popped; possible emplace() failure");
    q2.try_pop(o);
    REQUIRE_MESSAGE((o.value1 == 15 && o.value2 == 3), "Unexpected data popped; possible emplace() failure");
    q2.try_pop(o);
    REQUIRE_MESSAGE((o.value1 == 8 && o.value2 == 8), "Unexpected data popped; possible emplace() failure");
    REQUIRE_MESSAGE(!q2.try_pop(o), "The queue should be empty");

    oneapi::tbb::concurrent_priority_queue<ForwardInEmplaceTester> q3;
    REQUIRE(ForwardInEmplaceTester::move_ctor_called == false);
    q3.emplace(ForwardInEmplaceTester{5}, 2);
    REQUIRE_MESSAGE(ForwardInEmplaceTester::move_ctor_called == true, "Not used std::forward in emplace()");
    ForwardInEmplaceTester obj(0);
    q3.try_pop(obj);

    REQUIRE_MESSAGE(ForwardInEmplaceTester::move_assign_called == true, "Not used move assignment in try_pop");
    REQUIRE_MESSAGE(obj.a == 5, "Not used std::forward in emplace");
    REQUIRE_MESSAGE(!q3.try_pop(obj), "The queue should be empty");
}

// Comparator with assert in default ctor
template <typename T>
class LessA : public std::less<T> {
public:
    explicit LessA( bool no_assert = false ) {
        REQUIRE_MESSAGE(no_assert, "Default ctor should not be called");
    }
}; // class LessA

// TODO: consider use of TEST_SUITE for these tests
// TODO: combine with the constructors test from the common part
void test_ctors_dtor_accessors() {
    std::vector<int> v;
    std::allocator<int> a;

    using cpq_type = oneapi::tbb::concurrent_priority_queue<int, std::less<int>>;
    using cpq_with_compare_type = oneapi::tbb::concurrent_priority_queue<int, LessA<int>>;
    using cpq_with_compare_and_allocator_type = oneapi::tbb::concurrent_priority_queue<int, LessA<int>, std::allocator<int>>;

    LessA<int> l(true);

    // Test default ctor
    cpq_type cpq1;
    REQUIRE_MESSAGE(cpq1.size() == 0, "Failed size test for default ctor");
    REQUIRE_MESSAGE(cpq1.empty(), "Failed empty test for default ctor");

    // Test capacity ctor
    cpq_type cpq2(42);
    REQUIRE_MESSAGE(cpq2.size() == 0, "Failed size test for capacity ctor");
    REQUIRE_MESSAGE(cpq2.empty(), "Failed empty test for capacity ctor");

    // Test compare ctor
    cpq_with_compare_type cpq3(l);
    REQUIRE_MESSAGE(cpq3.size() == 0, "Failed size test for compare ctor");
    REQUIRE_MESSAGE(cpq3.empty(), "Failed empty test for compare ctor");

    // Test compare+allocator ctor
    cpq_with_compare_and_allocator_type cpq4(l, a);
    REQUIRE_MESSAGE(cpq4.size() == 0, "Failed size test for compare+allocator ctor");
    REQUIRE_MESSAGE(cpq4.empty(), "Failed empty test for compare+allocator ctor");

    // Test capacity+compare ctor
    cpq_with_compare_type cpq5(42, l);
    REQUIRE_MESSAGE(cpq5.size() == 0, "Failed size test for capacity+compare ctor");
    REQUIRE_MESSAGE(cpq5.empty(), "Failed empty test for capacity+compare ctor");

    // Test capacity+compare+allocator ctor
    cpq_with_compare_and_allocator_type cpq6(42, l, a);
    REQUIRE_MESSAGE(cpq6.size() == 0, "Failed size test for capacity+compare+allocator ctor");
    REQUIRE_MESSAGE(cpq6.empty(), "Failed empty test for capacity+compare+allocator ctor");

    // Test half-open range ctor
    for (int i = 0; i < 42; ++i) {
        v.emplace_back(i);
    }
    using equality_comparison_helpers::toVector;
    cpq_type cpq7(v.begin(), v.end());
    REQUIRE_MESSAGE(cpq7.size() == 42, "Failed size test for half-open range ctor");
    REQUIRE_MESSAGE(!cpq7.empty(), "Failed empty test for half-open range test");
    REQUIRE_MESSAGE(v == toVector(cpq7), "Failed equality test for half-open range ctor");

    // Test half-open range + compare ctor
    cpq_with_compare_type cpq8(v.begin(), v.end(), l);
    REQUIRE_MESSAGE(cpq8.size() == 42, "Failed size test for half-open range+compare ctor");
    REQUIRE_MESSAGE(!cpq8.empty(), "Failed empty test for half-open range+compare ctor");
    REQUIRE_MESSAGE(v == toVector(cpq8), "Failed equality test for half-open range+compare ctor");

    // Test copy ctor
    cpq_type cpq9(cpq7);
    REQUIRE_MESSAGE(cpq9.size() == cpq7.size(), "Failed size test for copy ctor");
    REQUIRE_MESSAGE(!cpq9.empty(), "Failed empty test for copy ctor");
    REQUIRE_MESSAGE(cpq9 == cpq7, "Failed equality test for copy ctor");
}

void test_assignment_clear_swap() {
    using equality_comparison_helpers::toVector;
    using cpq_type = oneapi::tbb::concurrent_priority_queue<int, std::less<int>>;
    std::vector<int> v;
    int e;

    for( int i = 0; i < 42; ++i )
        v.emplace_back(i);

    cpq_type q(v.begin(), v.end());
    cpq_type qo;

    // Test assignment
    qo = q;
    REQUIRE_MESSAGE(qo.size() == 42, "Failed assignment size test");
    REQUIRE_MESSAGE(!qo.empty(), "Failed assignment empty test");
    REQUIRE_MESSAGE(v == toVector(qo), "Failed assignment equality test");
    REQUIRE_MESSAGE(qo == q, "Failed assignment equality test");
    REQUIRE_MESSAGE(!(qo != q), "Failed assignment inequality test");

    cpq_type assigned_q;
    // Testing assign member function
    assigned_q.assign(v.begin(), v.end());
    REQUIRE_MESSAGE(assigned_q.size() == 42, "Failed assign size test");
    REQUIRE_MESSAGE(!assigned_q.empty(), "Failed assign empty test");
    REQUIRE_MESSAGE(v == toVector(assigned_q), "Failed assign equality test");

    // Testing clear()
    q.clear();
    REQUIRE_MESSAGE(q.size() == 0, "Failed clear size test");
    REQUIRE_MESSAGE(q.empty(), "Failed clear empty test");

    // Test assignment again
    for (std::size_t i = 0; i < 5; ++i)
        qo.try_pop(e);

    q = qo;
    REQUIRE_MESSAGE(q.size() == 37, "Failed assignment size test");
    REQUIRE_MESSAGE(!q.empty(), "Failed assignment empty test");

    for (std::size_t i = 0; i < 5; ++i)
        qo.try_pop(e);

    q.swap(qo);

    REQUIRE_MESSAGE(q.size() == 32, "Failed swap size test");
    REQUIRE_MESSAGE(!q.empty(), "Failed swap empty test");
    REQUIRE_MESSAGE(qo.size() == 37, "Failed swap size test");
    REQUIRE_MESSAGE(!qo.empty(), "Failed swap empty test");
}

void test_serial_push_pop() {
    oneapi::tbb::concurrent_priority_queue<int, std::less<int>> q(MAX_ITER);
    int e = 42;
    int prev = INT_MAX;
    std::size_t count = 0;

    // Test serial push
    for (std::size_t i = 0; i < MAX_ITER; ++i) {
        push_selector(q, e, i);
        e = e*-1 + int(i);
    }

    REQUIRE_MESSAGE(q.size() == MAX_ITER, "Failed push size test");
    REQUIRE_MESSAGE(!q.empty(), "Failed push empty test");

    // Test serial pop
    while(!q.empty()) {
        REQUIRE_MESSAGE(q.try_pop(e), "Failed pop test");
        REQUIRE_MESSAGE(prev >= e, "Failed pop priority test");
        prev = e;
        ++count;

        REQUIRE_MESSAGE(q.size() == MAX_ITER - count, "Failed pop size test");
        REQUIRE_MESSAGE((!q.empty() || count == MAX_ITER), "Failed pop empty test");
    }
    REQUIRE_MESSAGE(!q.try_pop(e), "Failed: successful pop from the empty queue");
}

void test_concurrent(std::size_t n) {
    test_parallel_push_pop<std::less<int>>(n, INT_MAX, INT_MIN);
    test_parallel_push_pop<std::less<int>>(n, (unsigned char)CHAR_MAX, (unsigned char)CHAR_MIN);

    test_flogger<std::less<int>, int>(n);
    test_flogger<std::less<int>, unsigned char>(n);

    MoveOperationTrackerConc::copy_assign_called_times = 0;
    test_flogger<std::less<MoveOperationTrackerConc>, MoveOperationTrackerConc>(n);
    REQUIRE_MESSAGE(MoveOperationTrackerConc::copy_assign_called_times == 0, "Copy assignment called during try_pop");
}

void test_multithreading() {
    for (std::size_t n = utils::MinThread; n != utils::MaxThread; ++n) {
        test_concurrent(n);
    }
}

struct CPQTraits {
    template <typename T>
    using container_value_type = T;

    template <typename T, typename Allocator>
    using container_type = oneapi::tbb::concurrent_priority_queue<T, std::less<T>, Allocator>;
}; // struct CPQTraits

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
template <template <typename...>typename TQueue>
void TestDeductionGuides() {
    using ComplexType = const std::string*;
    std::string s("s");
    std::vector<ComplexType> v;
    auto l = {ComplexType(&s), ComplexType(&s) };

    // check TQueue(InputIterator, InputIterator)
    TQueue qv(v.begin(), v.end());
    static_assert(std::is_same<decltype(qv), TQueue<ComplexType> >::value);

    // check TQueue(InputIterator, InputIterator, Allocator)
    TQueue qva(v.begin(), v.end(), std::allocator<ComplexType>());
    static_assert(std::is_same<decltype(qva), TQueue<ComplexType, std::less<ComplexType>,
        std::allocator<ComplexType>>>::value);

    // check TQueue(InputIterator, InputIterator, Compare)
    TQueue qvc(v.begin(), v.end(), LessA<ComplexType>(true));
    static_assert(std::is_same<decltype(qvc), TQueue<ComplexType, LessA<ComplexType>>>::value);

    // check TQueue(InputIterator, InputIterator, Compare, Allocator)
    TQueue qvca(v.begin(), v.end(), LessA<ComplexType>(true), std::allocator<ComplexType>());
    static_assert(std::is_same<decltype(qvca), TQueue<ComplexType, LessA<ComplexType>,
        std::allocator<ComplexType>>>::value);

    // check TQueue(std::initializer_list)
    TQueue ql(l);
    static_assert(std::is_same<decltype(ql), TQueue<ComplexType>>::value);

    // check TQueue(std::initializer_list, Allocator)
    TQueue qla(l, std::allocator<ComplexType>());
    static_assert(std::is_same<decltype(qla), TQueue<ComplexType, std::less<ComplexType>,
        std::allocator<ComplexType>>>::value);

    // check TQueue(std::initializer_list, Compare)
    TQueue qlc(l, LessA<ComplexType>(true));
    static_assert(std::is_same<decltype(qlc), TQueue<ComplexType, LessA<ComplexType>>>::value);

    // check TQueue(std::initializer_list, Compare, Allocator)
    TQueue qlca(l, LessA<ComplexType>(true), std::allocator<ComplexType>());
    static_assert(std::is_same<decltype(qlca), TQueue<ComplexType, LessA<ComplexType>,
        std::allocator<ComplexType>>>::value);

    // check TQueue(TQueue &)
    TQueue qc(qv);
    static_assert(std::is_same<decltype(qv), decltype(qv)>::value);

    // check TQueue(TQueue &, Allocator)
    TQueue qca(qva, std::allocator<ComplexType>());
    static_assert(std::is_same<decltype(qca), decltype(qva)>::value);

    // check TQueue(TQueue &&)
    TQueue qm(std::move(qv));
    static_assert(std::is_same<decltype(qm), decltype(qv)>::value);

    // check TQueue(TQueue &&, Allocator)
    TQueue qma(std::move(qva), std::allocator<ComplexType>());
    static_assert(std::is_same<decltype(qma), decltype(qva)>::value);
}
#endif // __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

template <typename CPQType>
void TestComparisonsBasic() {
    using comparisons_testing::testEqualityComparisons;
    CPQType c1, c2;
    testEqualityComparisons</*ExpectEqual = */true>(c1, c2);

    c1.emplace(1);
    testEqualityComparisons</*ExpectEqual = */false>(c1, c2);

    c2.emplace(1);
    testEqualityComparisons</*ExpectEqual = */true>(c1, c2);
}

template <typename TwoWayComparableCPQType>
void TestTwoWayComparableCPQ() {
    TwoWayComparableCPQType c1, c2;
    c1.emplace(1);
    c2.emplace(1);
    comparisons_testing::TwoWayComparable::reset();
    REQUIRE_MESSAGE(c1 == c2, "Incorrect operator == result");
    comparisons_testing::check_equality_comparison();
    REQUIRE_MESSAGE(!(c1 != c2), "Incorrect operator != result");
    comparisons_testing::check_equality_comparison();
}

void TestCPQComparisons() {
    using integral_container = oneapi::tbb::concurrent_priority_queue<int>;
    using two_way_comparable_container = oneapi::tbb::concurrent_priority_queue<comparisons_testing::TwoWayComparable>;

    TestComparisonsBasic<integral_container>();
    TestComparisonsBasic<two_way_comparable_container>();
    TestTwoWayComparableCPQ<two_way_comparable_container>();
}

// Testing basic operations with concurrent_priority_queue with integral value type
//! \brief \ref interface \ref requirement
TEST_CASE("basic test for concurrent_priority_queue") {
    test_to_vector(); // Test concurrent_priority_queue helper
    test_basic();
}

// Testing std::initializer_list interfaces in concurrent_priority_queue
//! \brief \ref interface \ref requirement
TEST_CASE("std::initializer_list support in concurrent_priority_queue") {
    test_initializer_list();
}

//! Testing concurrent_priority_queue moving constructors
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_priority_queue move constructor") {
    test_cpq_move_constructor();
}

//! Testing concurrent_priority_queue move assignment operator with different allocator types
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_priority_queue move assignment operator") {
    test_cpq_move_assignment();
}

//! Testing move semantics on basic push-pop operations
//! \brief \ref requirement
TEST_CASE("move semantics support on push-pop operations") {
    test_move_support_in_push_pop();
}

//! \brief \ref interface \ref requirement
TEST_CASE("constructors, destructor and accessors") {
    test_ctors_dtor_accessors();
}

//! \brief \ref interface \ref requirement
TEST_CASE("assignment, clear and swap operations") {
    test_assignment_clear_swap();
}

//! Testing push-pop operations in concurrent_priority_queue
//! \brief \ref requirement
TEST_CASE("serial push-pop") {
    test_serial_push_pop();
}

//! Testing push-pop operations in concurrent_priority_queue with multithreading
//! \brief \ref requirement
TEST_CASE("multithreading support in concurrent_priority_queue") {
    test_multithreading();
}

#if !_MSC_VER || _MSC_VER > 1900
// MSVC 2015 does not provide required level of allocator support for standard containers

//! \brief \ref requirement
TEST_CASE("std::allocator_traits support in concurrent_priority_queue") {
    test_allocator_traits_support<CPQTraits>();
}
#endif

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Testing Class Template Argument Deduction in concurrent_priority_queue
//! \brief \ref interface \ref requirement
TEST_CASE("CTAD support in concurrent_priority_queue") {
    TestDeductionGuides<oneapi::tbb::concurrent_priority_queue>();
}
#endif // __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_priority_queue iterator comparisons") {
    TestCPQComparisons();
}
