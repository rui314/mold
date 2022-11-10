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

#if _MSC_VER
#if __INTEL_COMPILER
    #pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#else
    // Workaround for vs2015 and warning name was longer than the compiler limit (4096).
    #pragma warning (disable: 4503)
#endif
#endif

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_report.h"
#include "common/utils_concurrency_limit.h"
#include "common/spin_barrier.h"
#include "common/checktype.h"
#include "common/test_comparisons.h"

#include "oneapi/tbb/detail/_utils.h"
#include "oneapi/tbb/enumerable_thread_specific.h"
#include "oneapi/tbb/parallel_for.h"
#include "oneapi/tbb/parallel_reduce.h"
#include "oneapi/tbb/parallel_invoke.h"
#include "oneapi/tbb/blocked_range.h"
#include "oneapi/tbb/tbb_allocator.h"
#include "oneapi/tbb/global_control.h"
#include "oneapi/tbb/cache_aligned_allocator.h"

#include <cstring>
#include <cstdio>
#include <vector>
#include <numeric>
#include <utility>
#include <atomic>

//! \file conformance_enumerable_thread_specific.cpp
//! \brief Test for [tls.enumerable_thread_specific tls.flattened2d] specification

//------------------------------------------------------------------------------------------------------
// Utility types/classes/functions
//------------------------------------------------------------------------------------------------------

//! Minimum number of threads
static int MinThread = 1;

//! Maximum number of threads
static int MaxThread = 4;

static std::atomic<int> construction_counter;
static std::atomic<int> destruction_counter;

const int REPETITIONS = 5;
const int N = 25000;
const int RANGE_MIN = 5000;
const double EXPECTED_SUM = (REPETITIONS + 1) * N;

//! A minimal class that occupies N bytes.
/** Defines default and copy constructor, and allows implicit operator&. Hides operator=. */
template<size_t N = oneapi::tbb::detail::max_nfs_size>
class minimalNComparable: utils::NoAssign {
private:
    int my_value;
    bool is_constructed;
    char pad[N-sizeof(int) - sizeof(bool)];
public:
    minimalNComparable() : utils::NoAssign(), my_value(0) { ++construction_counter; is_constructed = true; }
    minimalNComparable( const minimalNComparable &m ) : utils::NoAssign(), my_value(m.my_value) { ++construction_counter; is_constructed = true; }
    ~minimalNComparable() { ++destruction_counter; CHECK_FAST(is_constructed); is_constructed = false; }
    void set_value( const int i ) { CHECK_FAST(is_constructed); my_value = i; }
    int value( ) const { CHECK_FAST(is_constructed); return my_value; }

    bool operator==( const minimalNComparable& other ) const { return my_value == other.my_value; }
};

static size_t AlignMask = 0;  // set to cache-line-size - 1

template<typename T>
T& check_alignment(T& t, const char *aname) {
    if( !oneapi::tbb::detail::is_aligned(&t, AlignMask)) {
        // TBB_REVAMP_TODO: previously was REPORT_ONCE
        REPORT("alignment error with %s allocator (%x)\n", aname, (int)size_t(&t) & (AlignMask-1));
    }
    return t;
}

template<typename T>
const T& check_alignment(const T& t, const char *aname) {
    if( !oneapi::tbb::detail::is_aligned(&t, AlignMask)) {
        // TBB_REVAMP_TODO: previously was REPORT_ONCE
        REPORT("alignment error with %s allocator (%x)\n", aname, (int)size_t(&t) & (AlignMask-1));
    }
    return t;
}

// Test constructors which throw.  If an ETS constructor throws before completion,
// the already-built objects are un-constructed.  Do not call the destructor if
// this occurs.

static std::atomic<int> gThrowValue;
static int targetThrowValue = 3;

class Thrower {
public:
    Thrower() {
#if TBB_USE_EXCEPTIONS
        if(++gThrowValue == targetThrowValue) {
            throw std::bad_alloc();
        }
#endif
    }
};

// MyThrower field of ThrowingConstructor will throw after a certain number of
// construction calls.  The constructor unwinder wshould unconstruct the instance
// of check_type<int> that was constructed just before.
class ThrowingConstructor {
    CheckType<int> m_checktype;
    Thrower m_throwing_field;
public:
    int m_cnt;
    ThrowingConstructor() : m_checktype(), m_throwing_field() { m_cnt = 0;}

    bool operator==( const ThrowingConstructor& other ) const { return m_cnt == other.m_cnt; }
private:
};

//
// A helper class that simplifies writing the tests since minimalNComparable does not
// define = or + operators.
//

template< typename T >
struct test_helper {
   static inline void init(T &e) { e = static_cast<T>(0); }
   static inline void sum(T &e, const int addend ) { e += static_cast<T>(addend); }
   static inline void sum(T &e, const double addend ) { e += static_cast<T>(addend); }
   static inline void set(T &e, const int value ) { e = static_cast<T>(value); }
   static inline double get(const T &e ) { return static_cast<double>(e); }
};

template<size_t N>
struct test_helper<minimalNComparable<N> > {
   static inline void init(minimalNComparable<N> &sum) { sum.set_value( 0 ); }
   static inline void sum(minimalNComparable<N> &sum, const int addend ) { sum.set_value( sum.value() + addend); }
   static inline void sum(minimalNComparable<N> &sum, const double addend ) { sum.set_value( sum.value() + static_cast<int>(addend)); }
   static inline void sum(minimalNComparable<N> &sum, const minimalNComparable<N> &addend ) { sum.set_value( sum.value() + addend.value()); }
   static inline void set(minimalNComparable<N> &v, const int value ) { v.set_value( static_cast<int>(value) ); }
   static inline double get(const minimalNComparable<N> &sum ) { return static_cast<double>(sum.value()); }
};

template<>
struct test_helper<ThrowingConstructor> {
   static inline void init(ThrowingConstructor &sum) { sum.m_cnt = 0; }
   static inline void sum(ThrowingConstructor &sum, const int addend ) { sum.m_cnt += addend; }
   static inline void sum(ThrowingConstructor &sum, const double addend ) { sum.m_cnt += static_cast<int>(addend); }
   static inline void sum(ThrowingConstructor &sum, const ThrowingConstructor &addend ) { sum.m_cnt += addend.m_cnt; }
   static inline void set(ThrowingConstructor &v, const int value ) { v.m_cnt = static_cast<int>(value); }
   static inline double get(const ThrowingConstructor &sum ) { return static_cast<double>(sum.m_cnt); }
};

//! Tag class used to make certain constructors hard to invoke accidentally.
struct SecretTagType {} SecretTag;

