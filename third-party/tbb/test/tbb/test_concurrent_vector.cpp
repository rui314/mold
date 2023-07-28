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

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#include <common/test.h>
#include <common/spin_barrier.h>
#include <common/state_trackable.h>
#include <common/container_move_support.h>
#include <common/range_based_for_support.h>
#include <common/utils.h>
#include <common/utils_concurrency_limit.h>
#include <common/vector_types.h>
#include <common/concepts_common.h>
#include <tbb/concurrent_vector.h>
#include <tbb/tick_count.h>
#include <tbb/parallel_reduce.h>
#include <tbb/parallel_for.h>
#include <algorithm>
#include <cmath>

//! \file test_concurrent_vector.cpp
//! \brief Test for [containers.concurrent_vector] specification

void TestSort() {
    for( int n=0; n<100; n=n*3+1 ) {
        tbb::concurrent_vector<int> array(n);
        for( int i=0; i<n; ++i ){
            array.at(i) = (i*7)%n;
        }
        std::sort( array.begin(), array.end() );
        for( int i=0; i<n; ++i ){
            REQUIRE( array[i]==i );
        }
    }
}

void TestRangeBasedFor(){
    using namespace range_based_for_support_tests;

    using c_vector = tbb::concurrent_vector<int>;
    c_vector a_c_vector;

    const int sequence_length = 10;
    for (int i = 1; i<= sequence_length; ++i){
        a_c_vector.push_back(i);
    }

    REQUIRE_MESSAGE( range_based_for_accumulate(a_c_vector, std::plus<int>(), 0) == gauss_summ_of_int_sequence(sequence_length), "incorrect accumulated value generated via range based for ?");
}

struct default_container_traits {
    template <typename container_type, typename iterator_type>
    static container_type& construct_container(typename std::aligned_storage<sizeof(container_type)>::type& storage, iterator_type begin, iterator_type end){
        container_type* ptr = reinterpret_cast<container_type*>(&storage);
        new (ptr) container_type(begin, end);
        return *ptr;
    }

    template <typename container_type, typename iterator_type, typename allocator_type>
    static container_type& construct_container(typename std::aligned_storage<sizeof(container_type)>::type& storage, iterator_type begin, iterator_type end, allocator_type const& a){
        container_type* ptr = reinterpret_cast<container_type*>(&storage);
        new (ptr) container_type(begin, end, a);
        return *ptr;
    }
};

struct c_vector_type : default_container_traits {
    template <typename T, typename Allocator>
    using container_type = tbb::concurrent_vector<T, Allocator>;

    template <typename T>
    using container_value_type = T;

    using init_iterator_type = move_support_tests::FooIterator;
    template<typename element_type, typename allocator_type>
    struct apply{
        using type = tbb::concurrent_vector<element_type,  allocator_type >;
    };

    enum{ expected_number_of_items_to_allocate_for_steal_move = 0 };

    template<typename element_type, typename allocator_type, typename iterator>
    static bool equal(tbb::concurrent_vector<element_type, allocator_type > const& c, iterator begin, iterator end){
        bool equal_sizes = (size_t)std::distance(begin, end) == c.size();
        return  equal_sizes && std::equal(c.begin(), c.end(), begin);
    }
};

void TestSerialGrowByWithMoveIterators(){
    using fixture_t = move_support_tests::DefaultStatefulFixtureHelper<c_vector_type>::type;
    using vector_t = fixture_t::container_type;

    fixture_t fixture;

    vector_t dst(fixture.dst_allocator);
    dst.grow_by(std::make_move_iterator(fixture.source.begin()), std::make_move_iterator(fixture.source.end()));

    fixture.verify_content_deep_moved(dst);
}

#if HAVE_m128 || HAVE_m256

template<typename ClassWithVectorType>
void TestVectorTypes() {
    tbb::concurrent_vector<ClassWithVectorType> v;
    for( int i = 0; i < 100; ++i ) {
        // VC8 does not properly align a temporary value; to work around, use explicit variable
        ClassWithVectorType foo(i);
        v.push_back(foo);
        for( int j=0; j<i; ++j ) {
            ClassWithVectorType bar(j);
            REQUIRE( v[j]==bar );
        }
    }
}
#endif /* HAVE_m128 | HAVE_m256 */


