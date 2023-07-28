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

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_report.h"
#include "common/spin_barrier.h"
#include "common/checktype.h"

#include "common/container_move_support.h"

#include "oneapi/tbb/combinable.h"
#include "oneapi/tbb/parallel_for.h"
#include "oneapi/tbb/blocked_range.h"
#include "oneapi/tbb/global_control.h"
#include "oneapi/tbb/tbb_allocator.h"
// INFO: #include "oneapi/tbb/tick_count.h"

#include <cstring>
#include <vector>
#include <utility>

//! \file conformance_combinable.cpp
//! \brief Test for [tls.combinable] specification

//! Minimum number of threads
const int MinThread = 1;

//! Maximum number of threads
const int MaxThread = 4;

static std::atomic<int> construction_counter;
static std::atomic<int> destruction_counter;

const int REPETITIONS = 10;
const int N = 100000;
const double EXPECTED_SUM = (REPETITIONS + 1) * N;

//
// A minimal class
// Define: default and copy constructor, and allow implicit operator&
// also operator=
//

class minimalCombinable {
private:
    int my_value;
public:
    minimalCombinable(int val=0) : my_value(val) { ++construction_counter; }
    minimalCombinable( const minimalCombinable&m ) : my_value(m.my_value) { ++construction_counter; }
    minimalCombinable& operator=(const minimalCombinable& other) { my_value = other.my_value; return *this; }
    minimalCombinable& operator+=(const minimalCombinable& other) { my_value += other.my_value; return *this; }
    operator int() const { return my_value; }
    ~minimalCombinable() { ++destruction_counter; }
    void set_value( const int i ) { my_value = i; }
    int value( ) const { return my_value; }
};

//// functors for initialization and combine

template <typename T>
struct FunctorAddFinit {
    T operator()() { return 0; }
};

template <typename T>
struct FunctorAddFinit7 {
    T operator()() { return 7; }
};

template <typename T>
struct FunctorAddCombine {
    T operator()(T left, T right ) const {
        return left + right;
    }
};

template <typename T>
struct FunctorAddCombineRef {
    T operator()(const T& left, const T& right ) const {
        return left + right;
    }
};

template <typename T>
T my_combine( T left, T right) { return left + right; }

template <typename T>
T my_combine_ref( const T &left, const T &right) { return left + right; }

template <typename T>
class CombineEachHelper {
public:
    CombineEachHelper(T& _result) : my_result(_result) {}
    void operator()(const T& new_bit) { my_result +=  new_bit; }
private:
    T& my_result;
};

template <typename T>
class CombineEachHelperCnt {
public:
    CombineEachHelperCnt(T& _result, int& _nbuckets) : my_result(_result), nBuckets(_nbuckets) {}
    void operator()(const T& new_bit) { my_result +=  new_bit; ++nBuckets; }
private:
    T& my_result;
    int& nBuckets;
};

template <typename T>
class CombineEachVectorHelper {
public:
    typedef std::vector<T, oneapi::tbb::tbb_allocator<T> > ContainerType;
    CombineEachVectorHelper(T& _result) : my_result(_result) { }
    void operator()(const ContainerType& new_bit) {
        for(typename ContainerType::const_iterator ci = new_bit.begin(); ci != new_bit.end(); ++ci) {
            my_result +=  *ci;
        }
    }

private:
    T& my_result;
};

//// end functors

// parallel body with a test for first access
template <typename T>
class ParallelScalarBody: utils::NoAssign {

    oneapi::tbb::combinable<T> &sums;

public:

    ParallelScalarBody ( oneapi::tbb::combinable<T> &_sums ) : sums(_sums) { }

    void operator()( const oneapi::tbb::blocked_range<int> &r ) const {
        for (int i = r.begin(); i != r.end(); ++i) {
            bool was_there;
            T& my_local = sums.local(was_there);
            if(!was_there) my_local = 0;
             my_local +=  1 ;
        }
    }

};

// parallel body with no test for first access
template <typename T>
class ParallelScalarBodyNoInit: utils::NoAssign {

    oneapi::tbb::combinable<T> &sums;

public:

    ParallelScalarBodyNoInit ( oneapi::tbb::combinable<T> &_sums ) : sums(_sums) { }

    void operator()( const oneapi::tbb::blocked_range<int> &r ) const {
        for (int i = r.begin(); i != r.end(); ++i) {
             sums.local() +=  1 ;
        }
    }

};