//// functors and routines for initialization and combine

//! Counts instances of FunctorFinit
static std::atomic<int> FinitCounter;

template <typename T, int Value>
struct FunctorFinit {
    FunctorFinit( const FunctorFinit& ) {++FinitCounter;}
    FunctorFinit( SecretTagType ) {++FinitCounter;}
    ~FunctorFinit() {--FinitCounter;}
    T operator()() { return Value; }
};

template <int Value>
struct FunctorFinit<ThrowingConstructor,Value> {
    FunctorFinit( const FunctorFinit& ) {++FinitCounter;}
    FunctorFinit( SecretTagType ) {++FinitCounter;}
    ~FunctorFinit() {--FinitCounter;}
    ThrowingConstructor operator()() { ThrowingConstructor temp; temp.m_cnt = Value; return temp; }
};

template <size_t N, int Value>
struct FunctorFinit<minimalNComparable<N>,Value> {
    FunctorFinit( const FunctorFinit& ) {++FinitCounter;}
    FunctorFinit( SecretTagType ) {++FinitCounter;}
    ~FunctorFinit() {--FinitCounter;}
    minimalNComparable<N> operator()() {
        minimalNComparable<N> result;
        result.set_value( Value );
        return result;
    }
};

// Addition

template <typename T>
struct FunctorAddCombineRef {
    T operator()(const T& left, const T& right) const {
        return left+right;
    }
};

template <size_t N>
struct FunctorAddCombineRef<minimalNComparable<N> > {
    minimalNComparable<N> operator()(const minimalNComparable<N>& left, const minimalNComparable<N>& right) const {
        minimalNComparable<N> result;
        result.set_value( left.value() + right.value() );
        return result;
    }
};

template <>
struct FunctorAddCombineRef<ThrowingConstructor> {
    ThrowingConstructor operator()(const ThrowingConstructor& left, const ThrowingConstructor& right) const {
        ThrowingConstructor result;
        result.m_cnt = ( left.m_cnt + right.m_cnt );
        return result;
    }
};

template <typename T>
struct FunctorAddCombine {
    T operator()(T left, T right ) const {
        return FunctorAddCombineRef<T>()( left, right );
    }
};

template <typename T>
T FunctionAddByRef( const T &left, const T &right) {
    return FunctorAddCombineRef<T>()( left, right );
}

template <typename T>
T FunctionAdd( T left, T right) { return FunctionAddByRef(left,right); }

template <typename T>
class Accumulator {
public:
    Accumulator(T& result) : my_result(result) {}
    Accumulator(const Accumulator& other) : my_result(other.my_result) {}
    Accumulator& operator=(const Accumulator& other) {
        test_helper<T>::set(my_result, test_helper<T>::get(other));
        return *this;
    }
    void operator()(const T& new_bit) { test_helper<T>::sum(my_result, new_bit); }
private:
    T& my_result;
};

template <typename T>
class ClearingAccumulator {
public:
    ClearingAccumulator(T& result) : my_result(result) {}
    ClearingAccumulator(const ClearingAccumulator& other) : my_result(other.my_result) {}
    ClearingAccumulator& operator=(const ClearingAccumulator& other) {
        test_helper<T>::set(my_result, test_helper<T>::get(other));
        return *this;
    }
    void operator()(T& new_bit) {
        test_helper<T>::sum(my_result, new_bit);
        test_helper<T>::init(new_bit);
    }
    static void AssertClean(const T& thread_local_value) {
        T zero;
        test_helper<T>::init(zero);
        REQUIRE_MESSAGE(test_helper<T>::get(thread_local_value)==test_helper<T>::get(zero),
               "combine_each does not allow to modify thread local values?");
    }
private:
    T& my_result;
};

//// end functors and routines

//------------------------------------------------------------------------------------------------------
// Tests for tests cases
//------------------------------------------------------------------------------------------------------

template <typename T, template<class> class Allocator>
class parallel_scalar_body: utils::NoAssign {
    typedef oneapi::tbb::enumerable_thread_specific<T, Allocator<T> > ets_type;
    ets_type &sums;
    const char* allocator_name;

public:

    parallel_scalar_body ( ets_type &_sums, const char *alloc_name ) : sums(_sums), allocator_name(alloc_name) { }

    void operator()( const oneapi::tbb::blocked_range<int> &r ) const {
        for (int i = r.begin(); i != r.end(); ++i)
            test_helper<T>::sum( check_alignment(sums.local(),allocator_name), 1 );
    }

};