static tbb::concurrent_vector<std::size_t> Primes;

class FindPrimes {
    bool is_prime( std::size_t val ) const {
        int limit, factor = 3;
        if( val<5u )
            return val==2;
        else {
            limit = long(sqrtf(float(val))+0.5f);
            while( factor<=limit && val % factor )
                ++factor;
            return factor>limit;
        }
    }
public:
    void operator()( const std::size_t idx ) const {
        if( idx % 2 && is_prime(idx) ) {
            Primes.push_back( idx );
        }
    }
};

double TimeFindPrimes( std::size_t nthread ) {
    Primes.clear();
    const std::size_t count = 1048576;
    Primes.reserve(count);// TODO: or compact()?
    tbb::tick_count t0 = tbb::tick_count::now();
    std::size_t block_size = count / nthread;
    utils::NativeParallelFor(count, block_size, FindPrimes() );
    tbb::tick_count t1 = tbb::tick_count::now();
    return (t1-t0).seconds();
}

void TestFindPrimes() {
    // Time fully subscribed run.

    // TimeFindPrimes( tbb::task_scheduler_init::automatic );
    double t2 = TimeFindPrimes( utils::get_platform_max_threads() );

    // Time parallel run that is very likely oversubscribed.
#if TBB_TEST_LOW_WORKLOAD
    double tx = TimeFindPrimes(32);
#else
    double tx = TimeFindPrimes(128);
#endif
    INFO("TestFindPrimes: t2 == " << t2 << " tx == " << tx << "k == " << tx / t2);

    // We allow the X-thread run a little extra time to allow for thread overhead.
    // Theoretically, following test will fail on machine with >X processors.
    // But that situation is not going to come up in the near future,
    // and the generalization to fix the issue is not worth the trouble.
    WARN_MESSAGE( tx <= 1.3*t2, "Warning: grow_by is pathetically slow");
    INFO("t2 == " << t2 << " tx == " << tx << "k == " << tx / t2);
}

template <typename Type, typename Allocator>
class test_grow_by_and_resize {
    tbb::concurrent_vector<Type, Allocator> &my_c;
public:
    test_grow_by_and_resize( tbb::concurrent_vector<Type, Allocator> &c ) : my_c(c) {}
    void operator()() const {
        const typename tbb::concurrent_vector<Type, Allocator>::size_type sz = my_c.size();
        my_c.grow_by( 5 );
        REQUIRE( my_c.size() == sz + 5 );
        my_c.resize( sz );
        REQUIRE( my_c.size() == sz );
    }
};

void test_scoped_allocator() {
    using allocator_data_type = AllocatorAwareData<std::scoped_allocator_adaptor<std::allocator<int>>>;
    using allocator_type = std::scoped_allocator_adaptor<std::allocator<allocator_data_type>>;
    using container_type = tbb::concurrent_vector<allocator_data_type, allocator_type>;

    allocator_type allocator;
    allocator_data_type data1(1, allocator);
    allocator_data_type data2(2, allocator);

    auto init_list = {data1, data2};

    container_type c1(allocator), c2(allocator);

    allocator_data_type::activate();

    c1.grow_by(100);
    c1.grow_by(10, data1);
    c1.grow_by(init_list.begin(), init_list.end());
    c1.grow_by(init_list);

    c1.clear();

    c1.grow_to_at_least(100);
    c1.grow_to_at_least(110, data1);

    c1.clear();

    c1.push_back(data1);
    c1.push_back(data2);
    c1.push_back(std::move(data1));
    c1.emplace_back(1);

    c1.clear();

    c1.reserve(100);
    c1.resize(110);
    c1.resize(100);
    c1.resize(110, data1);
    c1.resize(100, data1);

    c1.shrink_to_fit();

    c1.clear();

    c1.grow_by(10, data1);
    c2.grow_by(20, data2);

    c1 = c2;
    c2 = std::move(c1);

    allocator_data_type::deactivate();
}

