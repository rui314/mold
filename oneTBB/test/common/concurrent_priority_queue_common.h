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

#ifndef __TBB_test_common_concurrent_priority_queue_common_H
#define __TBB_test_common_concurrent_priority_queue_common_H

// We need to skip allocator_traits::is_always_equal tests for C++11 and C++14
#define __TBB_TEST_SKIP_IS_ALWAYS_EQUAL_CHECK (__cplusplus < 201703L)
#include <common/test.h>
#include <common/utils.h>
#include <oneapi/tbb/concurrent_priority_queue.h>
#include <oneapi/tbb/blocked_range.h>
#include <vector>

namespace equality_comparison_helpers {

template <typename ElementType, typename Compare, typename Allocator>
std::vector<ElementType> toVector( const tbb::concurrent_priority_queue<ElementType, Compare, Allocator>& source ) {
    auto cpq = source;
    std::vector<ElementType> v;
    v.reserve(cpq.size());

    ElementType element;
    while(cpq.try_pop(element)) {
        v.emplace_back(element);
    }
    std::reverse(v.begin(), v.end());
    return v;
}

}; // namespace equality_comparison_helpers

template <bool HasCopyCtor>
struct QueuePushHelper {
    template <typename Q, typename T>
    static void push(Q& q, T&& t) {
        q.push(std::forward<T>(t));
    }
};

template<>
template <typename Q, typename T>
void QueuePushHelper<false>::push( Q& q, T&& t ) {
    q.push(std::move(t));
}

template <bool HasCopyCtor, typename QueueType>
void examine( QueueType& q1, QueueType& q2, const std::vector<typename QueueType::value_type>& vec_sorted ) {
    using value_type = typename QueueType::value_type;

    REQUIRE((!q1.empty() && q1.size() == vec_sorted.size()));
    value_type elem;

    q2.clear();
    REQUIRE((q2.empty() && !q2.size() && !q2.try_pop(elem)));

    typename std::vector<value_type>::const_reverse_iterator it1;
    for (it1 = vec_sorted.rbegin(); q1.try_pop(elem); ++it1) {
        REQUIRE(utils::IsEqual{}(elem, *it1));
        if (std::distance(vec_sorted.rbegin(), it1) % 2) {
            QueuePushHelper<HasCopyCtor>::push(q2, elem);
        } else {
            QueuePushHelper<HasCopyCtor>::push(q2, std::move(elem));
        }
    }
    REQUIRE(it1 == vec_sorted.rend());
    REQUIRE((q1.empty() && !q1.size()));
    REQUIRE((!q2.empty() && q2.size() == vec_sorted.size()));

    q1.swap(q2);
    REQUIRE((q2.empty() && !q2.size()));
    REQUIRE((!q1.empty() && q1.size() == vec_sorted.size()));
    for (it1 = vec_sorted.rbegin(); q1.try_pop(elem); ++it1)
        REQUIRE(utils::IsEqual{}(elem, *it1));
    REQUIRE(it1 == vec_sorted.rend());
};

template <typename QueueType>
void examine( const QueueType& q, const std::vector<typename QueueType::value_type>& vec_sorted ) {
    QueueType q1(q), q2(q);
    examine</*HasCopyCtor=*/true>(q1, q2, vec_sorted);
}