template< typename T, template<class> class Allocator>
void run_parallel_scalar_tests_nocombine(const char* /* test_name */, const char *allocator_name) {

    typedef oneapi::tbb::enumerable_thread_specific<T, Allocator<T> > ets_type;

    Checker<T> my_check;

    gThrowValue = 0;
    struct fail_on_exception_guard {
        bool dismiss = false;
        ~fail_on_exception_guard() {
            if (!dismiss) {
                FAIL("The exception is not expected");
            }
        }
    } guard;
    T default_value{};
    guard.dismiss = true;

    gThrowValue = 0;
    {
        // We assume that static_sums zero-initialized or has a default constructor that zeros it.
        ets_type static_sums = ets_type( T() );

        T exemplar;
        test_helper<T>::init(exemplar);

        for (int p = std::max(MinThread, 2); p <= MaxThread; ++p) {
            oneapi::tbb::global_control gc(oneapi::tbb::global_control::max_allowed_parallelism, p);

            T iterator_sum;
            test_helper<T>::init(iterator_sum);

            T finit_ets_sum;
            test_helper<T>::init(finit_ets_sum);

            T const_iterator_sum;
            test_helper<T>::init(const_iterator_sum);

            T range_sum;
            test_helper<T>::init(range_sum);

            T const_range_sum;
            test_helper<T>::init(const_range_sum);

            T cconst_sum;
            test_helper<T>::init(cconst_sum);

            T assign_sum;
            test_helper<T>::init(assign_sum);

            T cassgn_sum;
            test_helper<T>::init(cassgn_sum);
            T non_cassgn_sum;
            test_helper<T>::init(non_cassgn_sum);

            T static_sum;
            test_helper<T>::init(static_sum);

            for (int t = -1; t < REPETITIONS; ++t) {
                static_sums.clear();

                ets_type sums(exemplar);
                FunctorFinit<T,0> my_finit(SecretTag);
                ets_type finit_ets(my_finit);

                REQUIRE( sums.empty());
                oneapi::tbb::parallel_for( oneapi::tbb::blocked_range<int>( 0, N*p, RANGE_MIN ), parallel_scalar_body<T,Allocator>( sums, allocator_name ) );
                REQUIRE( !sums.empty());

                REQUIRE( finit_ets.empty());
                oneapi::tbb::parallel_for( oneapi::tbb::blocked_range<int>( 0, N*p, RANGE_MIN ), parallel_scalar_body<T,Allocator>( finit_ets, allocator_name ) );
                REQUIRE( !finit_ets.empty());

                REQUIRE(static_sums.empty());
                oneapi::tbb::parallel_for( oneapi::tbb::blocked_range<int>( 0, N*p, RANGE_MIN ), parallel_scalar_body<T,Allocator>( static_sums, allocator_name ) );
                REQUIRE( !static_sums.empty());

                // use iterator
                typename ets_type::size_type size = 0;
                for ( typename ets_type::iterator i = sums.begin(); i != sums.end(); ++i ) {
                     ++size;
                     test_helper<T>::sum(iterator_sum, *i);
                }
                REQUIRE( sums.size() == size);

                // use const_iterator
                for ( typename ets_type::const_iterator i = sums.begin(); i != sums.end(); ++i ) {
                     test_helper<T>::sum(const_iterator_sum, *i);
                }

                // use range_type
                typename ets_type::range_type r = sums.range();
                for ( typename ets_type::range_type::const_iterator i = r.begin(); i != r.end(); ++i ) {
                     test_helper<T>::sum(range_sum, *i);
                }

                // use const_range_type
                const ets_type& csums = sums;
                typename ets_type::const_range_type cr = csums.range();
                for ( typename ets_type::const_range_type::iterator i = cr.begin(); i != cr.end(); ++i ) {
                     test_helper<T>::sum(const_range_sum, *i);
                }

                // test copy constructor, with TLS-cached locals
                typedef typename oneapi::tbb::enumerable_thread_specific<T, Allocator<T>, oneapi::tbb::ets_key_per_instance> cached_ets_type;

                cached_ets_type cconst(sums);
                oneapi::tbb::parallel_for( oneapi::tbb::blocked_range<int>(0, N*p, RANGE_MIN), [&]( const oneapi::tbb::blocked_range<int>& ) {
                    bool exists = false;
                    T& ref = cconst.local(exists);
                    CHECK( (exists || ref == default_value) );
                } );
                cached_ets_type cconst_to_assign1 = cconst;
                cached_ets_type cconst_to_assign2;
                cconst_to_assign2 = std::move(cconst_to_assign1);
                REQUIRE(cconst_to_assign2.size() == cconst.size());

                for ( typename cached_ets_type::const_iterator i = cconst.begin(); i != cconst.end(); ++i ) {
                     test_helper<T>::sum(cconst_sum, *i);
                }

                // test assignment
                ets_type assigned;
                assigned = sums;

                for ( typename ets_type::const_iterator i = assigned.begin(); i != assigned.end(); ++i ) {
                     test_helper<T>::sum(assign_sum, *i);
                }

                // test assign to and from cached locals
                cached_ets_type cassgn;
                cassgn = sums;
                for ( typename cached_ets_type::const_iterator i = cassgn.begin(); i != cassgn.end(); ++i ) {
                     test_helper<T>::sum(cassgn_sum, *i);
                }

                ets_type non_cassgn;
                non_cassgn = cassgn;
                for ( typename ets_type::const_iterator i = non_cassgn.begin(); i != non_cassgn.end(); ++i ) {
                     test_helper<T>::sum(non_cassgn_sum, *i);
                }

                // test finit-initialized ets
                for(typename ets_type::const_iterator i = finit_ets.begin(); i != finit_ets.end(); ++i) {
                    test_helper<T>::sum(finit_ets_sum, *i);
                }

                // test static ets
                for(typename ets_type::const_iterator i = static_sums.begin(); i != static_sums.end(); ++i) {
                    test_helper<T>::sum(static_sum, *i);
                }

            }

            REQUIRE(EXPECTED_SUM*p == test_helper<T>::get(iterator_sum));
            REQUIRE(EXPECTED_SUM*p == test_helper<T>::get(const_iterator_sum));
            REQUIRE(EXPECTED_SUM*p == test_helper<T>::get(range_sum));
            REQUIRE(EXPECTED_SUM*p == test_helper<T>::get(const_range_sum));

            REQUIRE(EXPECTED_SUM*p == test_helper<T>::get(cconst_sum));
            REQUIRE(EXPECTED_SUM*p == test_helper<T>::get(assign_sum));
            REQUIRE(EXPECTED_SUM*p == test_helper<T>::get(cassgn_sum));
            REQUIRE(EXPECTED_SUM*p == test_helper<T>::get(non_cassgn_sum));
            REQUIRE(EXPECTED_SUM*p == test_helper<T>::get(finit_ets_sum));
            REQUIRE(EXPECTED_SUM*p == test_helper<T>::get(static_sum));
        }
    }  // Checker block
}