template <bool default_construction_present> struct do_default_construction_test {
    template<typename FuncType> void operator() ( FuncType func ) const { func(); }
};
template <> struct do_default_construction_test<false> {
    template<typename FuncType> void operator()( FuncType ) const {}
};

template <typename Type, typename Allocator>
void CompareVectors( const tbb::concurrent_vector<Type, Allocator> &c1, const tbb::concurrent_vector<Type, Allocator> &c2 ) {
    REQUIRE( (!(c1 == c2) && c1 != c2) );
    REQUIRE( (c1 <= c2 && c1 < c2 && c2 >= c1 && c2 > c1) );
}

template <typename Type, typename Allocator>
void CompareVectors( const tbb::concurrent_vector<std::weak_ptr<Type>, Allocator> &, const tbb::concurrent_vector<std::weak_ptr<Type>, Allocator> & ) {
    /* do nothing for std::weak_ptr */
}

template <bool default_construction_present, typename Type, typename Allocator>
void Examine( tbb::concurrent_vector<Type, Allocator> c, const std::vector<Type> &vec ) {
    using vector_t = tbb::concurrent_vector<Type, Allocator>;
    using size_type_t = typename vector_t::size_type;


    REQUIRE( c.size() == vec.size() );
    for ( size_type_t i=0; i<c.size(); ++i ) {
        REQUIRE( utils::IsEqual()(c[i], vec[i]) );
    }
    do_default_construction_test<default_construction_present>()(test_grow_by_and_resize<Type,Allocator>(c));
    c.grow_by( size_type_t(5), c[0] );
    c.grow_to_at_least( c.size()+5, c.at(0) );
    vector_t c2;
    c2.reserve( 5 );
    std::copy( c.begin(), c.begin() + 5, std::back_inserter( c2 ) );

    c.grow_by( c2.begin(), c2.end() );
    const vector_t& cvcr = c;
    REQUIRE( utils::IsEqual()(cvcr.front(), *(c2.rend()-1)) );
    REQUIRE( utils::IsEqual()(cvcr.back(), *c2.rbegin()));
    REQUIRE( utils::IsEqual()(*c.cbegin(), *(c.crend()-1)) );
    REQUIRE( utils::IsEqual()(*(c.cend()-1), *c.crbegin()) );
    c.swap( c2 );
    REQUIRE( c.size() == 5 );
    CompareVectors( c, c2 );
    c.swap( c2 );
    c2.clear();
    REQUIRE( c2.size() == 0 );
    c2.shrink_to_fit();
    Allocator a = c.get_allocator();
    a.deallocate( a.allocate(1), 1 );
}

template <typename Type>
class test_default_construction {
    const std::vector<Type> &my_vec;
public:
    test_default_construction( const std::vector<Type> &vec ) : my_vec(vec) {}
    void operator()() const {
        // Construction with initial size specified by argument n.
        tbb::concurrent_vector<Type> c7( my_vec.size() );
        std::copy( my_vec.begin(), my_vec.end(), c7.begin() );
        Examine</*default_construction_present = */true>( c7, my_vec );
        tbb::concurrent_vector< Type, std::allocator<Type> > c8( my_vec.size() );
        std::copy( c7.begin(), c7.end(), c8.begin() );
        Examine</*default_construction_present = */true>( c8, my_vec );
    }
};