// TODO: consider wrapping each constructor test into separate TEST_CASE
template <typename ValueType, typename Compare>
void type_tester( const std::vector<ValueType>& vec, Compare comp ) {
    using queue_type = tbb::concurrent_priority_queue<ValueType, Compare>;
    REQUIRE_MESSAGE(vec.size() >= 5, "Array should have at least 5 elements");

    std::vector<ValueType> vec_sorted(vec);
    std::sort(vec_sorted.begin(), vec_sorted.end(), comp);

    // Construct an empty queue
    queue_type q1;
    q1.assign(vec.begin(), vec.end());
    examine(q1, vec_sorted);

    // Constructor from std::initializer_list
    queue_type q2({vec[0], vec[1], vec[2]});
    for (auto it = vec.begin() + 3; it != vec.end(); ++it)
        q2.push(*it);
    examine(q2, vec_sorted);

    // Assignment operator with std::initializer_list
    queue_type q3;
    q3 = {vec[0], vec[1], vec[2]};
    for (auto it = vec.begin() + 3; it != vec.end(); ++it)
        q3.push(*it);
    examine(q3, vec_sorted);

    // Copy ctor
    queue_type q4(q1);
    examine(q4, vec_sorted);

    // Copy ctor with allocator
    auto alloc = q1.get_allocator();
    queue_type q4_alloc(q1, alloc);
    examine(q4_alloc, vec_sorted);

    // Constructor from the half-open interval
    queue_type q5(vec.begin(), vec.end());
    examine(q5, vec_sorted);

    // Constructor from the allocator object
    queue_type q6(alloc);
    q6.assign(vec.begin(), vec.end());
    examine(q6, vec_sorted);

    // Constructor from the comparator and allocator object
    queue_type q7(comp);
    q7.assign(vec.begin(), vec.end());
    examine(q7, vec_sorted);

    queue_type q8(comp, alloc);
    q8.assign(vec.begin(), vec.end());
    examine(q8, vec_sorted);

    // Constructor from the initial capacity, comparator and allocator
    queue_type q9(100);
    q9.assign(vec.begin(), vec.end());
    examine(q9, vec_sorted);

    queue_type q10(100, comp);
    q10.assign(vec.begin(), vec.end());
    examine(q10, vec_sorted);

    queue_type q11(100, alloc);
    q11.assign(vec.begin(), vec.end());
    examine(q11, vec_sorted);

    // Constructor from the half-open interval, compare and allocator object
    queue_type q12(vec.begin(), vec.end(), comp);
    examine(q12, vec_sorted);

    queue_type q13(vec.begin(), vec.end(), alloc);
    examine(q13, vec_sorted);

    // Constructor from the std::initializer_list from the half-open interval, compare and allocator object
    queue_type q14({vec[0], vec[1], vec[2]}, comp);
    for (auto it = vec.begin() + 3; it != vec.end(); ++it)
        q14.push(*it);
    examine(q14, vec_sorted);

    queue_type q15({vec[0], vec[1], vec[2]}, alloc);
    for (auto it = vec.begin() + 3; it != vec.end(); ++it)
        q15.push(*it);
    examine(q15, vec_sorted);
}

template <typename ValueType>
void type_tester( const std::vector<ValueType>& vec ) {
    type_tester(vec, std::less<ValueType>{});
}

struct LessForSmartPointers {
    template <typename T>
    bool operator()( const T& t1, const T& t2 ) {
        return *t1 < *t2;
    }

    template <typename T>
    bool operator()( const std::weak_ptr<T>& t1, const std::weak_ptr<T>& t2 ) {
        return *t1.lock().get() < *t2.lock().get();
    }
}; // struct LessForSmartPointers

template <typename T>
void type_tester_unique_ptr( const std::vector<T>& vec ) {
    REQUIRE_MESSAGE(vec.size() >= 5, "Array should have at least 5 elements");

    using value_type = std::unique_ptr<T>;
    using queue_type = tbb::concurrent_priority_queue<value_type, LessForSmartPointers>;

    std::vector<value_type> vec_sorted;
    for (auto& item : vec) {
        vec_sorted.push_back(value_type(new T(item)));
    }
    std::sort(vec_sorted.begin(), vec_sorted.end(), LessForSmartPointers{});

    queue_type q1, q1_copy;
    for (auto& item : vec) {
        q1.push(value_type(new T(item)));
        q1_copy.push(value_type(new T(item)));
    }
    examine</*HasCopyCtor=*/false>(q1, q1_copy, vec_sorted);

    queue_type q3_copy;
    q1.clear();

    for (auto& item : vec) {
        q1.emplace(new T(item));
    }

    queue_type q3(std::move(q1));
    examine</*HasCopyCtor=*/false>(q3, q3_copy, vec_sorted);
}

