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

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_report.h"
#include "common/state_trackable.h"
#include "common/container_move_support.h"
#include "common/custom_allocators.h"
#include "common/initializer_list_support.h"
#include "common/containers_common.h"
#define __TBB_TEST_CPP20_COMPARISONS __TBB_CPP20_COMPARISONS_PRESENT && __TBB_CPP20_CONCEPTS_PRESENT
#include "common/test_comparisons.h"
#include "oneapi/tbb/concurrent_vector.h"
#include "oneapi/tbb/parallel_for.h"
#include "oneapi/tbb/tick_count.h"
#include "oneapi/tbb/global_control.h"
#include <initializer_list>
#include <numeric>

//! \file conformance_concurrent_vector.cpp
//! \brief Test for [containers.concurrent_vector] specification

const size_t N = 8192;

template<typename Vector, typename Iterator>
void CheckConstIterator( const Vector& u, int i, const Iterator& cp ) {
    typename Vector::const_reference pref = *cp;
    CHECK((pref.bar()==i));
    typename Vector::difference_type delta = cp-u.begin();
    REQUIRE( delta==i );
    CHECK((u[i].bar()==i));
    REQUIRE( u.begin()[i].bar()==i );
}

template<typename Iterator1, typename Iterator2, typename V>
void CheckIteratorComparison( V& u ) {
    V u2 = u;
    Iterator1 i = u.begin();

    for( int i_count=0; i_count<100; ++i_count ) {
        Iterator2 j = u.begin();
        Iterator2 i2 = u2.begin();
        for( int j_count=0; j_count<100; ++j_count ) {
            REQUIRE( ((i==j)==(i_count==j_count)) );
            REQUIRE( ((i!=j)==(i_count!=j_count)) );
            REQUIRE( ((i-j)==(i_count-j_count)) );
            REQUIRE( ((i<j)==(i_count<j_count)) );
            REQUIRE( ((i>j)==(i_count>j_count)) );
            REQUIRE( ((i<=j)==(i_count<=j_count)) );
            REQUIRE( ((i>=j)==(i_count>=j_count)) );
            REQUIRE( (!(i==i2)) );
            REQUIRE( i!=i2 );
            ++j;
            ++i2;
        }
        ++i;
    }
}

template<typename Iterator1, typename Iterator2>
void TestIteratorAssignment( Iterator2 j ) {
    Iterator1 i(j);
    REQUIRE( i==j );
    REQUIRE( !(i!=j) );
    Iterator1 k;
    k = j;
    REQUIRE( k==j );
    REQUIRE( !(k!=j) );
}

template<typename Range1, typename Range2>
void TestRangeAssignment( Range2 r2 ) {
    Range1 r1(r2); r1 = r2;
}

template<typename T>
void TestSequentialFor() {
    using V = oneapi::tbb::concurrent_vector<move_support_tests::FooWithAssign>;
    V v(N);
    REQUIRE(v.grow_by(0) == v.grow_by(0, move_support_tests::FooWithAssign()));

    // Check iterator
    typename V::iterator p = v.begin();
    REQUIRE( !(*p).is_const() );
    REQUIRE( !p->is_const() );
    for( int i=0; std::size_t(i)<v.size(); ++i, ++p ) {
        CHECK( ((*p).state==move_support_tests::Foo::DefaultInitialized) );
        typename V::reference pref = *p;
        pref.bar() = i;
        typename V::difference_type delta = p-v.begin();
        REQUIRE( delta==i );
        REQUIRE_MESSAGE( (-delta<=0), "difference type not signed?" );
    }

    // Check const_iterator going forwards
    const V& u = v;
    typename V::const_iterator cp = u.begin();
    REQUIRE( cp == v.cbegin() );
    REQUIRE( (*cp).is_const() );
    REQUIRE( (cp->is_const()) );
    REQUIRE( (*cp == v.front()) );
    for( int i=0; std::size_t(i)<u.size(); ++i ) {
        CheckConstIterator(u,i,cp);
        V::const_iterator &cpr = ++cp;
        REQUIRE_MESSAGE( (&cpr == &cp), "pre-increment not returning a reference?");
    }

    // Now go backwards
    cp = u.end();
    REQUIRE( cp == v.cend() );
    for( int i=int(u.size()); i>0; ) {
        --i;
        V::const_iterator &cpr = --cp;
        REQUIRE_MESSAGE( &cpr == &cp, "pre-decrement not returning a reference?");
        if( i>0 ) {
            typename V::const_iterator cp_old = cp--;
            intptr_t here = (*cp_old).bar();
            REQUIRE( here==u[i].bar() );
            typename V::const_iterator cp_new = cp++;
            intptr_t prev = (*cp_new).bar();
            REQUIRE( prev==u[i-1].bar() );
        }
        CheckConstIterator(u,i,cp);
    }

    // Now go forwards and backwards
    std::ptrdiff_t k = 0;
    cp = u.begin();
    for( std::size_t i=0; i<u.size(); ++i ) {
        CheckConstIterator(u,int(k),cp);
        typename V::difference_type delta = i*3 % u.size();
        if( 0<=k+delta && std::size_t(k+delta)<u.size() ) {
            V::const_iterator &cpr = (cp += delta);
            REQUIRE_MESSAGE( (&cpr == &cp), "+= not returning a reference?");
            k += delta;
        }
        delta = i*7 % u.size();
        if( 0<=k-delta && std::size_t(k-delta)<u.size() ) {
            if( i&1 ) {
                V::const_iterator &cpr = (cp -= delta);
                REQUIRE_MESSAGE( (&cpr == &cp), "-= not returning a reference?");
            } else
                cp = cp - delta;        // Test operator-
            k -= delta;
        }
    }

    for( int i=0; std::size_t(i)<u.size(); i=(i<50?i+1:i*3) )
        for( int j=-i; std::size_t(i+j)<u.size(); j=(j<50?j+1:j*5) ) {
            REQUIRE( ((u.begin()+i)[j].bar()==i+j) );
            REQUIRE( ((v.begin()+i)[j].bar()==i+j) );
            REQUIRE( ((v.cbegin()+i)[j].bar()==i+j) );
            REQUIRE( ((i+u.begin())[j].bar()==i+j) );
            REQUIRE( ((i+v.begin())[j].bar()==i+j) );
            REQUIRE(((i+v.cbegin())[j].bar()==i+j) );
        }

    CheckIteratorComparison<typename V::iterator, typename V::iterator>(v);
    CheckIteratorComparison<typename V::iterator, typename V::const_iterator>(v);
    CheckIteratorComparison<typename V::const_iterator, typename V::iterator>(v);
    CheckIteratorComparison<typename V::const_iterator, typename V::const_iterator>(v);

    TestIteratorAssignment<typename V::const_iterator>( u.begin() );
    TestIteratorAssignment<typename V::const_iterator>( v.begin() );
    TestIteratorAssignment<typename V::const_iterator>( v.cbegin() );
    TestIteratorAssignment<typename V::iterator>( v.begin() );
    // doesn't compile as expected: TestIteratorAssignment<typename V::iterator>( u.begin() );

    TestRangeAssignment<typename V::const_range_type>( u.range() );
    TestRangeAssignment<typename V::const_range_type>( v.range() );
    TestRangeAssignment<typename V::range_type>( v.range() );
    // doesn't compile as expected: TestRangeAssignment<typename V::range_type>( u.range() );

    // Check reverse_iterator
    typename V::reverse_iterator rp = v.rbegin();
    for( std::size_t i=v.size(); i>0; --i, ++rp ) {
        typename V::reference pref = *rp;
        REQUIRE( (std::size_t(pref.bar())==i-1) );
        REQUIRE( (rp!=v.rend()) );
    }
    REQUIRE( rp==v.rend() );

    // Check const_reverse_iterator
    typename V::const_reverse_iterator crp = u.rbegin();
    REQUIRE( crp == v.crbegin() );
    REQUIRE( *crp == v.back() );
    for(std::size_t i = v.size(); i>0; --i, ++crp) {
        typename V::const_reference cpref = *crp;
        REQUIRE( (std::size_t(cpref.bar())==i-1) );
        REQUIRE( crp!=u.rend() );
    }
    REQUIRE( crp == u.rend() );
    REQUIRE( crp == v.crend() );

    TestIteratorAssignment<typename V::const_reverse_iterator>( u.rbegin() );
    TestIteratorAssignment<typename V::reverse_iterator>( v.rbegin() );

    {
        oneapi::tbb::concurrent_vector<int> v1, v2(1ul, 100);
        v1.assign(1, 100);
        REQUIRE(v1 == v2);
        REQUIRE_MESSAGE((v1.size() == 1 && v1[0] == 100), "used integral iterators");
    }
}