template <bool default_construction_present, typename Type>
void TypeTester( const std::vector<Type> &vec ) {
    __TBB_ASSERT( vec.size() >= 5, "Array should have at least 5 elements" );
    // Construct empty vector.
    tbb::concurrent_vector<Type> c1;
    std::copy( vec.begin(), vec.end(), std::back_inserter(c1) );
    Examine<default_construction_present>( c1, vec );
    // Constructor from initializer_list.
    tbb::concurrent_vector<Type> c2({vec[0],vec[1],vec[2]});
    std::copy( vec.begin()+3, vec.end(), std::back_inserter(c2) );
    Examine<default_construction_present>( c2, vec );
    // Copying constructor.
    tbb::concurrent_vector<Type> c3(c1);
    Examine<default_construction_present>( c3, vec );
    // Construct with non-default allocator
    tbb::concurrent_vector< Type, std::allocator<Type> > c4;
    std::copy( vec.begin(), vec.end(), std::back_inserter(c4) );
    Examine<default_construction_present>( c4, vec );
    // Construction with initial size specified by argument n.
    do_default_construction_test<default_construction_present>()(test_default_construction<Type>(vec));
    // Construction with initial size specified by argument n, initialization by copying of t, and given allocator instance.
    std::allocator<Type> allocator;
    tbb::concurrent_vector< Type, std::allocator<Type> > c9(vec.size(), vec[1], allocator);
    Examine<default_construction_present>( c9, std::vector<Type>(vec.size(), vec[1]) );
    // Construction with copying iteration range and given allocator instance.
    tbb::concurrent_vector< Type, std::allocator<Type> > c10(c1.begin(), c1.end(), allocator);
    Examine<default_construction_present>( c10, vec );
    tbb::concurrent_vector<Type> c11(vec.begin(), vec.end());
    Examine<default_construction_present>( c11, vec );
}

void TestTypes() {
    const int NUMBER = 100;

    std::vector<int> intArr;
    for ( int i=0; i<NUMBER; ++i ) intArr.push_back(i);
    TypeTester</*default_construction_present = */true>( intArr );

    std::vector< std::reference_wrapper<int> > refArr;
    // The constructor of std::reference_wrapper<T> from T& is explicit in some versions of libstdc++.
    for ( int i=0; i<NUMBER; ++i ) refArr.push_back( std::reference_wrapper<int>(intArr[i]) );
    TypeTester</*default_construction_present = */false>( refArr );

    // std::vector< std::atomic<int> > tbbIntArr( NUMBER ); //TODO compilation error
    // for ( int i=0; i<NUMBER; ++i ) tbbIntArr[i] = i;
    // TypeTester</*default_construction_present = */true>( tbbIntArr );

    std::vector< std::shared_ptr<int> > shrPtrArr;
    for ( int i=0; i<NUMBER; ++i ) shrPtrArr.push_back( std::make_shared<int>(i) );
    TypeTester</*default_construction_present = */true>( shrPtrArr );

    std::vector< std::weak_ptr<int> > wkPtrArr;
    std::copy( shrPtrArr.begin(), shrPtrArr.end(), std::back_inserter(wkPtrArr) );
    TypeTester</*default_construction_present = */true>( wkPtrArr );
}

template <typename Vector>
void test_grow_by_empty_range( Vector &v, typename Vector::value_type* range_begin_end ) {
    const Vector v_copy = v;
    REQUIRE_MESSAGE( (v.grow_by( range_begin_end, range_begin_end ) == v.end()), "grow_by(empty_range) returned a wrong iterator." );
    REQUIRE_MESSAGE( v == v_copy, "grow_by(empty_range) has changed the vector." );
}

void TestSerialGrowByRange( bool fragmented_vector ) {
    tbb::concurrent_vector<int> v;
    if ( fragmented_vector ) {
        v.reserve( 1 );
    }
    int init_range[] = { 1, 2, 3, 4, 5, 6, 7, 8, 9, 10 };
    REQUIRE_MESSAGE( (v.grow_by( init_range, init_range + (utils::array_length( init_range )) ) == v.begin()), "grow_by(I,I) returned a wrong iterator." );
    REQUIRE_MESSAGE( std::equal( v.begin(), v.end(), init_range ), "grow_by(I,I) did not properly copied all elements ?" );
    test_grow_by_empty_range( v, init_range );
    test_grow_by_empty_range( v, (int*)nullptr );
}