template< typename T >
void RunParallelScalarTests(const char* /* test_name */) {
    for (int p = MinThread; p <= MaxThread; ++p) {

        if (p == 0) continue;
        // REMARK("  Testing parallel %s on %d thread(s)...\n", test_name, p);
        oneapi::tbb::global_control gc(oneapi::tbb::global_control::max_allowed_parallelism, p);

        // INFO: oneapi::tbb::tick_count t0;
        T combine_sum(0);
        T combine_ref_sum(0);
        T combine_finit_sum(0);
        T combine_each_sum(0);
        T copy_construct_sum(0);
        T copy_assign_sum(0);
        T move_construct_sum(0);
        T move_assign_sum(0);

        for (int t = -1; t < REPETITIONS; ++t) {
            // INFO: if (Verbose && t == 0) t0 = oneapi::tbb::tick_count::now();

            // test uninitialized parallel combinable
            oneapi::tbb::combinable<T> sums;
            oneapi::tbb::parallel_for( oneapi::tbb::blocked_range<int>( 0, N, 10000 ), ParallelScalarBody<T>( sums ) );
            combine_sum += sums.combine(my_combine<T>);
            combine_ref_sum += sums.combine(my_combine_ref<T>);

            // test combinable::clear()
            oneapi::tbb::combinable<T> sums_to_clear;
            oneapi::tbb::parallel_for( oneapi::tbb::blocked_range<int>(0, N, 10000), ParallelScalarBody<T>(sums_to_clear) );
            sums_to_clear.clear();
            CHECK_MESSAGE(sums_to_clear.combine(my_combine<T>) == 0, "Failed combinable::clear test");

            // test parallel combinable preinitialized with a functor that returns 0
            FunctorAddFinit<T> my_finit_decl;
            oneapi::tbb::combinable<T> finit_combinable(my_finit_decl);
            oneapi::tbb::parallel_for( oneapi::tbb::blocked_range<int>( 0, N, 10000 ), ParallelScalarBodyNoInit<T>( finit_combinable ) );
            combine_finit_sum += finit_combinable.combine(my_combine<T>);

            // test another way of combining the elements using CombineEachHelper<T> functor
            CombineEachHelper<T> my_helper(combine_each_sum);
            sums.combine_each(my_helper);

            // test copy constructor for parallel combinable
            oneapi::tbb::combinable<T> copy_constructed(sums);
            copy_construct_sum += copy_constructed.combine(my_combine<T>);

            // test copy assignment for uninitialized parallel combinable
            oneapi::tbb::combinable<T> assigned;
            assigned = sums;
            copy_assign_sum += assigned.combine(my_combine<T>);

            // test move constructor for parallel combinable
            oneapi::tbb::combinable<T> moved1(std::move(sums));
            move_construct_sum += moved1.combine(my_combine<T>);

            // test move assignment for uninitialized parallel combinable
            oneapi::tbb::combinable<T> moved2;
            moved2=std::move(finit_combinable);
            move_assign_sum += moved2.combine(my_combine<T>);
        }
        // Here and below comparison for equality of float numbers succeeds
        // as the rounding error doesn't accumulate and doesn't affect the comparison
        REQUIRE( EXPECTED_SUM == combine_sum );
        REQUIRE( EXPECTED_SUM == combine_ref_sum );
        REQUIRE( EXPECTED_SUM == combine_finit_sum );
        REQUIRE( EXPECTED_SUM == combine_each_sum );
        REQUIRE( EXPECTED_SUM == copy_construct_sum );
        REQUIRE( EXPECTED_SUM == copy_assign_sum );
        REQUIRE( EXPECTED_SUM == move_construct_sum );
        REQUIRE( EXPECTED_SUM == move_assign_sum );
        // REMARK("  done parallel %s, %d, %g, %g\n", test_name, p, static_cast<double>(combine_sum), ( oneapi::tbb::tick_count::now() - t0).seconds());
    }
}

template <typename T>
class ParallelVectorForBody: utils::NoAssign {

    oneapi::tbb::combinable< std::vector<T, oneapi::tbb::tbb_allocator<T> > > &locals;

public:

    ParallelVectorForBody ( oneapi::tbb::combinable< std::vector<T, oneapi::tbb::tbb_allocator<T> > > &_locals ) : locals(_locals) { }

    void operator()( const oneapi::tbb::blocked_range<int> &r ) const {
        T one = 1;

        for (int i = r.begin(); i < r.end(); ++i) {
            locals.local().push_back( one );
        }
    }

};