inline void NextSize( int& s ) {
    if( s<=32 ) ++s;
    else s += s/10;
}


template<typename T, std::size_t N>
inline T* end( T(& array)[N]) {
    return array + utils::array_length(array) ;
}

template<typename vector_t>
static void CheckVector( const vector_t& cv, std::size_t expected_size, std::size_t /*old_size*/ ) {
    REQUIRE( cv.capacity()>=expected_size );
    REQUIRE( cv.size()==expected_size );
    REQUIRE( cv.empty()==(expected_size==0) );
    for( int j=0; j<int(expected_size); ++j ) {
        CHECK((cv[j].bar()==~j));
    }
}

void TestResizeAndCopy() {
    using allocator_t = StaticSharedCountingAllocator<std::allocator<move_support_tests::Foo>>;
    using vector_t = oneapi::tbb::concurrent_vector<move_support_tests::Foo, allocator_t>;
    allocator_t::init_counters();
    for( int old_size=0; old_size<=0; NextSize( old_size ) ) {
        for( int new_size=0; new_size<=8; NextSize( new_size ) ) {
            std::size_t count = move_support_tests::foo_count;

            vector_t v;
            REQUIRE( count==move_support_tests::foo_count );
            v.assign(old_size/2, move_support_tests::Foo() );
            REQUIRE( ((count+old_size/2) == move_support_tests::foo_count) );
            for( int j=0; j<old_size/2; ++j ){
                REQUIRE( v[j].state == move_support_tests::Foo::CopyInitialized);
            }

            v.assign(move_support_tests::FooIterator(0), move_support_tests::FooIterator(old_size));
            v.resize(new_size, move_support_tests::Foo(33) );
            REQUIRE(count+new_size==move_support_tests::foo_count);
            for( int j=0; j<new_size; ++j ) {
                int expected = j<old_size ? j : 33;
                CHECK((v[j].bar()==expected));
            }
            REQUIRE( v.size()==std::size_t(new_size) );
            for( int j=0; j<new_size; ++j ) {
                v[j].bar() = ~j;
            }

            const vector_t& cv = v;
            // Try copy constructor
            vector_t copy_of_v(cv);
            CheckVector(cv,new_size,old_size);

            REQUIRE( !(v != copy_of_v) );
            v.clear();

            REQUIRE( v.empty() );
            swap(v, copy_of_v);
            REQUIRE( copy_of_v.empty() );
            CheckVector(v,new_size,old_size);
        }
    }
    REQUIRE( allocator_t::items_constructed == allocator_t::items_destroyed );
    REQUIRE( allocator_t::items_allocated == allocator_t::items_freed );
    REQUIRE( allocator_t::allocations == allocator_t::frees );
}


void TestCopyAssignment() {
    using allocator_t = StaticCountingAllocator<std::allocator<move_support_tests::FooWithAssign>>;
    using vector_t = oneapi::tbb::concurrent_vector<move_support_tests::FooWithAssign, allocator_t>;
    StaticCountingAllocator<std::allocator<move_support_tests::FooWithAssign>> init_alloc;
    for( int dst_size=1; dst_size<=128; NextSize( dst_size ) ) {
        for( int src_size=2; src_size<=128; NextSize( src_size ) ) {
            vector_t u(move_support_tests::FooIterator(0), move_support_tests::FooIterator(src_size), init_alloc);
            for( int i=0; i<src_size; ++i )
                REQUIRE( u[i].bar()==i );
            vector_t v(dst_size, move_support_tests::FooWithAssign(), init_alloc);
            for( int i=0; i<dst_size; ++i ) {
                REQUIRE( v[i].state==move_support_tests::Foo::CopyInitialized );
                v[i].bar() = ~i;
            }
            REQUIRE( v != u );
            v.swap(u);
            CheckVector(u, dst_size, src_size);
            u.swap(v);
            // using assignment
            v = u;
            REQUIRE( v == u );
            u.clear();
            REQUIRE( u.size()==0 );
            REQUIRE( v.size()==std::size_t(src_size) );
            for( int i=0; i<src_size; ++i ){
                REQUIRE( v[i].bar()==i );
            }
            u.shrink_to_fit(); // deallocate unused memory
        }
    }
    REQUIRE( allocator_t::items_allocated == allocator_t::items_freed );
    REQUIRE( allocator_t::allocations == allocator_t::frees );
}

template<typename Vector, typename T>
void TestGrowToAtLeastWithSourceParameter(T const& src){
    static const std::size_t vector_size = 10;
    Vector v1(vector_size,src);
    Vector v2;
    v2.grow_to_at_least(vector_size,src);
    REQUIRE_MESSAGE(v1==v2,"grow_to_at_least(vector_size,src) did not properly initialize new elements ?");
}

void TestCapacity() {
    using allocator_t = StaticCountingAllocator<std::allocator<move_support_tests::Foo> /*TODO: oneapi::tbb::cache_aligned_allocator*/>;
    using vector_t = oneapi::tbb::concurrent_vector<move_support_tests::Foo, allocator_t>;
    allocator_t::init_counters();
    for( std::size_t old_size=0; old_size<=11000; old_size=(old_size<5 ? old_size+1 : 3*old_size) ) {
        for( std::size_t new_size=0; new_size<=11000; new_size=(new_size<5 ? new_size+1 : 3*new_size) ) {
            std::size_t count = move_support_tests::foo_count;
            {
                vector_t v; v.reserve(old_size);
                REQUIRE( v.capacity()>=old_size );
                v.reserve( new_size );
                REQUIRE( v.capacity()>=old_size );
                REQUIRE( v.capacity()>=new_size );
                REQUIRE( v.empty() );
                std::size_t fill_size = 2*new_size;
                for (std::size_t i=0; i<fill_size; ++i) {
                    REQUIRE( std::size_t(move_support_tests::foo_count)==count+i );
                    std::size_t j = v.grow_by(1) - v.begin();
                    REQUIRE( j==i );
                    v[j].bar() = int(~j);
                }
                vector_t copy_of_v(v); // should allocate first segment with same size as for shrink_to_fit()
                if(oneapi::tbb::detail::log2(/*reserved size*/old_size|1) > oneapi::tbb::detail::log2(fill_size|1) ){
                   REQUIRE( v.capacity() != copy_of_v.capacity() );
                }
                v.shrink_to_fit();
                REQUIRE( v.capacity() == copy_of_v.capacity() );
                CheckVector(v, new_size*2, old_size); // check vector correctness
                REQUIRE( v==copy_of_v ); // TODO: check also segments layout equality
            }
            REQUIRE( move_support_tests::foo_count==count );
        }
    }
    REQUIRE( allocator_t::items_allocated == allocator_t::items_freed );
    REQUIRE( allocator_t::allocations == allocator_t::frees );
}

template<typename c_vector>
std::size_t get_early_size(c_vector & v){
      return v.grow_by(0) - v.begin();
}

void verify_c_vector_size(std::size_t size, std::size_t capacity, std::size_t early_size){
    REQUIRE( size <= capacity );
    REQUIRE( early_size >= size );
}

template<typename c_vector_t>
void verify_c_vector_size(c_vector_t & c_v){
    verify_c_vector_size(c_v.size(), c_v.capacity(), get_early_size(c_v));
}