template <typename allocator_type>
void TestConcurrentOperationsWithUnSafeOperations(std::size_t threads_number) {
    using vector_type = tbb::concurrent_vector<move_support_tests::Foo, allocator_type>;

    vector_type vector;

    constexpr std::size_t max_operations = 1000;
    std::atomic<int> curr_unsafe_thread{-1};
    // 0 - is safe operations
    // 1 - is shrink_to_fit
    // 2 - is clear + shrink_to_fit
    // 3 - is resize
    std::vector<std::size_t> operations(std::size_t(max_operations * 0.95), 0);
    utils::FastRandom<> op_rand(42);
    for (std::size_t i = std::size_t(max_operations * 0.95); i < max_operations; ++i) {
        std::size_t random_operation = op_rand.get() % 3;
        operations.push_back(random_operation + 1);
    }

    // Array of active threads
    std::unique_ptr<std::atomic<int>[]> active_threads{ new std::atomic<int>[threads_number]() };
    // If thread still have i < max_operations than in array will be false
    // When some thread finish it operation, set true in active_thread on thread_id position and start executing only safe operations
    // Than wait all threads
    // When all threads is finish their operations, all thread exit from lambda
    auto all_done = [&active_threads, threads_number] {
        for (std::size_t i = 0; i < threads_number; ++i) {
            if (active_threads[i].load(std::memory_order_relaxed) == 0) return false;
        }
        return true;
    };

    // Need double synchronization to correct work
    std::unique_ptr<std::atomic<int>[]> ready_threads{ new std::atomic<int>[threads_number]() };
    auto all_ready_leave = [&ready_threads, threads_number] {
        for (std::size_t i = 0; i < threads_number; ++i) {
            if (ready_threads[i].load(std::memory_order_relaxed) == 0) return false;
        }
        return true;
    };

    utils::SpinBarrier barrier(threads_number);
    auto concurrent_func = [&operations, &vector, &curr_unsafe_thread, &barrier, &all_done, &active_threads,
                            &all_ready_leave, &ready_threads] (std::size_t thread_id)
    {
        std::vector<std::size_t> local_operations(operations);
        utils::FastRandom<> rand(thread_id);
        // std::shuffle doesn't work with msvc2017 and FastRandom
        for (std::size_t i = local_operations.size(); i > 1; --i) {
            std::size_t j = rand.get() % i;
            std::swap(local_operations[i - 1], local_operations[j]);
        }

        std::size_t i = 0;
        do {
            if (all_done()) ready_threads[thread_id] = 1;
            if (curr_unsafe_thread.load() != -1) {
                    // If lock taken, wait
                    // First wait unblock unsafe thread
                    // Second wait finish unsafe operations
                    barrier.wait();
                    barrier.wait();
            }
            // Is safe operation
            if (active_threads[thread_id] == 1 || local_operations[i] == 0) {
                // If lock is free, perform various operations
                std::size_t random_operation = rand.get() % 3;
                switch (random_operation) {
                    case 0:
                        {
                            vector.push_back(1);
                        }
                        break;
                    case 1:
                        {
                            std::size_t grow_size = rand.get() % 100;
                            vector.grow_by(grow_size, 1);
                        }
                        break;
                    case 2:
                        {
                            std::size_t grow_at_least_size = vector.size() + rand.get() % 100;
                            vector.grow_to_at_least(grow_at_least_size, 1);
                        }
                        break;
                }
            } else {
                int default_unsafe_thread = -1;
                if (curr_unsafe_thread.compare_exchange_strong(default_unsafe_thread, int(thread_id))) {
                    barrier.wait();
                    // All threads are blocked we can execute our unsafe operation
                    switch (local_operations[i]) {
                        case 1:
                            vector.shrink_to_fit();
                            break;
                        case 2:
                            {
                                vector.clear();
                                vector.shrink_to_fit();
                            }
                            break;
                        case 3:
                            {
                                vector.resize(0);
                            }
                            break;
                    }
                    curr_unsafe_thread = -1;
                    barrier.wait();
                }
            }
            ++i;
            if (i >= local_operations.size()) active_threads[thread_id] = 1;
        } while (!all_ready_leave() || !all_done());
    };

    utils::NativeParallelFor(threads_number, concurrent_func);

    vector.clear();
    vector.shrink_to_fit();
}