template< typename T >
void RunParallelVectorTests(const char* /* test_name */) {
    typedef std::vector<T, oneapi::tbb::tbb_allocator<T> > ContainerType;

    for (int p = MinThread; p <= MaxThread; ++p) {

        if (p == 0) continue;
        // REMARK("  Testing parallel %s on %d thread(s)... \n", test_name, p);
        oneapi::tbb::global_control gc(oneapi::tbb::global_control::max_allowed_parallelism, p);

        // INFO: oneapi::tbb::tick_count t0;
        T defaultConstructed_sum(0);
        T copyConstructed_sum(0);
        T copyAssigned_sum(0);
        T moveConstructed_sum(0);
        T moveAssigned_sum(0);

        for (int t = -1; t < REPETITIONS; ++t) {
            // if (Verbose && t == 0) t0 = oneapi::tbb::tick_count::now();

            typedef typename oneapi::tbb::combinable< ContainerType > CombinableType;

            // test uninitialized parallel combinable
            CombinableType vs;
            oneapi::tbb::parallel_for( oneapi::tbb::blocked_range<int> (0, N, 10000), ParallelVectorForBody<T>( vs ) );
            CombineEachVectorHelper<T> MyCombineEach(defaultConstructed_sum);
            vs.combine_each(MyCombineEach); // combine_each sums all elements of each vector into the result

            // test copy constructor for parallel combinable with vectors
            CombinableType vs2(vs);
            CombineEachVectorHelper<T> MyCombineEach2(copyConstructed_sum);
            vs2.combine_each(MyCombineEach2);

            // test copy assignment for uninitialized parallel combinable with vectors
            CombinableType vs3;
            vs3 = vs;
            CombineEachVectorHelper<T> MyCombineEach3(copyAssigned_sum);
            vs3.combine_each(MyCombineEach3);

            // test move constructor for parallel combinable with vectors
            CombinableType vs4(std::move(vs2));
            CombineEachVectorHelper<T> MyCombineEach4(moveConstructed_sum);
            vs4.combine_each(MyCombineEach4);

            // test move assignment for uninitialized parallel combinable with vectors
            vs4=std::move(vs3);
            CombineEachVectorHelper<T> MyCombineEach5(moveAssigned_sum);
            vs4.combine_each(MyCombineEach5);
        }

        double ResultValue = defaultConstructed_sum;
        REQUIRE( EXPECTED_SUM == ResultValue );
        ResultValue = copyConstructed_sum;
        REQUIRE( EXPECTED_SUM == ResultValue );
        ResultValue = copyAssigned_sum;
        REQUIRE( EXPECTED_SUM == ResultValue );
        ResultValue = moveConstructed_sum;
        REQUIRE( EXPECTED_SUM == ResultValue );
        ResultValue = moveAssigned_sum;
        REQUIRE( EXPECTED_SUM == ResultValue );

        // REMARK("  done parallel %s, %d, %g, %g\n", test_name, p, ResultValue, ( oneapi::tbb::tick_count::now() - t0).seconds());
    }
}

void
RunParallelTests() {
    // REMARK("Running RunParallelTests\n");
    RunParallelScalarTests<int>("int");
    RunParallelScalarTests<double>("double");
    RunParallelScalarTests<minimalCombinable>("minimalCombinable");
    RunParallelVectorTests<int>("std::vector<int, oneapi::tbb::tbb_allocator<int> >");
    RunParallelVectorTests<double>("std::vector<double, oneapi::tbb::tbb_allocator<double> >");
}

template <typename T>
void
RunAssignmentAndCopyConstructorTest(const char* /* test_name */) {
    // REMARK("  Testing assignment and copy construction for combinable<%s>...\n", test_name);

    // test creation with finit function (combine returns finit return value if no threads have created locals)
    FunctorAddFinit7<T> my_finit7_decl;
    oneapi::tbb::combinable<T> create1(my_finit7_decl);
    REQUIRE_MESSAGE(7 == create1.combine(my_combine<T>), "Unexpected combine result for combinable object preinitialized with functor");

    // test copy construction with function initializer
    oneapi::tbb::combinable<T> copy1(create1);
    REQUIRE_MESSAGE(7 == copy1.combine(my_combine<T>), "Unexpected combine result for copy-constructed combinable object");

    // test copy assignment with function initializer
    FunctorAddFinit<T> my_finit_decl;
    oneapi::tbb::combinable<T> assign1(my_finit_decl);
    assign1 = create1;
    REQUIRE_MESSAGE(7 == assign1.combine(my_combine<T>), "Unexpected combine result for copy-assigned combinable object");

    // test move construction with function initializer
    oneapi::tbb::combinable<T> move1(std::move(create1));
    REQUIRE_MESSAGE(7 == move1.combine(my_combine<T>), "Unexpected combine result for move-constructed combinable object");

    // test move assignment with function initializer
    oneapi::tbb::combinable<T> move2;
    move2=std::move(copy1);
    REQUIRE_MESSAGE(7 == move2.combine(my_combine<T>), "Unexpected combine result for move-assigned combinable object");

    // REMARK("  done\n");

}