#if TBB_USE_EXCEPTIONS
void TestExceptions() {
    using allocator_t = StaticSharedCountingAllocator<std::allocator<move_support_tests::FooWithAssign>>;
    using vector_t = oneapi::tbb::concurrent_vector<move_support_tests::FooWithAssign, allocator_t>;

    enum methods {
        zero_method = 0,
        ctor_copy, ctor_size, assign_nt, assign_ir, reserve, compact,
        all_methods
    };
    REQUIRE( !move_support_tests::foo_count );

    try {
        vector_t src(move_support_tests::FooIterator(0), move_support_tests::FooIterator(N)); // original data

        for(int t = 0; t < 2; ++t) // exception type
        for(int m = zero_method+1; m < all_methods; ++m)
        {
            move_support_tests::track_foo_count<__LINE__> check_all_foo_destroyed_on_exit{};
            move_support_tests::track_allocator_memory<allocator_t> verify_no_leak_at_exit{};
            allocator_t::init_counters();
            if(t) move_support_tests::max_foo_count = move_support_tests::foo_count + N/4;
            else allocator_t::set_limits(N/4);
            vector_t victim;
            try {
                switch(m) {
                case ctor_copy: {
                        vector_t acopy(src);
                    } break; // auto destruction after exception is checked by ~Foo
                case ctor_size: {
                        vector_t sized(N);
                    } break; // auto destruction after exception is checked by ~Foo
                // Do not test assignment constructor due to reusing of same methods as below
                case assign_nt: {
                        victim.assign(N, move_support_tests::FooWithAssign());
                    } break;
                case assign_ir: {
                        victim.assign(move_support_tests::FooIterator(0), move_support_tests::FooIterator(N));
                    } break;
                case reserve: {
                        try {
                            victim.reserve(victim.max_size()+1);
                        } catch(std::length_error &) {
                        } catch(...) {
                            INFO("ERROR: unrecognized exception - known compiler issue\n");
                        }
                        victim.reserve(N);
                    } break;
                case compact: {
                        if(t) move_support_tests::max_foo_count = 0; else allocator_t::set_limits(); // reset limits
                        victim.reserve(2);
                        victim = src; // fragmented assignment
                        if(t) {
                            move_support_tests::max_foo_count = move_support_tests::foo_count + 10;
                        }
                        else {
                            allocator_t::set_limits(1); // block any allocation
                        }
                        victim.shrink_to_fit(); // should start defragmenting first segment
                    } break;
                default:;
                }
                if(!t || m != reserve) REQUIRE_MESSAGE(false, "should throw an exception");
            } catch(std::bad_alloc &e) {
                allocator_t::set_limits(); move_support_tests::max_foo_count = 0;
                std::size_t capacity = victim.capacity();
                std::size_t size = victim.size();

                std::size_t req_size = get_early_size(victim);

                verify_c_vector_size(size, capacity, req_size);

                switch(m) {
                case reserve:
                    if(t) REQUIRE(false);
                    utils_fallthrough;
                case assign_nt:
                case assign_ir:
                    if(!t) {
                        REQUIRE_MESSAGE(capacity < N/2, "unexpected capacity");
                        REQUIRE_MESSAGE(size == 0, "unexpected size");
                        break;
                    } else {
                        REQUIRE_MESSAGE(size == N, "unexpected size");
                        REQUIRE_MESSAGE(capacity >= N, "unexpected capacity");
                        int i;
                        for(i = 1; ; ++i)
                            if(!victim[i].zero_bar()) break;
                            else {
                                REQUIRE(victim[i].bar() == (m == assign_ir? i : move_support_tests::initial_bar));
                            }
                        for(; size_t(i) < size; ++i) {
                            REQUIRE(!victim[i].zero_bar());
                        }
                        REQUIRE(size_t(i) == size);
                        break;
                    }
                case compact:
                    REQUIRE_MESSAGE(capacity > 0, "unexpected capacity");
                    REQUIRE_MESSAGE(victim == src, "shrink_to_fit() is broken");
                    break;

                default:; // nothing to check here
                }
                INFO("Exception " << m << ": " << e.what() << "\t- ok\n");
            }
        }
    } catch(...) {
        REQUIRE_MESSAGE(false, "unexpected exception");
    }
}
#endif

void verify_c_vector_capacity_is_below(size_t capacity, size_t high){
    REQUIRE_MESSAGE(capacity > 0, "unexpected capacity");
    REQUIRE_MESSAGE(capacity < high, "unexpected capacity");
}

template<typename allocator_t>
void verify_vector_partially_copied(
        oneapi::tbb::concurrent_vector<move_support_tests::FooWithAssign, allocator_t> const& victim, size_t planned_victim_size,
        oneapi::tbb::concurrent_vector<move_support_tests::FooWithAssign, allocator_t> const& src,  bool is_memory_allocation_failure)
{
    if (is_memory_allocation_failure) { // allocator generated exception
        using vector_t = oneapi::tbb::concurrent_vector<move_support_tests::FooWithAssign, allocator_t>;
        REQUIRE_MESSAGE( victim == vector_t(src.begin(), src.begin() + victim.size(), src.get_allocator()), "failed to properly copy of source ?" );
    }else{
        REQUIRE_MESSAGE( std::equal(victim.begin(), victim.begin() + planned_victim_size, src.begin()), "failed to properly copy items before the exception?" );
        REQUIRE_MESSAGE( (std::all_of( victim.begin() + planned_victim_size, victim.end(), is_state_predicate<move_support_tests::Foo::ZeroInitialized>()) ), "failed to zero-initialize items left not constructed after the exception?" );
    }
}

template<typename vector_t>
void verify_last_segment_allocation_failed(vector_t const& victim){
    utils::suppress_unused_warning(victim);
    CHECK_THROWS_AS((victim.at(victim.size())), std::out_of_range);
}

template<typename vector_t>
void verify_copy_and_assign_from_produce_the_same(vector_t const& victim){
    //TODO: remove explicit copy of allocator when full support of C++11 allocator_traits in concurrent_vector is present
    vector_t copy_of_victim(victim, victim.get_allocator());
    REQUIRE_MESSAGE(copy_of_victim == victim, "copy doesn't match original");
    vector_t copy_of_victim2(10, victim[0], victim.get_allocator());
    copy_of_victim2 = victim;
    REQUIRE_MESSAGE(copy_of_victim == copy_of_victim2, "assignment doesn't match copying");
}

template<typename vector_t>
void verify_assignment_operator_throws_bad_last_alloc(vector_t & victim){
    vector_t copy_of_victim(victim, victim.get_allocator());
    //CHECK_THROWS_AS(victim = copy_of_victim, oneapi::tbb::bad_last_alloc); //TODO exceptions support
}

#if _MSC_VER
#pragma warning (push)
// Forcing value to bool 'true' or 'false'
#pragma warning (disable: 4800)
#endif //#if _MSC_VER

//TODO: split into two separate tests
//TODO: remove code duplication in exception safety tests
void test_ex_assign_operator(){
    //TODO: use __FUNCTION__ for test name
    using allocator_t = StaticCountingAllocator<std::allocator<move_support_tests::FooWithAssign>>;
    using vector_t = oneapi::tbb::concurrent_vector<move_support_tests::FooWithAssign, allocator_t>;

    move_support_tests::track_foo_count<__LINE__> check_all_foo_destroyed_on_exit{};
    move_support_tests::track_allocator_memory<allocator_t> verify_no_leak_at_exit{};

    vector_t src(move_support_tests::FooIterator(0), move_support_tests::FooIterator(N)); // original data

    const size_t planned_victim_size = N/4;

    for(int t = 0; t < 2; ++t) { // exception type
        vector_t victim;
        victim.reserve(2); // get fragmented assignment
        REQUIRE_THROWS_AS([&](){
            move_support_tests::LimitFooCountInScope foo_limit(move_support_tests::foo_count + planned_victim_size, t);
            move_support_tests::LimitAllocatedItemsInScope<allocator_t> allocator_limit(allocator_t::items_allocated + planned_victim_size, !t);

            victim = src; // fragmented assignment
        }(), const std::bad_alloc);

        verify_c_vector_size(victim);

        if(!t) {
            verify_c_vector_capacity_is_below(victim.capacity(), N);
        }

        verify_vector_partially_copied(victim, planned_victim_size, src, !t);
        verify_last_segment_allocation_failed(victim);
        verify_copy_and_assign_from_produce_the_same(victim);
        verify_assignment_operator_throws_bad_last_alloc(victim); //TODO exceptions support
    }
}

#if _MSC_VER
#pragma warning (pop)
#endif

template<typename T>
void AssertSameType( const T& /*x*/, const T& /*y*/ ) {}