template <typename RangeType>
int reduce_vector(RangeType test_range) {
    return tbb::parallel_reduce(test_range, 0,
        [] ( const RangeType& range, int sum ) {
            for (auto it = range.begin(); it != range.end(); ++it) {
                sum += *it;
            }

            return sum;
        },
        [] ( const int& lhs, const int& rhs) {
            return lhs + rhs;
        }
    );
}

//! Test the grow_by on range
//! \brief \ref interface \ref requirement
TEST_CASE("testing serial grow_by range"){
    TestSerialGrowByRange(/*fragmented_vector = */false);
    TestSerialGrowByRange(/*fragmented_vector = */true);
}

//! Test of push_back operation
//! \brief \ref interface
TEST_CASE("testing range based for support"){
    TestRangeBasedFor();
}

//! Test of work STL algorithms  with concurrent_vector iterator.
//! \brief \ref interface
TEST_CASE("testing sort"){
    TestSort();
}

//! Test concurrent_vector with vector types
//! \brief \ref error_guessing
TEST_CASE("testing concurrent_vector with vector types"){
#if HAVE_m128
    TestVectorTypes<ClassWithSSE>();
#endif
#if HAVE_m256
    if (have_AVX()) TestVectorTypes<ClassWithAVX>();
#endif
}

//! Test concurrent push_back operation
//! \brief \ref error_guessing
TEST_CASE("testing find primes"){
    TestFindPrimes();
}

//! Test concurrent_vector with std::scoped_allocator_adaptor
//! \brief \ref error_guessing
TEST_CASE("test concurrent_vector with std::scoped_allocator_adaptor") {
    test_scoped_allocator();
}

//! Test type of vector
//! \brief \ref requirement
TEST_CASE("testing types"){
    TestTypes();
}

//! Test concurrent and unsafe operations
//! \brief \ref regression \ref error_guessing
TEST_CASE("Work without hang") {
    using allocator_type = StaticSharedCountingAllocator<std::allocator<move_support_tests::Foo>>;
    std::size_t max_threads = utils::get_platform_max_threads() - 1;

    for (std::size_t threads = 1; threads < max_threads; threads = std::size_t(double(threads) * 2.7)) {
        allocator_type::init_counters();
        TestConcurrentOperationsWithUnSafeOperations<allocator_type>(threads);

        REQUIRE( allocator_type::allocations == allocator_type::frees );
        REQUIRE( allocator_type::items_allocated == allocator_type::items_freed );
        REQUIRE( allocator_type::items_constructed == allocator_type::items_destroyed );
    }
}

#if TBB_USE_EXCEPTIONS
//! Whitebox test for segment table extension
//! \brief \ref regression \ref error_guessing
TEST_CASE("Whitebox test for segment table extension") {
    using allocator_type = StaticSharedCountingAllocator<std::allocator<move_support_tests::Foo>>;
    using vector_type = tbb::concurrent_vector<move_support_tests::Foo, allocator_type>;

    std::size_t max_number_of_elements_in_embedded = 12;

    for (std::size_t i = 3; i < max_number_of_elements_in_embedded; i += 3) {
        vector_type vector;
        allocator_type::init_counters();
        allocator_type::set_limits(std::size_t(1) << (i + 1));

        try {
            for (std::size_t j = 0; j < std::size_t(1) << i; ++j) {
                vector.push_back(1);
            }
            vector.grow_by(1000);
        } catch (std::bad_alloc& ) {
            allocator_type::set_limits();
            vector_type copy_of_vector(vector);
            vector_type copy_of_copy(copy_of_vector);
            vector_type assigned_vector;
            assigned_vector = vector;
            REQUIRE(copy_of_vector == copy_of_copy);
            REQUIRE(assigned_vector == copy_of_copy);
        }
    }
}