template< typename T, template<class> class Allocator>
void run_parallel_scalar_tests(const char* test_name, const char* allocator_name) {

    typedef oneapi::tbb::enumerable_thread_specific<T, Allocator<T> > ets_type;
    bool exception_caught = false;

    // We assume that static_sums zero-initialized or has a default constructor that zeros it.
    ets_type static_sums = ets_type( T() );

    T exemplar;
    test_helper<T>::init(exemplar);

    int test_throw_count = 10;
    // the test will be performed repeatedly until it does not throw.  For non-throwing types
    // this means once; for the throwing type test it may loop two or three times.  The
    // value of targetThrowValue will determine when and if the test will throw.
    do {
        targetThrowValue = test_throw_count;  // keep testing until we get no exception
        exception_caught = false;
#if TBB_USE_EXCEPTIONS
        try {
#endif
            run_parallel_scalar_tests_nocombine<T,Allocator>(test_name, allocator_name);
#if TBB_USE_EXCEPTIONS
        }
        catch(...) {}
#endif
        for (int p = std::max(MinThread, 2); p <= MaxThread; ++p) {
            oneapi::tbb::global_control gc(oneapi::tbb::global_control::max_allowed_parallelism, p);

            gThrowValue = 0;

            T combine_sum;
            test_helper<T>::init(combine_sum);

            T combine_ref_sum;
            test_helper<T>::init(combine_ref_sum);

            T accumulator_sum;
            test_helper<T>::init(accumulator_sum);

            T static_sum;
            test_helper<T>::init(static_sum);

            T clearing_accumulator_sum;
            test_helper<T>::init(clearing_accumulator_sum);

            {
                Checker<T> my_check;
#if TBB_USE_EXCEPTIONS
                try
#endif
                {
                    for (int t = -1; t < REPETITIONS; ++t) {
                        static_sums.clear();

                        ets_type sums(exemplar);

                        REQUIRE(sums.empty());
                        oneapi::tbb::parallel_for(oneapi::tbb::blocked_range<int>(0, N * p, RANGE_MIN),
                            parallel_scalar_body<T, Allocator>(sums, allocator_name));
                        REQUIRE(!sums.empty());

                        REQUIRE(static_sums.empty());
                        oneapi::tbb::parallel_for(oneapi::tbb::blocked_range<int>(0, N * p, RANGE_MIN),
                            parallel_scalar_body<T, Allocator>(static_sums, allocator_name));
                        REQUIRE(!static_sums.empty());

                        // Use combine
                        test_helper<T>::sum(combine_sum, sums.combine(FunctionAdd<T>));
                        test_helper<T>::sum(combine_ref_sum, sums.combine(FunctionAddByRef<T>));
                        test_helper<T>::sum(static_sum, static_sums.combine(FunctionAdd<T>));

                        // Accumulate with combine_each
                        sums.combine_each(Accumulator<T>(accumulator_sum));
                        // Accumulate and clear thread-local values
                        sums.combine_each(ClearingAccumulator<T>(clearing_accumulator_sum));
                        // Check that the values were cleared
                        sums.combine_each(ClearingAccumulator<T>::AssertClean);
                    }
                }
#if TBB_USE_EXCEPTIONS
                catch (...) {
                    exception_caught = true;
                }
#endif
            }

            if (!exception_caught) {
                REQUIRE(EXPECTED_SUM * p == test_helper<T>::get(combine_sum));
                REQUIRE(EXPECTED_SUM * p == test_helper<T>::get(combine_ref_sum));
                REQUIRE(EXPECTED_SUM * p == test_helper<T>::get(static_sum));
                REQUIRE(EXPECTED_SUM * p == test_helper<T>::get(accumulator_sum));
                REQUIRE(EXPECTED_SUM * p == test_helper<T>::get(clearing_accumulator_sum));
            }

        }  // MinThread .. MaxThread
        test_throw_count += 10;  // keep testing until we don't get an exception
    } while (exception_caught && test_throw_count < 200);
    REQUIRE_MESSAGE(!exception_caught, "No non-exception test completed");
}

template <typename T, template<class> class Allocator>
class parallel_vector_for_body: utils::NoAssign {
    typedef std::vector<T, oneapi::tbb::tbb_allocator<T> > container_type;
    typedef oneapi::tbb::enumerable_thread_specific< container_type, Allocator<container_type> > ets_type;
    ets_type &locals;
    const char *allocator_name;

public:

    parallel_vector_for_body ( ets_type &_locals, const char *aname ) : locals(_locals), allocator_name(aname) { }

    void operator()( const oneapi::tbb::blocked_range<int> &r ) const {
        T one;
        test_helper<T>::set(one, 1);

        for (int i = r.begin(); i < r.end(); ++i) {
            check_alignment(locals.local(),allocator_name).push_back( one );
        }
    }

};

template <typename R, typename T>
struct parallel_vector_reduce_body {

    T sum;
    size_t count;
    typedef std::vector<T, oneapi::tbb::tbb_allocator<T> > container_type;

    parallel_vector_reduce_body ( ) : count(0) { test_helper<T>::init(sum); }
    parallel_vector_reduce_body ( parallel_vector_reduce_body<R, T> &, oneapi::tbb::split ) : count(0) {  test_helper<T>::init(sum); }

    void operator()( const R &r ) {
        for (typename R::iterator ri = r.begin(); ri != r.end(); ++ri) {
            const container_type &v = *ri;
            ++count;
            for (typename container_type::const_iterator vi = v.begin(); vi != v.end(); ++vi) {
                test_helper<T>::sum(sum, *vi);
            }
        }
    }

    void join( const parallel_vector_reduce_body &b ) {
        test_helper<T>::sum(sum,b.sum);
        count += b.count;
    }

};

template< typename T, template<class> class Allocator>
void run_parallel_vector_tests(const char* /* test_name */, const char *allocator_name) {
    typedef std::vector<T, oneapi::tbb::tbb_allocator<T> > container_type;
    typedef oneapi::tbb::enumerable_thread_specific< container_type, Allocator<container_type> > ets_type;

    for (int p = std::max(MinThread, 2); p <= MaxThread; ++p) {
        oneapi::tbb::global_control gc(oneapi::tbb::global_control::max_allowed_parallelism, p);

        T sum;
        test_helper<T>::init(sum);

        for (int t = -1; t < REPETITIONS; ++t) {
            ets_type vs;

            REQUIRE( vs.empty() );
            oneapi::tbb::parallel_for( oneapi::tbb::blocked_range<int> (0, N*p, RANGE_MIN),
                               parallel_vector_for_body<T,Allocator>( vs, allocator_name ) );
            REQUIRE( !vs.empty() );

            // copy construct
            ets_type vs2(vs); // this causes an assertion failure, related to allocators...

            // assign
            ets_type vs3;
            vs3 = vs;

            parallel_vector_reduce_body< typename ets_type::const_range_type, T > pvrb;
            oneapi::tbb::parallel_reduce ( vs.range(1), pvrb );

            test_helper<T>::sum(sum, pvrb.sum);

            REQUIRE( vs.size() == pvrb.count );
            REQUIRE( vs2.size() == pvrb.count );
            REQUIRE( vs3.size() == pvrb.count );

            oneapi::tbb::flattened2d<ets_type> fvs = flatten2d(vs);
            size_t ccount = fvs.size();
            REQUIRE( ccount == size_t(N*p) );
            size_t elem_cnt = 0;
            typename oneapi::tbb::flattened2d<ets_type>::iterator it;
            auto it2(it);
            it = fvs.begin();
            REQUIRE(it != it2);
            typename oneapi::tbb::flattened2d<ets_type>::iterator it3;
            typename oneapi::tbb::flattened2d<ets_type>::const_iterator cit = fvs.begin();
            it3 = cit;
            REQUIRE(it3 == cit);
            REQUIRE(it3.operator->() == &(*it3));

            for(typename oneapi::tbb::flattened2d<ets_type>::const_iterator i = fvs.begin(); i != fvs.end(); ++i) {
                ++elem_cnt;
            };
            REQUIRE( ccount == elem_cnt );

            elem_cnt = 0;
            for(typename oneapi::tbb::flattened2d<ets_type>::iterator i = fvs.begin(); i != fvs.end(); i++) {
                ++elem_cnt;
            };
            REQUIRE( ccount == elem_cnt );

            // Test the ETS constructor with multiple args
            T minus_one;
            test_helper<T>::set(minus_one, -1);
            // Set ETS to construct "local" vectors pre-occupied with 25 "minus_one"s
            // Cast 25 to size_type to prevent Intel Compiler SFINAE compilation issues with gcc 5.
            ets_type vvs( typename container_type::size_type(25), minus_one, oneapi::tbb::tbb_allocator<T>() );
            REQUIRE( vvs.empty() );
            oneapi::tbb::parallel_for ( oneapi::tbb::blocked_range<int> (0, N*p, RANGE_MIN), parallel_vector_for_body<T,Allocator>( vvs, allocator_name ) );
            REQUIRE( !vvs.empty() );

            parallel_vector_reduce_body< typename ets_type::const_range_type, T > pvrb2;
            oneapi::tbb::parallel_reduce ( vvs.range(1), pvrb2 );
            REQUIRE( pvrb2.count == vvs.size() );
            REQUIRE( test_helper<T>::get(pvrb2.sum) == N*p-pvrb2.count*25 );

            oneapi::tbb::flattened2d<ets_type> fvvs = flatten2d(vvs);
            ccount = fvvs.size();
            REQUIRE( ccount == N*p+pvrb2.count*25 );
        }

        double result_value = test_helper<T>::get(sum);
        REQUIRE( EXPECTED_SUM*p == result_value);
    }
}