struct test_grow_by {
    template<typename container_type, typename element_type>
    static void test( std::initializer_list<element_type> const& il, container_type const& expected ) {
        container_type vd;
        vd.grow_by( il );
        REQUIRE_MESSAGE( vd == expected, "grow_by with an initializer list failed" );
    }
};

template<typename Iterator, typename T>
void TestIteratorTraits() {
    AssertSameType( static_cast<typename Iterator::difference_type*>(nullptr), static_cast<std::ptrdiff_t*>(nullptr) );
    AssertSameType( static_cast<typename Iterator::value_type*>(nullptr), static_cast<T*>(nullptr) );
    AssertSameType( static_cast<typename Iterator::pointer*>(nullptr), static_cast<T**>(nullptr) );
    AssertSameType( static_cast<typename Iterator::iterator_category*>(nullptr), static_cast<std::random_access_iterator_tag*>(nullptr) );
    T x;
    typename Iterator::reference xr = x;
    typename Iterator::pointer xp = &x;
    REQUIRE( &xr==xp );
}

void TestInitList() {
    using namespace initializer_list_support_tests;
    test_initializer_list_support<oneapi::tbb::concurrent_vector<char>, test_grow_by>( { 1, 2, 3, 4, 5 } );
    test_initializer_list_support<oneapi::tbb::concurrent_vector<int>, test_grow_by>( {} );
}

namespace TestMoveInShrinkToFitHelpers {
    struct dummy : StateTrackable<>{
        int i;
        dummy(int an_i) noexcept : StateTrackable<>(0), i(an_i) {}

        friend bool operator== (const dummy &lhs, const dummy &rhs){ return lhs.i == rhs.i; }
    };
}