const std::size_t MAX_ITER = 10000;
const std::size_t push_selector_variants = 3; // push, push rvalue and emplace

template <typename Q, typename E>
void push_selector(Q& q, E e, std::size_t i) {
    switch(i % push_selector_variants) {
    case 0: q.push(e); break;
    case 1: q.push(std::move(e)); break;
    case 2: q.emplace(e); break;
    }
}

static std::atomic<std::size_t> counter;

template <typename T, typename C>
class FillBody {
    std::size_t n_thread;
    T my_min, my_max;
    tbb::concurrent_priority_queue<T, C>* q;
public:
    FillBody( const FillBody& ) = delete;
    FillBody& operator=( const FillBody& ) = delete;

    FillBody( std::size_t n, T max, T min, tbb::concurrent_priority_queue<T, C>* cpq )
        : n_thread(n), my_min(min), my_max(max), q(cpq) {}

    void operator()( const std::size_t thread_id ) const {
        T elem = my_min + T(int(thread_id));
        for (std::size_t i = 0; i < MAX_ITER; ++i) {
            // do some pushes
            push_selector(*q, elem, i);
            if (elem == my_max) elem = my_min;
            elem = elem + T(int(n_thread));
        }
    }
}; // class FillBody

template <typename T, typename C>
struct EmptyBody {
    T my_max;
    tbb::concurrent_priority_queue<T, C>* q;
    C less_than;
public:
    EmptyBody( const EmptyBody& ) = delete;
    EmptyBody& operator=( const EmptyBody& ) = delete;

    EmptyBody( T max, tbb::concurrent_priority_queue<T, C>* cpq )
        : my_max(max), q(cpq) {}

    void operator()( const std::size_t ) const {
        T elem(my_max), last;
        if (q->try_pop(last)) {
            ++counter;
            while(q->try_pop(elem)) {
                REQUIRE_MESSAGE(!less_than(last, elem), "Failed pop/priority test in EmptyBody");
                last = elem;
                elem = my_max;
                ++counter;
            }
        }
    }
}; // struct EmptyBody

template <typename T, typename C>
class FloggerBody {
    tbb::concurrent_priority_queue<T, C>* q;
public:
    FloggerBody( const FloggerBody& ) = delete;
    FloggerBody& operator=( const FloggerBody& ) = delete;

    FloggerBody( tbb::concurrent_priority_queue<T, C>* cpq )
        : q(cpq) {}

    void operator()( const std::size_t thread_id ) const {
        T elem = T(int(thread_id + 1));
        for (std::size_t i = 0; i < MAX_ITER; ++i) {
            push_selector(*q, elem, i);
            q->try_pop(elem);
        }
    }
}; // class FloggerBody

template <typename C, typename T>
void test_parallel_push_pop( std::size_t n, T t_max, T t_min ) {
    std::size_t qsize;

    tbb::concurrent_priority_queue<T, C> q(0);
    FillBody<T, C> filler(n, t_max, t_min, &q);
    EmptyBody<T, C> emptier(t_max, &q);

    counter = 0;
    utils::NativeParallelFor(n, filler);

    qsize = q.size();
    REQUIRE_MESSAGE(q.size() == n * MAX_ITER, "Failed concurrent push size test");
    REQUIRE_MESSAGE(!q.empty(), "Failed concurrent push empty test");

    utils::NativeParallelFor(n, emptier);
    REQUIRE_MESSAGE(counter == qsize, "Failed pop size test");
    REQUIRE_MESSAGE(q.size() == 0, "Failed pop empty test");
}

template <typename C, typename T>
void test_flogger( std::size_t n ) {
    tbb::concurrent_priority_queue<T, C> q(0);
    utils::NativeParallelFor(n, FloggerBody<T, C>{&q});
    REQUIRE_MESSAGE(q.empty(), "Failed flogger empty test");
    REQUIRE_MESSAGE(!q.size(), "Failed flogger size test");
}

#endif // __TBB_test_common_concurrent_priority_queue_common_H