template<typename T, template<class> class Allocator>
void run_cross_type_vector_tests(const char* /* test_name */) {
    const char* allocator_name = "default";
    typedef std::vector<T, oneapi::tbb::tbb_allocator<T> > container_type;

    for (int p = std::max(MinThread, 2); p <= MaxThread; ++p) {
        oneapi::tbb::global_control gc(oneapi::tbb::global_control::max_allowed_parallelism, p);

        T sum;
        test_helper<T>::init(sum);

        for (int t = -1; t < REPETITIONS; ++t) {
            typedef typename oneapi::tbb::enumerable_thread_specific< container_type, Allocator<container_type>, oneapi::tbb::ets_no_key > ets_nokey_type;
            typedef typename oneapi::tbb::enumerable_thread_specific< container_type, Allocator<container_type>, oneapi::tbb::ets_key_per_instance > ets_tlskey_type;
            ets_nokey_type vs;

            REQUIRE( vs.empty());
            oneapi::tbb::parallel_for ( oneapi::tbb::blocked_range<int> (0, N*p, RANGE_MIN), parallel_vector_for_body<T, Allocator>( vs, allocator_name ) );
            REQUIRE( !vs.empty());

            // copy construct
            ets_tlskey_type vs2(vs);

            // assign
            ets_nokey_type vs3;
            vs3 = vs2;

            parallel_vector_reduce_body< typename ets_nokey_type::const_range_type, T > pvrb;
            oneapi::tbb::parallel_reduce ( vs3.range(1), pvrb );

            test_helper<T>::sum(sum, pvrb.sum);

            REQUIRE( vs3.size() == pvrb.count);

            oneapi::tbb::flattened2d<ets_nokey_type> fvs = flatten2d(vs3);
            size_t ccount = fvs.size();
            size_t elem_cnt = 0;
            for(typename oneapi::tbb::flattened2d<ets_nokey_type>::const_iterator i = fvs.begin(); i != fvs.end(); ++i) {
                ++elem_cnt;
            };
            REQUIRE(ccount == elem_cnt);

            elem_cnt = 0;
            for(typename oneapi::tbb::flattened2d<ets_nokey_type>::iterator i = fvs.begin(); i != fvs.end(); ++i) {
                ++elem_cnt;
            };
            REQUIRE(ccount == elem_cnt);

            oneapi::tbb::flattened2d<ets_nokey_type> fvs2 = flatten2d(vs3, vs3.begin(), std::next(vs3.begin()));
            REQUIRE(std::distance(fvs2.begin(), fvs2.end()) == vs3.begin()->size());
            const oneapi::tbb::flattened2d<ets_nokey_type>& cfvs2(fvs2);
            REQUIRE(std::distance(cfvs2.begin(), cfvs2.end()) == vs3.begin()->size());
        }

        double result_value = test_helper<T>::get(sum);
        REQUIRE( EXPECTED_SUM*p == result_value);
    }
}

template< typename T >
void run_serial_scalar_tests(const char* /* test_name */) {
    T sum;
    test_helper<T>::init(sum);

    for (int t = -1; t < REPETITIONS; ++t) {
        for (int i = 0; i < N; ++i) {
            test_helper<T>::sum(sum,1);
        }
    }

    double result_value = test_helper<T>::get(sum);
    REQUIRE( EXPECTED_SUM == result_value);
}

template< typename T >
void run_serial_vector_tests(const char* /* test_name */) {
    T sum;
    test_helper<T>::init(sum);
    T one;
    test_helper<T>::set(one, 1);

    for (int t = -1; t < REPETITIONS; ++t) {
        std::vector<T, oneapi::tbb::tbb_allocator<T> > v;
        for (int i = 0; i < N; ++i) {
            v.push_back( one );
        }
        for (typename std::vector<T, oneapi::tbb::tbb_allocator<T> >::const_iterator i = v.begin(); i != v.end(); ++i)
            test_helper<T>::sum(sum, *i);
    }

    double result_value = test_helper<T>::get(sum);
    REQUIRE( EXPECTED_SUM == result_value);
}

const size_t line_size = oneapi::tbb::detail::max_nfs_size;

void run_reference_check() {
    run_serial_scalar_tests<int>("int");
    run_serial_scalar_tests<double>("double");
    run_serial_scalar_tests<minimalNComparable<> >("minimalNComparable<>");
    run_serial_vector_tests<int>("std::vector<int, oneapi::tbb::tbb_allocator<int> >");
    run_serial_vector_tests<double>("std::vector<double, oneapi::tbb::tbb_allocator<double> >");
}

template<template<class>class Allocator>
void run_parallel_tests(const char *allocator_name) {
    run_parallel_scalar_tests<int, Allocator>("int",allocator_name);
    run_parallel_scalar_tests<double, Allocator>("double",allocator_name);
    run_parallel_scalar_tests_nocombine<minimalNComparable<>,Allocator>("minimalNComparable<>",allocator_name);
    run_parallel_scalar_tests<ThrowingConstructor, Allocator>("ThrowingConstructor", allocator_name);
    run_parallel_vector_tests<int, Allocator>("std::vector<int, oneapi::tbb::tbb_allocator<int> >",allocator_name);
    run_parallel_vector_tests<double, Allocator>("std::vector<double, oneapi::tbb::tbb_allocator<double> >",allocator_name);
}