void TestSerialMoveInShrinkToFit(){
    using TestMoveInShrinkToFitHelpers::dummy;

    static_assert(std::is_nothrow_move_constructible<dummy>::value,"incorrect test setup or broken configuration?");
    {
        dummy src(0);
        REQUIRE_MESSAGE(is_state<StateTrackableBase::MoveInitialized>(dummy(std::move_if_noexcept(src))),"broken configuration ?");
    }
    static const std::size_t sequence_size = 15;
    using c_vector_t = oneapi::tbb::concurrent_vector<dummy>;
    std::vector<dummy> source(sequence_size, 0);
    std::generate_n(source.begin(), source.size(), std::rand);

    c_vector_t c_vector;
    c_vector.reserve(1); //make it fragmented

    c_vector.assign(source.begin(), source.end());
    move_support_tests::MemoryLocations c_vector_before_shrink(c_vector);
    c_vector.shrink_to_fit();

    REQUIRE_MESSAGE(c_vector_before_shrink.content_location_changed(c_vector), "incorrect test setup? shrink_to_fit should cause moving elements to other memory locations while it is not");
    REQUIRE_MESSAGE((std::all_of(c_vector.begin(), c_vector.end(), is_state_predicate<StateTrackableBase::MoveInitialized>())), "container did not move construct some elements?");
    REQUIRE((c_vector == c_vector_t(source.begin(),source.end())));
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
    using container_type = oneapi::tbb::concurrent_vector<T, Allocator>;

    template <typename T>
    using container_value_type = T;

    using init_iterator_type = move_support_tests::FooIterator;
    template<typename element_type, typename allocator_type>
    struct apply{
        using type = oneapi::tbb::concurrent_vector<element_type,  allocator_type >;
    };

    enum{ expected_number_of_items_to_allocate_for_steal_move = 0 };

    template<typename element_type, typename allocator_type, typename iterator>
    static bool equal(oneapi::tbb::concurrent_vector<element_type, allocator_type > const& c, iterator begin, iterator end){
        bool equal_sizes = (std::size_t)std::distance(begin, end) == c.size();
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

namespace test_grow_to_at_least_helpers {
    template<typename MyVector >
    class GrowToAtLeast {
        using const_reference = typename MyVector::const_reference;

        const bool my_use_two_args_form ;
        MyVector& my_vector;
        const_reference my_init_from;
    public:
        void operator()( const oneapi::tbb::blocked_range<std::size_t>& range ) const {
            for( std::size_t i=range.begin(); i!=range.end(); ++i ) {
                std::size_t n = my_vector.size();
                std::size_t req = (i % (2*n+1))+1;

                typename MyVector::iterator p;
                move_support_tests::Foo::State desired_state;
                if (my_use_two_args_form){
                    p = my_vector.grow_to_at_least(req,my_init_from);
                    desired_state = move_support_tests::Foo::CopyInitialized;
                }else{
                    p = my_vector.grow_to_at_least(req);
                    desired_state = move_support_tests::Foo::DefaultInitialized;
                }
                if( p-my_vector.begin() < typename MyVector::difference_type(req) )
                    CHECK((p->state == desired_state || p->state == move_support_tests::Foo::ZeroInitialized));
                CHECK(my_vector.size() >= req);
            }
        }
        GrowToAtLeast(bool use_two_args_form, MyVector& vector, const_reference init_from )
            : my_use_two_args_form(use_two_args_form), my_vector(vector), my_init_from(init_from) {}
    };
}

template<bool use_two_arg_form>
void TestConcurrentGrowToAtLeastImpl() {
    using namespace test_grow_to_at_least_helpers;
    using MyAllocator = StaticCountingAllocator<std::allocator<move_support_tests::Foo>>;
    using MyVector = oneapi::tbb::concurrent_vector<move_support_tests::Foo, MyAllocator>;
    move_support_tests::Foo copy_from;
    MyAllocator::init_counters();
    MyVector v(2, move_support_tests::Foo(), MyAllocator());
    for (std::size_t s=1; s<1000; s*=10) {
        oneapi::tbb::parallel_for(oneapi::tbb::blocked_range<std::size_t>(0, 10000*s, s), GrowToAtLeast<MyVector>(use_two_arg_form, v, copy_from), oneapi::tbb::simple_partitioner());
    }

    v.clear();
    v.shrink_to_fit();
    std::size_t items_allocated = v.get_allocator().items_allocated,
           items_freed = v.get_allocator().items_freed;
    std::size_t allocations = v.get_allocator().allocations,
           frees = v.get_allocator().frees;
    REQUIRE( items_allocated == items_freed );
    REQUIRE( allocations == frees );
}

struct AssignElement {
    using iterator = oneapi::tbb::concurrent_vector<int>::range_type::iterator;
    iterator base;
    void operator()( const oneapi::tbb::concurrent_vector<int>::range_type& range ) const {
        for (iterator i = range.begin(); i != range.end(); ++i) {
            if (*i != 0) {
                REPORT("ERROR for v[%ld]\n", long(i - base));
            }
            *i = int(i-base);
        }
    }
    AssignElement( iterator base_ ) : base(base_) {}
};

struct CheckElement {
    using iterator = oneapi::tbb::concurrent_vector<int>::const_range_type::iterator;
    iterator base;
    void operator()( const oneapi::tbb::concurrent_vector<int>::const_range_type& range ) const {
        for (iterator i = range.begin(); i != range.end(); ++i) {
            if (*i != int(i-base)) {
                REPORT("ERROR for v[%ld]\n", long(i-base));
            }
        }
    }
    CheckElement( iterator base_ ) : base(base_) {}
};

// Test parallel access by iterators
void TestParallelFor( std::size_t nthread ) {
    using vector_type = oneapi::tbb::concurrent_vector<int>;
    vector_type v;
    v.resize(N);
    oneapi::tbb::tick_count t0 = oneapi::tbb::tick_count::now();
    INFO("Calling parallel_for with " << nthread << " threads");
    oneapi::tbb::parallel_for(v.range(10000), AssignElement(v.begin()));
    oneapi::tbb::tick_count t1 = oneapi::tbb::tick_count::now();
    const vector_type& u = v;
    oneapi::tbb::parallel_for(u.range(10000), CheckElement(u.begin()));
    oneapi::tbb::tick_count t2 = oneapi::tbb::tick_count::now();
    INFO("Time for parallel_for: assign time = " << (t1 - t0).seconds() <<
        " , check time = " << (t2 - t1).seconds());
    for (int i = 0; std::size_t(i) < v.size(); ++i) {
        if (v[i] != i) {
            REPORT("ERROR for v[%ld]\n", i);
        }
    }
}


struct grain_map {
    enum grow_method_enum {
        grow_by_range = 1,
        grow_by_default,
        grow_by_copy,
        grow_by_init_list,
        push_back,
        push_back_move,
        emplace_back,
        last_method
    };

    struct range_part {
        std::size_t number_of_parts;
        grain_map::grow_method_enum method;
        bool distribute;
        move_support_tests::Foo::State expected_element_state;
    };

    const std::vector<range_part> distributed;
    const std::vector<range_part> batched;
    const std::size_t total_number_of_parts;

    grain_map(const range_part* begin, const range_part* end)
    : distributed(separate(begin,end, &distributed::is_not))
    , batched(separate(begin,end, &distributed::is_yes))
    , total_number_of_parts(std::accumulate(begin, end, (std::size_t)0, &sum_number_of_parts::sum))
    {}

private:
    struct sum_number_of_parts{
        static std::size_t sum(std::size_t accumulator, grain_map::range_part const& rp){ return accumulator + rp.number_of_parts;}
    };

    template <typename functor_t>
    static std::vector<range_part> separate(const range_part* begin, const range_part* end, functor_t f){
        std::vector<range_part> part;
        part.reserve(std::distance(begin,end));
        //copy all that false==f(*it)
        std::remove_copy_if(begin, end, std::back_inserter(part), f);

        return part;
    }

    struct distributed {
        static bool is_not(range_part const& rp){ return !rp.distribute;}
        static bool is_yes(range_part const& rp){ return rp.distribute;}
    };
};


//! Test concurrent invocations of method concurrent_vector::grow_by
template<typename MyVector>
class GrowBy {
    MyVector& my_vector;
    const grain_map& my_grain_map;
    std::size_t my_part_weight;
public:
    void operator()( const oneapi::tbb::blocked_range<std::size_t>& range ) const {
        CHECK(range.begin() < range.end());

        std::size_t current_adding_index_in_cvector = range.begin();

        for (std::size_t index = 0; index < my_grain_map.batched.size(); ++index){
            const grain_map::range_part& batch_part = my_grain_map.batched[index];
            const std::size_t number_of_items_to_add = batch_part.number_of_parts * my_part_weight;
            const std::size_t end = current_adding_index_in_cvector + number_of_items_to_add;

            switch(batch_part.method){
            case grain_map::grow_by_range : {
                    my_vector.grow_by(move_support_tests::FooIterator(current_adding_index_in_cvector), move_support_tests::FooIterator(end));
                } break;
            case grain_map::grow_by_default : {
                    typename MyVector::iterator const s = my_vector.grow_by(number_of_items_to_add);
                    for (std::size_t k = 0; k < number_of_items_to_add; ++k) {
                        s[k].bar() = current_adding_index_in_cvector + k;
                    }
                } break;
            case grain_map::grow_by_init_list : {
                    move_support_tests::FooIterator curr(current_adding_index_in_cvector);
                    for (std::size_t k = 0; k < number_of_items_to_add; ++k) {
                        if (k + 4 < number_of_items_to_add) {
                            my_vector.grow_by( { *curr++, *curr++, *curr++, *curr++, *curr++ } );
                            k += 4;
                        } else {
                            my_vector.grow_by( { *curr++ } );
                        }
                    }
                    CHECK(curr == move_support_tests::FooIterator(end));
                } break;
            default : { REQUIRE_MESSAGE(false, "using unimplemented method of batch add in ConcurrentGrow test.");} break;
            };

            current_adding_index_in_cvector = end;
        }

        std::vector<std::size_t> items_left_to_add(my_grain_map.distributed.size());
        for (std::size_t i=0; i < my_grain_map.distributed.size(); ++i) {
            items_left_to_add[i] = my_grain_map.distributed[i].number_of_parts * my_part_weight;
        }

        for (;current_adding_index_in_cvector < range.end(); ++current_adding_index_in_cvector) {
            std::size_t method_index = current_adding_index_in_cvector % my_grain_map.distributed.size();

            if (!items_left_to_add[method_index]) {
                struct not_zero{
                    static bool is(std::size_t items_to_add){ return items_to_add != 0;}
                };
                method_index = std::distance(items_left_to_add.begin(), std::find_if(items_left_to_add.begin(), items_left_to_add.end(), &not_zero::is));
                REQUIRE_MESSAGE(method_index < my_grain_map.distributed.size(), "incorrect test setup - wrong expected distribution: left free space but no elements to add?");
            };

            REQUIRE_MESSAGE(items_left_to_add[method_index], "logic error ?");
            const grain_map::range_part& distributed_part = my_grain_map.distributed[method_index];

            typename MyVector::iterator r;
            typename MyVector::value_type source;
            source.bar() = current_adding_index_in_cvector;

            switch(distributed_part.method){
            case grain_map::grow_by_default : {
                    (r = my_vector.grow_by(1))->bar() = current_adding_index_in_cvector;
                } break;
            case grain_map::grow_by_copy : {
                    r = my_vector.grow_by(1, source);
                } break;
            case grain_map::push_back : {
                    r = my_vector.push_back(source);
                } break;
            case grain_map::push_back_move : {
                    r = my_vector.push_back(std::move(source));
                } break;
            case grain_map::emplace_back : {
                    r = my_vector.emplace_back(current_adding_index_in_cvector);
                } break;

            default : { REQUIRE_MESSAGE(false, "using unimplemented method of batch add in ConcurrentGrow test.");} break;
            };

            CHECK(static_cast<std::size_t>(r->bar()) == current_adding_index_in_cvector);
            }

        }

    GrowBy( MyVector& vector, const grain_map& m, std::size_t part_weight )
    : my_vector(vector), my_grain_map(m), my_part_weight(part_weight)
    {}
};

//! Test concurrent invocations of grow methods
void TestConcurrentGrowBy() {

    const grain_map::range_part concurrent_grow_single_range_map [] = {
    //  number_of_parts,         method,             distribute,   expected_element_state
            {3,           grain_map::grow_by_range,     false,   move_support_tests::Foo::MoveInitialized},

            {1,           grain_map::grow_by_init_list, false,   move_support_tests::Foo::CopyInitialized},

            {2,           grain_map::grow_by_default,   false,   move_support_tests::Foo::DefaultInitialized},
            {1,           grain_map::grow_by_default,   true,    move_support_tests::Foo::DefaultInitialized},
            {1,           grain_map::grow_by_copy,      true,    move_support_tests::Foo::CopyInitialized},
            {1,           grain_map::push_back,         true,    move_support_tests::Foo::CopyInitialized},

            {1,           grain_map::push_back_move,    true,    move_support_tests::Foo::MoveInitialized},

            {1,           grain_map::emplace_back,      true,    move_support_tests::Foo::DirectInitialized},

    };

    using MyAllocator = StaticCountingAllocator<std::allocator<move_support_tests::Foo> >;
    using MyVector = oneapi::tbb::concurrent_vector<move_support_tests::Foo, MyAllocator>;

    MyAllocator::init_counters();
    {
        grain_map m(concurrent_grow_single_range_map, end(concurrent_grow_single_range_map));

        static const std::size_t desired_grain_size = 100;

        static const std::size_t part_weight = desired_grain_size / m.total_number_of_parts;
        static const std::size_t grain_size = part_weight * m.total_number_of_parts;
        static const std::size_t number_of_grains = 8; //this should be (power of two) in order to get minimal ranges equal to grain_size
        static const std::size_t range_size = grain_size * number_of_grains;

        MyAllocator a;
        MyVector v(a);
        oneapi::tbb::parallel_for(oneapi::tbb::blocked_range<std::size_t>(0, range_size, grain_size), GrowBy<MyVector>(v, m, part_weight), oneapi::tbb::simple_partitioner());

        REQUIRE( v.size() == std::size_t(range_size) );

        // Verify that v is a permutation of 0..m
        size_t direct_inits = 0, def_inits = 0, copy_inits = 0, move_inits = 0;
        std::vector<bool> found(range_size, 0);
        for( std::size_t i=0; i<range_size; ++i ) {
            if( v[i].state == move_support_tests::Foo::DefaultInitialized ) ++def_inits;
            else if( v[i].state == move_support_tests::Foo::DirectInitialized ) ++direct_inits;
            else if( v[i].state == move_support_tests::Foo::CopyInitialized ) ++copy_inits;
            else if( v[i].state == move_support_tests::Foo::MoveInitialized ) ++move_inits;
            else {
                REQUIRE_MESSAGE( false, "v[i] seems not initialized");
            }
            intptr_t index = v[i].bar();
            REQUIRE( !found[index] );
            found[index] = true;
        }

        std::size_t expected_direct_inits = 0, expected_def_inits = 0, expected_copy_inits = 0, expected_move_inits = 0;
        for (std::size_t i=0; i < utils::array_length(concurrent_grow_single_range_map); ++i){
            const grain_map::range_part& rp =concurrent_grow_single_range_map[i];
            switch (rp.expected_element_state){
            case move_support_tests::Foo::DefaultInitialized: { expected_def_inits += rp.number_of_parts ; } break;
            case move_support_tests::Foo::DirectInitialized:  { expected_direct_inits += rp.number_of_parts ;} break;
            case move_support_tests::Foo::MoveInitialized:    { expected_move_inits += rp.number_of_parts ;} break;
            case move_support_tests::Foo::CopyInitialized:    { expected_copy_inits += rp.number_of_parts ;} break;
            default: {REQUIRE_MESSAGE(false, "unexpected expected state");}break;
            };
        }

        expected_def_inits    *= part_weight * number_of_grains;
        expected_move_inits   *= part_weight * number_of_grains;
        expected_copy_inits   *= part_weight * number_of_grains;
        expected_direct_inits *= part_weight * number_of_grains;

        REQUIRE( def_inits == expected_def_inits );
        REQUIRE( copy_inits == expected_copy_inits );
        REQUIRE( move_inits == expected_move_inits );
        REQUIRE( direct_inits == expected_direct_inits );
    }
    //TODO: factor this into separate thing, as it seems to used in big number of tests
    std::size_t items_allocated = MyAllocator::items_allocated,
           items_freed = MyAllocator::items_freed;
    std::size_t allocations = MyAllocator::allocations,
           frees = MyAllocator::frees;
    REQUIRE(items_allocated == items_freed);
    REQUIRE(allocations == frees);
}

void TestComparison() {
    std::string str[3];
    str[0] = "abc";
    str[1].assign("cba");
    str[2].assign("abc"); // same as 0th
    oneapi::tbb::concurrent_vector<char> var[3];
    var[0].assign(str[0].begin(), str[0].end());
    var[1].assign(str[0].rbegin(), str[0].rend());
    var[2].assign(var[1].rbegin(), var[1].rend()); // same as 0th
    for (int i = 0; i < 3; ++i) {
        for (int j = 0; j < 3; ++j) {
            REQUIRE( (var[i] == var[j]) == (str[i] == str[j]) );
            REQUIRE( (var[i] != var[j]) == (str[i] != str[j]) );
            REQUIRE( (var[i] < var[j]) == (str[i] < str[j]) );
            REQUIRE( (var[i] > var[j]) == (str[i] > str[j]) );
            REQUIRE( (var[i] <= var[j]) == (str[i] <= str[j]) );
            REQUIRE( (var[i] >= var[j]) == (str[i] >= str[j]) );
        }
    }
}

#if TBB_USE_EXCEPTIONS
void test_ex_move_assignment_memory_failure() {
    using fixture_type = move_support_tests::DefaultStatefulFixtureHelper<c_vector_type, /*POCMA = */std::false_type>::type;
    using arena_allocator_fixture_type = move_support_tests::ArenaAllocatorFixture<move_support_tests::FooWithAssign, /*POCMA = */std::false_type>;
    using allocator_type = fixture_type::allocator_type;
    using vector_type = fixture_type::container_type;

    fixture_type fixture;
    arena_allocator_fixture_type arena_allocator_fixture(4 * fixture.container_size);

    const std::size_t allocation_limit = fixture.container_size/4;

    vector_type victim(arena_allocator_fixture.allocator);
    victim.reserve(2); // for fragmented assignment

    REQUIRE_THROWS_AS(
        [&]() {
            move_support_tests::LimitAllocatedItemsInScope<allocator_type> allocator_limit(allocator_type::items_allocated + allocation_limit);
            victim = std::move(fixture.source); // fragmented assignment
        }(),
        std::bad_alloc
    );

    verify_c_vector_size(victim);
    verify_c_vector_capacity_is_below(victim.capacity(), allocation_limit + 2);
    fixture.verify_part_of_content_deep_moved(victim, victim.size());

    verify_last_segment_allocation_failed(victim);
    verify_copy_and_assign_from_produce_the_same(victim);
    verify_assignment_operator_throws_bad_last_alloc(victim);
}

void test_ex_move_assignment_element_ctor_exception(){
    using fixture_type = move_support_tests::DefaultStatefulFixtureHelper<c_vector_type, std::false_type>::type;
    using arena_allocator_fixture_type = move_support_tests::ArenaAllocatorFixture<move_support_tests::FooWithAssign, std::false_type>;
    using vector_type = fixture_type::container_type;

    fixture_type fixture;
    const size_t planned_victim_size = fixture.container_size/4;
    arena_allocator_fixture_type arena_allocator_fixture(4 * fixture.container_size);

    vector_type victim(arena_allocator_fixture.allocator);
    victim.reserve(2); // get fragmented assignment

    REQUIRE_THROWS_AS(
        [&](){
            move_support_tests::LimitFooCountInScope foo_limit(move_support_tests::foo_count + planned_victim_size);
            victim = std::move(fixture.source); // fragmented assignment
        }(),
        std::bad_alloc
    );

    verify_c_vector_size(victim);

    fixture.verify_part_of_content_deep_moved(victim, planned_victim_size);

    verify_last_segment_allocation_failed(victim);
    verify_copy_and_assign_from_produce_the_same(victim);
    verify_assignment_operator_throws_bad_last_alloc(victim);
}

void test_ex_move_assignment() {
    test_ex_move_assignment_memory_failure();
    test_ex_move_assignment_element_ctor_exception();
}
#endif

template <typename Type, typename Allocator>
class test_grow_by_and_resize {
    oneapi::tbb::concurrent_vector<Type, Allocator> &my_c;
public:
    test_grow_by_and_resize( oneapi::tbb::concurrent_vector<Type, Allocator> &c ) : my_c(c) {}
    void operator()() const {
        const typename oneapi::tbb::concurrent_vector<Type, Allocator>::size_type sz = my_c.size();
        my_c.grow_by( 5 );
        REQUIRE( my_c.size() == sz + 5 );
        my_c.resize( sz );
        REQUIRE( my_c.size() == sz );
    }
};

namespace push_back_exception_safety_helpers {
    //TODO: remove code duplication with emplace_helpers::wrapper_type
    struct throwing_foo:move_support_tests::Foo {
        int value1;
        int value2;
        explicit throwing_foo(int v1, int v2) : value1 (v1), value2(v2) {}
    };

    template< typename foo_t = throwing_foo>
    struct fixture {
        using vector_t = oneapi::tbb::concurrent_vector<foo_t, std::allocator<foo_t> >;
        vector_t v;

        void test( void(*p_test)(vector_t&)){
            utils::suppress_unused_warning(p_test);
            move_support_tests::track_foo_count<__LINE__> verify_no_foo_leaked_during_exception{};
            utils::suppress_unused_warning(verify_no_foo_leaked_during_exception);
            REQUIRE_MESSAGE(v.empty(),"incorrect test setup?" );
            REQUIRE_THROWS_AS(p_test(v), move_support_tests::FooException);
            REQUIRE_MESSAGE(is_state<move_support_tests::Foo::ZeroInitialized>(v[0]),"incorrectly filled item during exception in emplace_back?");
        }
    };
}

void TestPushBackMoveExceptionSafety() {
    using fixture_t = push_back_exception_safety_helpers::fixture<move_support_tests::Foo>;
    fixture_t t;

    move_support_tests::LimitFooCountInScope foo_limit(move_support_tests::foo_count + 1);

    struct test {
        static void test_move_push_back(fixture_t::vector_t& v) {
            move_support_tests::Foo f;
            v.push_back(std::move(f));
        }
    };
    t.test(&test::test_move_push_back);
}

void TestEmplaceBackExceptionSafety(){
    using fixture_t = push_back_exception_safety_helpers::fixture<>;
    fixture_t t;

    move_support_tests::Foo dummy; //make FooCount non zero;
    utils::suppress_unused_warning(dummy);
    move_support_tests::LimitFooCountInScope foo_limit(move_support_tests::foo_count);

    struct test {
        static void test_emplace(fixture_t::vector_t& v) {
            v.emplace_back(1,2);
        }
    };
    t.test(&test::test_emplace);
}

namespace move_semantics_helpers {
    struct move_only_type {
        const int* my_pointer;
        move_only_type(move_only_type && other): my_pointer(other.my_pointer){other.my_pointer=nullptr;}
        explicit move_only_type(const int* value): my_pointer(value) {}
    };
}

void TestPushBackMoveOnlyContainer(){
    using namespace move_semantics_helpers;
    using vector_t = oneapi::tbb::concurrent_vector<move_only_type >;
    vector_t v;
    static const int magic_number = 7;
    move_only_type src(&magic_number);
    v.push_back(std::move(src));
    REQUIRE_MESSAGE((v[0].my_pointer == &magic_number),"item was incorrectly moved during push_back?");
    REQUIRE_MESSAGE(src.my_pointer == nullptr,"item was incorrectly moved during push_back?");
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
template <template <typename...> typename TVector>
void TestDeductionGuides() {
    using ComplexType = const std::string*;
    std::vector<ComplexType> v;
    std::string s = "s";
    auto l = {ComplexType(&s), ComplexType(&s)};

    // check TVector(InputIterator, InputIterator)
    TVector v1(v.begin(), v.end());
    static_assert(std::is_same<decltype(v1), TVector<ComplexType>>::value);

    // check TVector(InputIterator, InputIterator, Alocator)
    TVector v2(v.begin(), v.end(), std::allocator<ComplexType>());
    static_assert(std::is_same<decltype(v2),
       TVector<ComplexType, std::allocator<ComplexType>>>::value);

    // check TVector(std::initializer_list<T>)
    TVector v3(l);
    static_assert(std::is_same<decltype(v3),
        TVector<ComplexType>>::value);

    // check TVector(std::initializer_list, Alocator)
    TVector v4(l, std::allocator<ComplexType>());
    static_assert(std::is_same<decltype(v4), TVector<ComplexType, std::allocator<ComplexType>>>::value);

    // check TVector(TVector&)
    TVector v5(v1);
    static_assert(std::is_same<decltype(v5), TVector<ComplexType>>::value);

    // check TVector(TVector&, Allocator)
    TVector v6(v5, oneapi::tbb::cache_aligned_allocator<ComplexType>());
    static_assert(std::is_same<decltype(v6), TVector<ComplexType, oneapi::tbb::cache_aligned_allocator<ComplexType>>>::value);

    // check TVector(TVector&&)
    TVector v7(std::move(v1));
    static_assert(std::is_same<decltype(v7), decltype(v1)>::value);

    // check TVector(TVector&&, Allocator)
    TVector v8(std::move(v5), oneapi::tbb::cache_aligned_allocator<ComplexType>());
    static_assert(std::is_same<decltype(v8), TVector<ComplexType, oneapi::tbb::cache_aligned_allocator<ComplexType>>>::value);

}
#endif

template <template <typename... > class ContainerType>
void test_member_types() {
    using default_container_type = ContainerType<int>;

    static_assert(std::is_same<typename default_container_type::allocator_type,
                               oneapi::tbb::cache_aligned_allocator<int>>::value,
                  "Incorrect default template allocator");


    using test_allocator_type = oneapi::tbb::cache_aligned_allocator<int>;
    using container_type = ContainerType<int, test_allocator_type>;

    static_assert(std::is_same<typename container_type::value_type, int>::value,
                  "Incorrect container value_type member type");

    static_assert(std::is_unsigned<typename container_type::size_type>::value,
                  "Incorrect container size_type member type");
    static_assert(std::is_signed<typename container_type::difference_type>::value,
                  "Incorrect container difference_type member type");

    using value_type = typename container_type::value_type;
    static_assert(std::is_same<typename container_type::reference, value_type&>::value,
                  "Incorrect container reference member type");
    static_assert(std::is_same<typename container_type::const_reference, const value_type&>::value,
                  "Incorrect container const_reference member type");
    using allocator_type = typename container_type::allocator_type;
    static_assert(std::is_same<typename container_type::pointer, typename std::allocator_traits<allocator_type>::pointer>::value,
                  "Incorrect container pointer member type");
    static_assert(std::is_same<typename container_type::const_pointer, typename std::allocator_traits<allocator_type>::const_pointer>::value,
                  "Incorrect container const_pointer member type");

    static_assert(utils::is_random_access_iterator<typename container_type::iterator>::value,
                  "Incorrect container iterator member type");
    static_assert(!std::is_const<typename container_type::iterator::value_type>::value,
                  "Incorrect container iterator member type");
    static_assert(utils::is_random_access_iterator<typename container_type::const_iterator>::value,
                  "Incorrect container const_iterator member type");
    static_assert(std::is_const<typename container_type::const_iterator::value_type>::value,
                  "Incorrect container iterator member type");
}

void TestConcurrentGrowToAtLeast() {
    TestConcurrentGrowToAtLeastImpl<false>();
    TestConcurrentGrowToAtLeastImpl<true>();
}

template <typename Vector>
void test_comparisons_basic() {
    using comparisons_testing::testEqualityAndLessComparisons;
    Vector v1, v2;
    testEqualityAndLessComparisons</*ExpectEqual = */true, /*ExpectLess = */false>(v1, v2);

    v1.emplace_back(1);
    testEqualityAndLessComparisons</*ExpectEqual = */false, /*ExpectLess = */false>(v1, v2);

    v2.emplace_back(1);
    testEqualityAndLessComparisons</*ExpectEqual = */true, /*ExpectLess = */false>(v1, v2);

    v2.emplace_back(2);
    testEqualityAndLessComparisons</*ExpectEqual = */false, /*ExpectLess = */true>(v1, v2);

    v1.clear();
    v2.clear();
    testEqualityAndLessComparisons</*ExpectEqual = */true, /*ExpectLess = */false>(v1, v2);
}

template <typename TwoWayComparableVectorType>
void test_two_way_comparable_vector() {
    TwoWayComparableVectorType v1, v2;
    v1.emplace_back(1);
    v2.emplace_back(1);
    comparisons_testing::TwoWayComparable::reset();
    REQUIRE_MESSAGE(!(v1 < v2), "Incorrect operator < result");
    comparisons_testing::check_two_way_comparison();
    REQUIRE_MESSAGE(!(v1 > v2), "Incorrect operator > result");
    comparisons_testing::check_two_way_comparison();
    REQUIRE_MESSAGE(v1 <= v2, "Incorrect operator <= result");
    comparisons_testing::check_two_way_comparison();
    REQUIRE_MESSAGE(v1 >= v2, "Incorrect operator >= result");
    comparisons_testing::check_two_way_comparison();
}

#if __TBB_TEST_CPP20_COMPARISONS
template <typename ThreeWayComparableVectorType>
void test_three_way_comparable_vector() {
    ThreeWayComparableVectorType v1, v2;
    v1.emplace_back(1);
    v2.emplace_back(1);
    comparisons_testing::ThreeWayComparable::reset();
    REQUIRE_MESSAGE(!(v1 <=> v2 < 0), "Incorrect operator<=> result");
    comparisons_testing::check_three_way_comparison();

    REQUIRE_MESSAGE(!(v1 < v2), "Incorrect operator< result");
    comparisons_testing::check_three_way_comparison();

    REQUIRE_MESSAGE(!(v1 > v2), "Incorrect operator> result");
    comparisons_testing::check_three_way_comparison();

    REQUIRE_MESSAGE(v1 <= v2, "Incorrect operator>= result");
    comparisons_testing::check_three_way_comparison();

    REQUIRE_MESSAGE(v1 >= v2, "Incorrect operator>= result");
    comparisons_testing::check_three_way_comparison();
}
#endif // __TBB_TEST_CPP20_COMPARISONS

void TestVectorComparisons() {
    using integral_vector = oneapi::tbb::concurrent_vector<int>;
    using two_way_comparable_vector = oneapi::tbb::concurrent_vector<comparisons_testing::TwoWayComparable>;

    test_comparisons_basic<integral_vector>();
    test_comparisons_basic<two_way_comparable_vector>();
    test_two_way_comparable_vector<two_way_comparable_vector>();

#if __TBB_TEST_CPP20_COMPARISONS
    using two_way_less_only_vector = oneapi::tbb::concurrent_vector<comparisons_testing::LessComparableOnly>;
    using three_way_only_vector = oneapi::tbb::concurrent_vector<comparisons_testing::ThreeWayComparableOnly>;
    using three_way_comparable_vector = oneapi::tbb::concurrent_vector<comparisons_testing::ThreeWayComparable>;

    test_comparisons_basic<two_way_less_only_vector>();
    test_comparisons_basic<three_way_only_vector>();
    test_comparisons_basic<three_way_comparable_vector>();
    test_three_way_comparable_vector<three_way_comparable_vector>();
#endif // __TBB_CPP20_COMPARISONS_PRESENT && __TBB_CPP20_CONVEPTS_PRESENT
}

template <bool ExpectEqual, bool ExpectLess, typename Iterator>
void DoVectorIteratorComparisons( const Iterator& lhs, const Iterator& rhs ) {
    // TODO: replace with testEqualityAndLessComparisons after adding <=> operator for concurrent_vector iterator
    using namespace comparisons_testing;
    testEqualityComparisons<ExpectEqual>(lhs, rhs);
    testTwoWayComparisons<ExpectEqual, ExpectLess>(lhs, rhs);
}

template <typename Iterator, typename VectorType>
void TestVectorIteratorComparisonsBasic( VectorType& vec ) {
    REQUIRE_MESSAGE(!vec.empty(), "Incorrect test setup");
    Iterator it1, it2;
    DoVectorIteratorComparisons</*ExpectEqual = */true, /*ExpectLess = */false>(it1, it2);
    it1 = vec.begin();
    it2 = vec.begin();
    DoVectorIteratorComparisons</*ExpectEqual = */true, /*ExpectLess = */false>(it1, it2);
    it2 = std::prev(vec.end());
    DoVectorIteratorComparisons</*ExpectEqual = */false, /*ExpectLess = */true>(it1, it2);
}

void TestVectorIteratorComparisons() {
    using vector_type = oneapi::tbb::concurrent_vector<int>;
    vector_type vec = {1, 2, 3, 4, 5};
    TestVectorIteratorComparisonsBasic<typename vector_type::iterator>(vec);
    const vector_type& cvec = vec;
    TestVectorIteratorComparisonsBasic<typename vector_type::const_iterator>(cvec);
}

//! Test type matching
//! \brief \ref interface \ref requirement
TEST_CASE("test type matching") {
    test_member_types<oneapi::tbb::concurrent_vector>();
}

//! Test sequential access to elements
//! \brief \ref interface \ref requirement
TEST_CASE("testing sequential for") {
    TestSequentialFor<move_support_tests::FooWithAssign> ();
}

//! Test of assign, grow, copying with various sizes
//! \brief \ref interface \ref requirement
TEST_CASE("testing resize and copy"){
    TestResizeAndCopy();
}

//! Test the assignment operator and swap
//! \brief \ref interface \ref requirement
TEST_CASE("testing copy assignment"){
    TestCopyAssignment();
}

//! Testing grow_to_at_least operations
//! \brief \ref interface
TEST_CASE("testing grow_to_at_least with source parameter"){
    TestGrowToAtLeastWithSourceParameter<oneapi::tbb::concurrent_vector<int>>(12345);
}

//! Test of capacity, reserve, and shrink_to_fit
//! \brief \ref interface \ref requirement
TEST_CASE("testing capacity"){
   TestCapacity();
}

#if TBB_USE_EXCEPTIONS
//! Test exceptions
//! \brief \ref requirement
TEST_CASE("testing exceptions"){
    TestExceptions();
}

//! Test of push_back move exception safety
//! \brief \ref requirement
TEST_CASE("testing push_back move exception safety"){
    TestPushBackMoveExceptionSafety();
}

//! Test of emplace back move exception safety
//! \brief \ref requirement
TEST_CASE("testing emplace back exception safety"){
    TestEmplaceBackExceptionSafety();
}

//! Test exceptions guarantees for assign operator
//! \brief \ref requirement
TEST_CASE("testing exception safety guaranteees for assign operator"){
    test_ex_assign_operator();
}

//! Test exceptions safety guarantees for concurrent_vector move constructor
//! \brief \ref requirement
TEST_CASE("exception safety guarantees for concurrent_vector move constructor") {
    move_support_tests::test_ex_move_constructor<c_vector_type>();
}

//! Test exceptions safety guarantees for concurrent_vector move assignment
//! \brief \ref requirement
TEST_CASE("test exception safety on concurrent_vector move assignment") {
    test_ex_move_assignment();
}
#endif
//! Test push_back in move only container
//! \brief \ref requirement
TEST_CASE("testing push_back move only container"){
    TestPushBackMoveOnlyContainer();
}

//! Test types for std::iterator_traits in concurrent_vector::iterator
//! \brief \ref requirement
TEST_CASE("testing std::iterator_traits for concurrent_vector::iterator"){
    TestIteratorTraits<oneapi::tbb::concurrent_vector<move_support_tests::Foo>::iterator,move_support_tests::Foo>();
}

//! Test types for std::iterator_traits in concurrent_vector::const_iterator
//! \brief \ref requirement
TEST_CASE("testing std::iterator_traits for concurrent_vector::const_iterator"){
    TestIteratorTraits<oneapi::tbb::concurrent_vector<move_support_tests::Foo>::const_iterator,const move_support_tests::Foo>();
}

//! Test initializer_list support
//! \brief \ref interface \ref requirement
TEST_CASE("testing initializer_list support"){
    TestInitList();
}

//! Test move constructor
//! \brief \ref interface \ref requirement
TEST_CASE("testing move constructor"){
    move_support_tests::test_move_constructor<c_vector_type>();
}

//! Test move assign operator
//! \brief \ref interface \ref requirement
TEST_CASE("testing move assign operator"){
    move_support_tests::test_move_assignment<c_vector_type>();
}

//! Test constructor with move iterators
//! \brief \ref requirement
TEST_CASE("testing constructor with move iterators"){
    move_support_tests::test_constructor_with_move_iterators<c_vector_type>();
}

//! Test assign with move iterators
//! \brief \ref interface \ref requirement
TEST_CASE("testing assign with move iterators"){
    move_support_tests::test_assign_with_move_iterators<c_vector_type>();
}

//! Test grow_by with move iterator
//! \brief \ref requirement
TEST_CASE("testing serial grow_by with move iterator"){
    TestSerialGrowByWithMoveIterators();
}

//! Test grow_by with move iterator
//! \brief \ref requirement
TEST_CASE("testing serial move in shrink_to_fit"){
    TestSerialMoveInShrinkToFit();
}

//! Test concurrent grow
//! \brief \ref requirement
TEST_CASE("testing concurrency"){
    REQUIRE(!move_support_tests::foo_count);
    for (std::size_t p = 1; p <= 4; ++p) {
        oneapi::tbb::global_control limit(oneapi::tbb::global_control::max_allowed_parallelism, p);
        TestParallelFor(p);
        TestConcurrentGrowToAtLeast();
        TestConcurrentGrowBy();
    }

    REQUIRE(!move_support_tests::foo_count);
}

//! Test assign operations
//! \brief \ref interface \ref requirement
TEST_CASE("testing comparison on assign operations"){
    TestComparison();
}

//! Test allocator_traits support in concurrent_vector
//! \brief \ref requirement
TEST_CASE("test allocator_traits support in concurrent_vector") {
    test_allocator_traits_support<c_vector_type>();
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Test deduction guides
//! \brief \ref requirement
TEST_CASE("testing deduction guides"){
    TestDeductionGuides<oneapi::tbb::concurrent_vector>();
}
#endif

//! Test concurrent_vector comparisons
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_vector comparisons") {
    TestVectorComparisons();
}

//! Test concurrent_vector iterators comparisons
//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_vector iterators comparisons") {
    TestVectorIteratorComparisons();
}