void RunAssignmentAndCopyConstructorTests() {
    // REMARK("Running assignment and copy constructor tests:\n");
    RunAssignmentAndCopyConstructorTest<int>("int");
    RunAssignmentAndCopyConstructorTest<double>("double");
    RunAssignmentAndCopyConstructorTest<minimalCombinable>("minimalCombinable");
}

void RunMoveSemanticsForStateTrackableObjectTest() {
    // REMARK("Testing move assignment and move construction for combinable<Harness::StateTrackable>...\n");

    oneapi::tbb::combinable< StateTrackable<true> > create1;
    REQUIRE_MESSAGE(create1.local().state == StateTrackable<true>::DefaultInitialized,
           "Unexpected value in default combinable object");

    // Copy constructing of the new combinable causes copying of stored values
    oneapi::tbb::combinable< StateTrackable<true> > copy1(create1);
    REQUIRE_MESSAGE(copy1.local().state == StateTrackable<true>::CopyInitialized,
           "Unexpected value in copy-constructed combinable object");

    // Copy assignment also causes copying of stored values
    oneapi::tbb::combinable< StateTrackable<true> > copy2;
    REQUIRE_MESSAGE(copy2.local().state == StateTrackable<true>::DefaultInitialized,
           "Unexpected value in default combinable object");
    copy2=create1;
    REQUIRE_MESSAGE(copy2.local().state == StateTrackable<true>::CopyInitialized,
           "Unexpected value in copy-assigned combinable object");

    // Store some marked values in the initial combinable object
    create1.local().state = StateTrackableBase::Unspecified;

    // Move constructing of the new combinable must not cause copying of stored values
    oneapi::tbb::combinable< StateTrackable<true> > move1(std::move(create1));
    REQUIRE_MESSAGE(move1.local().state == StateTrackableBase::Unspecified, "Unexpected value in move-constructed combinable object");

    // Move assignment must not cause copying of stored values
    copy1=std::move(move1);
    REQUIRE_MESSAGE(copy1.local().state == StateTrackableBase::Unspecified, "Unexpected value in move-assigned combinable object");

    // Make the stored values valid again in order to delete StateTrackable object correctly
    copy1.local().state = StateTrackable<true>::MoveAssigned;

    // REMARK("done\n");
}

utils::SpinBarrier sBarrier;

struct Body : utils::NoAssign {
    oneapi::tbb::combinable<int>* locals;
    const int nthread;
    const int nIters;
    Body( int nthread_, int niters_ ) : nthread(nthread_), nIters(niters_) { sBarrier.initialize(nthread_); }

    void operator()(int thread_id ) const {
        bool existed;
        sBarrier.wait();
        for(int i = 0; i < nIters; ++i ) {
            existed = thread_id & 1;
            int oldval = locals->local(existed);
            REQUIRE_MESSAGE(existed == (i > 0), "Error on first reference");
            REQUIRE_MESSAGE((!existed || (oldval == thread_id)), "Error on fetched value");
            existed = thread_id & 1;
            locals->local(existed) = thread_id;
            REQUIRE_MESSAGE(existed, "Error on assignment");
        }
    }
};

void TestLocalAllocations( int nthread ) {
    REQUIRE_MESSAGE(nthread > 0, "nthread must be positive");
#define NITERATIONS 1000
    Body myBody(nthread, NITERATIONS);
    oneapi::tbb::combinable<int> myCombinable;
    myBody.locals = &myCombinable;

    NativeParallelFor( nthread, myBody );

    int mySum = 0;
    int mySlots = 0;
    CombineEachHelperCnt<int> myCountCombine(mySum, mySlots);
    myCombinable.combine_each(myCountCombine);

    REQUIRE_MESSAGE(nthread == mySlots, "Incorrect number of slots");
    REQUIRE_MESSAGE(mySum == (nthread - 1) * nthread / 2, "Incorrect values in result");
}

void RunLocalAllocationsTests() {
    // REMARK("Testing local() allocations\n");
    for(int i = 1 <= MinThread ? MinThread : 1; i <= MaxThread; ++i) {
        // REMARK("  Testing local() allocation with nthreads=%d...\n", i);
        for(int j = 0; j < 100; ++j) {
            TestLocalAllocations(i);
        }
        // REMARK("  done\n");
    }
}

//! Test combinable in parallel algorithms
//! \brief \ref interface \ref requirement
TEST_CASE("Parallel scenario") {
    RunParallelTests();
    RunLocalAllocationsTests();
}

//! Test assignment and copy construction
//! \brief \ref interface \ref requirement
TEST_CASE("Assignment and copy constructor test") {
    RunAssignmentAndCopyConstructorTests();
}

//! Test move support
//! \brief \ref interface \ref requirement
TEST_CASE("Move semantics") {
    RunMoveSemanticsForStateTrackableObjectTest();
}