void run_cross_type_tests() {
    // cross-type scalar tests are part of run_parallel_scalar_tests_nocombine
    run_cross_type_vector_tests<int, oneapi::tbb::tbb_allocator>("std::vector<int, oneapi::tbb::tbb_allocator<int> >");
    run_cross_type_vector_tests<double, oneapi::tbb::tbb_allocator>("std::vector<double, oneapi::tbb::tbb_allocator<double> >");
}

template<typename T, template<class> class Allocator, typename Init>
oneapi::tbb::enumerable_thread_specific<T,Allocator<T> > MakeETS( Init init ) {
    return oneapi::tbb::enumerable_thread_specific<T,Allocator<T> >(init);
}
// In some GCC versions, parameter packs in lambdas might cause compile errors
template<typename ETS, typename... P>
struct MakeETS_Functor {
    ETS operator()( typename std::decay<P>::type&&... params ) {
        return ETS(std::move(params)...);
    }
};
template<typename T, template<class> class Allocator, typename... P>
oneapi::tbb::enumerable_thread_specific<T,Allocator<T> > MakeETS( oneapi::tbb::detail::stored_pack<P...> pack ) {
    typedef oneapi::tbb::enumerable_thread_specific<T,Allocator<T> > result_type;
    return oneapi::tbb::detail::call_and_return< result_type >(
        MakeETS_Functor<result_type,P...>(), std::move(pack)
    );
}

template<typename T, template<class> class Allocator, typename InitSrc, typename InitDst, typename Validator>
void ets_copy_assign_test( InitSrc init1, InitDst init2, Validator check, const char *allocator_name ) {
    typedef oneapi::tbb::enumerable_thread_specific<T, Allocator<T> > ets_type;

    // Create the source instance
    const ets_type& cref_binder = MakeETS<T, Allocator>(init1);
    ets_type& source = const_cast<ets_type&>(cref_binder);
    check(check_alignment(source.local(),allocator_name));

    // Test copy construction
    bool existed = false;
    ets_type copy(source);
    check(check_alignment(copy.local(existed),allocator_name));
    REQUIRE_MESSAGE(existed, "Local data not created by ETS copy constructor");
    copy.clear();
    check(check_alignment(copy.local(),allocator_name));

    // Test assignment
    existed = false;
    ets_type assign(init2);
    assign = source;
    check(check_alignment(assign.local(existed),allocator_name));
    REQUIRE_MESSAGE(existed, "Local data not created by ETS assignment");
    assign.clear();
    check(check_alignment(assign.local(),allocator_name));

    // Create the source instance
    ets_type&& rvref_binder = MakeETS<T, Allocator>(init1);
    check(check_alignment(rvref_binder.local(),allocator_name));

    // Test move construction
    existed = false;
    ets_type moved(rvref_binder);
    check(check_alignment(moved.local(existed),allocator_name));
    REQUIRE_MESSAGE(existed, "Local data not created by ETS move constructor");
    moved.clear();
    check(check_alignment(moved.local(),allocator_name));

    // Test assignment
    existed = false;
    ets_type move_assign(init2);
    move_assign = std::move(moved);
    check(check_alignment(move_assign.local(existed),allocator_name));
    REQUIRE_MESSAGE(existed, "Local data not created by ETS move assignment");
    move_assign.clear();
    check(check_alignment(move_assign.local(),allocator_name));
}

template<typename T, int Expected>
struct Validator {
    void operator()( const T& value ) {
        REQUIRE(test_helper<T>::get(value) == Expected);
    }
    void operator()( const std::pair<int,T>& value ) {
        REQUIRE(value.first > 0);
        REQUIRE(test_helper<T>::get(value.second) == Expected*value.first);
    }
};

template <typename T, template<class> class Allocator>
void run_assign_and_copy_constructor_test(const char* /* test_name */, const char *allocator_name) {
    #define EXPECTED 3142

    // test with exemplar initializer
    T src_init;
    test_helper<T>::set(src_init,EXPECTED);
    T other_init;
    test_helper<T>::init(other_init);
    ets_copy_assign_test<T, Allocator>(src_init, other_init, Validator<T,EXPECTED>(), allocator_name);

    // test with function initializer
    FunctorFinit<T,EXPECTED> src_finit(SecretTag);
    FunctorFinit<T,0> other_finit(SecretTag);
    ets_copy_assign_test<T, Allocator>(src_finit, other_finit, Validator<T,EXPECTED>(), allocator_name);

    // test with multi-argument "emplace" initializer
    // The arguments are wrapped into oneapi::tbb::internal::stored_pack to avoid variadic templates in ets_copy_assign_test.
    test_helper<T>::set(src_init,EXPECTED*17);
    ets_copy_assign_test< std::pair<int,T>, Allocator>(oneapi::tbb::detail::save_pack(17,src_init), std::make_pair(-1,T()), Validator<T,EXPECTED>(), allocator_name);
    #undef EXPECTED
}

template< template<class> class Allocator>
void run_assignment_and_copy_constructor_tests(const char* allocator_name) {
    run_assign_and_copy_constructor_test<int, Allocator>("int", allocator_name);
    run_assign_and_copy_constructor_test<double, Allocator>("double", allocator_name);
    // Try class sizes that are close to a cache line in size, in order to check padding calculations.
    run_assign_and_copy_constructor_test<minimalNComparable<line_size-1>, Allocator >("minimalNComparable<line_size-1>", allocator_name);
    run_assign_and_copy_constructor_test<minimalNComparable<line_size>, Allocator >("minimalNComparable<line_size>", allocator_name);
    run_assign_and_copy_constructor_test<minimalNComparable<line_size+1>, Allocator >("minimalNComparable<line_size+1>", allocator_name);
    REQUIRE(FinitCounter==0);
}

// Class with no default constructor
class HasNoDefaultConstructor {
    HasNoDefaultConstructor();
public:
    HasNoDefaultConstructor( SecretTagType ) {}
};
// Initialization functor for HasNoDefaultConstructor
struct HasNoDefaultConstructorFinit {
    HasNoDefaultConstructor operator()() {
        return HasNoDefaultConstructor(SecretTag);
    }
};
// Combine functor for HasNoDefaultConstructor
struct HasNoDefaultConstructorCombine {
    HasNoDefaultConstructor operator()( HasNoDefaultConstructor, HasNoDefaultConstructor ) {
        return HasNoDefaultConstructor(SecretTag);
    }
};