//! Test exception in constructors
//! \brief \ref regression \ref error_guessing
TEST_CASE("Test exception in constructors") {
    using allocator_type = StaticSharedCountingAllocator<std::allocator<double>>;
    using vector_type = tbb::concurrent_vector<double, allocator_type>;

    allocator_type::set_limits(1);

    REQUIRE_THROWS_AS( [] {
        vector_type vec1(42, 42.);
        utils::suppress_unused_warning(vec1);
    }(), const std::bad_alloc);

    auto list = { 42., 42., 42., 42., 42., 42., 42., 42., 42., 42. };
    REQUIRE_THROWS_AS( [&] {
        vector_type vec2(list.begin(), list.end());
        utils::suppress_unused_warning(vec2);
    }(), const std::bad_alloc);

    allocator_type::init_counters();
    allocator_type::set_limits(0);
    vector_type src_vec(42, 42.);
    allocator_type::set_limits(1);

    REQUIRE_THROWS_AS( [&] {
        vector_type vec3(src_vec, allocator_type{});
        utils::suppress_unused_warning(vec3);
    }(), const std::bad_alloc);
}
#endif // TBB_USE_EXCEPTIONS

//! \brief \ref regression \ref error_guessing
TEST_CASE("Reducing concurrent_vector") {
    constexpr int final_sum = 100000;
    tbb::concurrent_vector<int> vec(final_sum, 1);
    const tbb::concurrent_vector<int> cvec(vec);

    CHECK(reduce_vector(vec.range()) == final_sum);
    CHECK(reduce_vector(cvec.range()) == final_sum);
}


//! \brief \ref error_guessing
TEST_CASE("swap with not always equal allocators"){
    using allocator_type = NotAlwaysEqualAllocator<int>;
    using vector_type = tbb::concurrent_vector<int, allocator_type>;

    vector_type vec1{};
    vector_type vec2(42, 42);

    swap(vec1, vec2);

    CHECK(vec2.empty());
}

// The problem was that after allocating first_block,
// no write was made to the embedded table.
// Also, two threads could be in the table extension section at once.
// NOTE: If the implementation of the vector has an issue, this test will either hang
// or fail with the assertion in debug mode.
//! \brief \ref regression
TEST_CASE("Testing vector in a highly concurrent environment") {
    for (std::size_t i = 0; i < 10000; ++i) {
        tbb::concurrent_vector<int> test_vec;

        tbb::parallel_for(tbb::blocked_range<std::size_t>(0, 10000), [&] (const tbb::blocked_range<std::size_t>&) {
            test_vec.grow_by(1);
        }, tbb::static_partitioner{});

        REQUIRE(test_vec.size() == utils::get_platform_max_threads());
    }
}

#if __TBB_CPP20_CONCEPTS_PRESENT
//! \brief \ref error_guessing
TEST_CASE("container_range concept for concurrent_vector ranges") {
    static_assert(test_concepts::container_range<typename tbb::concurrent_vector<int>::range_type>);
    static_assert(test_concepts::container_range<typename tbb::concurrent_vector<int>::const_range_type>);
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT

// There was a bug in concurrent_vector that was reproduced when resize marked
// segment (that owned by my_first_block) as deleted and
// on segment allocation thread is stuck waiting this segment to be published by other thread that allocated first block.
//! Testing resize behavior for case when new size lesser than old size.
//! \brief \ref regression
TEST_CASE("testing resize on sequantual mode") {
    tbb::concurrent_vector<int> v;

    v.resize(382);
    CHECK(v.size() == 382);
    while (v.size() < 737) {
        v.emplace_back();
    }
    CHECK(v.size() == 737);

    v.resize(27);
    CHECK(v.size() == 27);
    while (v.size() < 737) {
        v.emplace_back();
    }
    CHECK(v.size() == 737);

    v.resize(1);
    CHECK(v.size() == 1);
    while (v.size() < 40) {
        v.emplace_back();
    }
    CHECK(v.size() == 40);

    v.resize(2222);
    CHECK(v.size() == 2222);
    while (v.size() < 4444) {
        v.emplace_back();
    }
    CHECK(v.size() == 4444);

    v.clear();
    CHECK(v.size() == 0);
}