// Class that only has a constructor with multiple parameters and a move constructor
class HasSpecialAndMoveCtor : utils::NoCopy {
    HasSpecialAndMoveCtor();
public:
    HasSpecialAndMoveCtor( SecretTagType, size_t = size_t(0), const char* = "" ) {}
    HasSpecialAndMoveCtor( HasSpecialAndMoveCtor&& ) {}
};

// No-op combine-each functor
template<typename V>
struct EmptyCombineEach {
    void operator()( const V& ) { }
};

//! Test situations where only default constructor or copy constructor is required.
template<template<class> class Allocator>
void TestInstantiation(const char* /* allocator_name */) {
    // Test instantiation is possible when copy constructor is not required.
    oneapi::tbb::enumerable_thread_specific<utils::NoCopy, Allocator<utils::NoCopy> > ets1;
    ets1.local();
    ets1.combine_each(EmptyCombineEach<utils::NoCopy>());

    // Test instantiation when default constructor is not required, because exemplar is provided.
    HasNoDefaultConstructor x(SecretTag);
    oneapi::tbb::enumerable_thread_specific<HasNoDefaultConstructor, Allocator<HasNoDefaultConstructor> > ets2(x);
    ets2.local();
    ets2.combine(HasNoDefaultConstructorCombine());

    // Test instantiation when default constructor is not required, because init function is provided.
    HasNoDefaultConstructorFinit f;
    oneapi::tbb::enumerable_thread_specific<HasNoDefaultConstructor, Allocator<HasNoDefaultConstructor> > ets3(f);
    ets3.local();
    ets3.combine(HasNoDefaultConstructorCombine());

    // Test instantiation with multiple arguments
    oneapi::tbb::enumerable_thread_specific<HasSpecialAndMoveCtor, Allocator<HasSpecialAndMoveCtor> > ets4(SecretTag, 0x42, "meaningless");
    ets4.local();
    ets4.combine_each(EmptyCombineEach<HasSpecialAndMoveCtor>());
    // Test instantiation with one argument that should however use the variadic constructor
    oneapi::tbb::enumerable_thread_specific<HasSpecialAndMoveCtor, Allocator<HasSpecialAndMoveCtor> > ets5(SecretTag);
    ets5.local();
    ets5.combine_each(EmptyCombineEach<HasSpecialAndMoveCtor>());
    // Test that move operations do not impose extra requirements
    // Default allocator is used. If it does not match Allocator, there will be elementwise move
    oneapi::tbb::enumerable_thread_specific<HasSpecialAndMoveCtor> ets6( std::move(ets4) );
    ets6.combine_each(EmptyCombineEach<HasSpecialAndMoveCtor>());
    ets6 = std::move(ets5);
}

void TestMemberTypes() {
    using default_container_type = oneapi::tbb::enumerable_thread_specific<int>;
    static_assert(std::is_same<typename default_container_type::allocator_type, oneapi::tbb::cache_aligned_allocator<int>>::value,
            "Incorrect default template allocator");

    using test_allocator_type = std::allocator<int>;
    using ets_container_type = oneapi::tbb::enumerable_thread_specific<int, test_allocator_type>;

    static_assert(std::is_same<typename ets_container_type::allocator_type, test_allocator_type>::value,
                  "Incorrect container allocator_type member type");

    using value_type = typename ets_container_type::value_type;

    static_assert(std::is_same<typename ets_container_type::value_type, int>::value,
                  "Incorrect container value_type member type");
    static_assert(std::is_same<typename ets_container_type::reference, value_type&>::value,
                  "Incorrect container reference member type");
    static_assert(std::is_same<typename ets_container_type::const_reference, const value_type&>::value,
                  "Incorrect container const_reference member type");

    using allocator_type = typename ets_container_type::allocator_type;
    static_assert(std::is_same<typename ets_container_type::pointer, typename std::allocator_traits<allocator_type>::pointer>::value,
                  "Incorrect container pointer member type");
    static_assert(std::is_same<typename ets_container_type::const_pointer, typename std::allocator_traits<allocator_type>::const_pointer>::value,
                  "Incorrect container const_pointer member type");

    static_assert(std::is_unsigned<typename ets_container_type::size_type>::value,
                  "Incorrect container size_type member type");
    static_assert(std::is_signed<typename ets_container_type::difference_type>::value,
                  "Incorrect container difference_type member type");

    static_assert(utils::is_random_access_iterator<typename ets_container_type::iterator>::value,
                  "Incorrect container iterator member type");
    static_assert(!std::is_const<typename ets_container_type::iterator::value_type>::value,
                  "Incorrect container iterator member type");
    static_assert(utils::is_random_access_iterator<typename ets_container_type::const_iterator>::value,
                  "Incorrect container const_iterator member type");
    static_assert(std::is_const<typename ets_container_type::const_iterator::value_type>::value,
                  "Incorrect container iterator member type");
}

size_t init_tbb_alloc_mask() {
    // TODO: use __TBB_alignof(T) to check for local() results instead of using internal knowledges of ets element padding
    if(oneapi::tbb::tbb_allocator<int>::allocator_type() == oneapi::tbb::tbb_allocator<int>::standard) {
        // scalable allocator is not available.
        return 1;
    }
    else {
        // this value is for large objects, but will be correct for small.
        return 64; // TBB_REVAMP_TODO: enable as estimatedCacheLineSize when tbbmalloc is available;
    }
}

// TODO: rework the test not to depend on oneTBB internals
static const size_t cache_allocator_mask = oneapi::tbb::detail::r1::cache_line_size();
static const size_t tbb_allocator_mask = init_tbb_alloc_mask();

void TestETSIterator() {
    using ets_type = oneapi::tbb::enumerable_thread_specific<int>;
    if (utils::get_platform_max_threads() == 1) {
        ets_type ets;
        ets.local() = 1;
        REQUIRE_MESSAGE(std::next(ets.begin()) == ets.end(), "Incorrect begin or end of the ETS");
        REQUIRE_MESSAGE(std::prev(ets.end()) == ets.begin(), "Incorrect begin or end of the ETS");
    } else {
        std::atomic<std::size_t> sync_counter(0);

        const std::size_t expected_ets_size = 2;
        ets_type ets;
        const ets_type& cets(ets);

        auto fill_ets_body = [&](){
            ets.local() = 42;
            ++sync_counter;
            while(sync_counter != expected_ets_size)
                utils::yield();
        };

        oneapi::tbb::parallel_invoke(fill_ets_body, fill_ets_body);
        REQUIRE_MESSAGE(ets.size() == expected_ets_size, "Incorrect ETS size");

        std::size_t counter = 0;
        auto iter = ets.begin();
        while(iter != ets.end()) {
            ++counter % 2 == 0 ? ++iter : iter++;
        }
        REQUIRE(counter == expected_ets_size);
        while(iter != ets.begin()) {
            --counter % 2 == 0 ? --iter : iter--;
        }
        REQUIRE(counter == 0);
        auto citer = cets.begin();
        while(citer != cets.end()) {
            ++counter % 2 == 0 ? ++citer : citer++;
        }
        REQUIRE(counter == expected_ets_size);
        while(citer != cets.begin()) {
            --counter % 2 == 0 ? --citer : citer--;
        }
        REQUIRE(counter == 0);
        REQUIRE(ets.begin() + expected_ets_size == ets.end());
        REQUIRE(expected_ets_size + ets.begin() == ets.end());
        REQUIRE(ets.end() - expected_ets_size == ets.begin());

        typename ets_type::iterator it;
        it = ets.begin();

        auto it_bkp = it;
        auto it2 = it++;
        REQUIRE(it2 == it_bkp);

        it = ets.begin();
        it += expected_ets_size;
        REQUIRE(it == ets.end());
        it -= expected_ets_size;
        REQUIRE(it == ets.begin());

        for (int i = 0; i < int(expected_ets_size - 1); ++i) {
            REQUIRE(ets.begin()[i] == 42);
            REQUIRE(std::prev(ets.end())[-i] == 42);
        }

        auto iter1 = ets.begin();
        auto iter2 = ets.end();
        REQUIRE(iter1 < iter2);
        REQUIRE(iter1 <= iter2);
        REQUIRE(!(iter1 > iter2));
        REQUIRE(!(iter1 >= iter2));
    }
}

template <bool ExpectEqual, bool ExpectLess, typename Iterator>
void DoETSIteratorComparisons( const Iterator& lhs, const Iterator& rhs ) {
    // TODO: replace with testEqualityAndLessComparisons after adding <=> operator for ETS iterator
    using namespace comparisons_testing;
    testEqualityComparisons<ExpectEqual>(lhs, rhs);
    testTwoWayComparisons<ExpectEqual, ExpectLess>(lhs, rhs);
}

template <typename Iterator, typename ETS>
void TestETSIteratorComparisonsBasic( ETS& ets ) {
    REQUIRE_MESSAGE(!ets.empty(), "Incorrect test setup");
    Iterator it1, it2;
    DoETSIteratorComparisons</*ExpectEqual = */true, /*ExpectLess = */false>(it1, it2);
    it1 = ets.begin();
    it2 = ets.begin();
    DoETSIteratorComparisons</*ExpectEqual = */true, /*ExpectLess = */false>(it1, it2);
    it2 = std::prev(ets.end());
    DoETSIteratorComparisons</*ExpectEqual = */false, /*ExpectLess = */true>(it1, it2);
}

void TestETSIteratorComparisons() {
    using ets_type = oneapi::tbb::enumerable_thread_specific<int>;
    ets_type ets;

    // Fill the ets
    const std::size_t expected_ets_size = 2;
    std::atomic<std::size_t> sync_counter(0);
    auto fill_ets_body = [&](int){
            ets.local() = 42;
            ++sync_counter;
            while(sync_counter != expected_ets_size)
                std::this_thread::yield();
        };

    utils::NativeParallelFor(2, fill_ets_body);

    TestETSIteratorComparisonsBasic<typename ets_type::iterator>(ets);
    const ets_type& cets = ets;
    TestETSIteratorComparisonsBasic<typename ets_type::const_iterator>(cets);
}

//! Test container instantiation
//! \brief \ref interface \ref requirement
TEST_CASE("Instantiation") {
    AlignMask = cache_allocator_mask;
    TestInstantiation<oneapi::tbb::cache_aligned_allocator>("oneapi::tbb::cache_aligned_allocator");
    AlignMask = tbb_allocator_mask;
    TestInstantiation<oneapi::tbb::tbb_allocator>("oneapi::tbb::tbb_allocator");
}

//! Test assignment and copy constructor
//! \brief \ref interface \ref requirement
TEST_CASE("Assignment and copy constructor") {
    AlignMask = cache_allocator_mask;
    run_assignment_and_copy_constructor_tests<oneapi::tbb::cache_aligned_allocator>("oneapi::tbb::cache_aligned_allocator");
    AlignMask = tbb_allocator_mask;
    run_assignment_and_copy_constructor_tests<oneapi::tbb::tbb_allocator>("oneapi::tbb::tbb_allocator");
}

//! Test for basic ETS functionality and requirements
//! \brief \ref interface \ref requirement
TEST_CASE("Basic ETS functionality") {
    const int LOCALS = 10;

    oneapi::tbb::enumerable_thread_specific<int> ets;
    ets.local() = 42;

    utils::SpinBarrier barrier(LOCALS);
    utils::NativeParallelFor(LOCALS, [&](int i) {
        barrier.wait();
        ets.local() = i;
        CHECK(ets.local() == i);
    });
    CHECK(ets.local() == 42);

    int ref_combined{0};
    std::vector<int> sequence(LOCALS);
    std::iota(sequence.begin(), sequence.end(), 0);
    for (int i : sequence) {
        ref_combined += i;
    }
    ref_combined += 42;
    int ets_combined = ets.combine([](int x, int y) {
        return x + y;
    });
    CHECK(ref_combined == ets_combined);
}

//! Test ETS usage in parallel algorithms.
//! Also tests flattened2d and flattend2d
//! \brief \ref interface \ref requirement \ref stress
TEST_CASE("Parallel test") {
    run_reference_check();
    AlignMask = cache_allocator_mask;
    run_parallel_tests<oneapi::tbb::cache_aligned_allocator>("oneapi::tbb::cache_aligned_allocator");
    AlignMask = tbb_allocator_mask;
    run_parallel_tests<oneapi::tbb::tbb_allocator>("oneapi::tbb::tbb_allocator");
    run_cross_type_tests();
}

//! \brief \ref interface \ref requirement
TEST_CASE("Member types") {
    TestMemberTypes();
}

//! \brief \ref interface \ref requirement
TEST_CASE("enumerable_thread_specific iterator") {
    TestETSIterator();
}

//! \brief \ref interface \ref requirement
TEST_CASE("enumerable_thread_specific iterator comparisons") {
    TestETSIteratorComparisons();
}
