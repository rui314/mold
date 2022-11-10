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

#include "common/test.h"
#include "common/config.h"
#include "common/utils_concurrency_limit.h"
#include "common/cpu_usertime.h"
#include "common/concepts_common.h"

#include "tbb/global_control.h"
#include "tbb/parallel_scan.h"
#include "tbb/blocked_range.h"
#include "tbb/tick_count.h"
#include <vector>
#include <atomic>

//! \file test_parallel_scan.cpp
//! \brief Test for [algorithms.parallel_scan] specification

using Range = tbb::blocked_range<long>;

static volatile bool ScanIsRunning = false;

//! Sum of 0..i with wrap around on overflow.
inline int TriangularSum( int i ) {
    return i&1 ? ((i>>1)+1)*i : (i>>1)*(i+1);
}

//! Verify that sum is init plus sum of integers in closed interval [0..finish_index].
/** line should be the source line of the caller */
void VerifySum( int init, long finish_index, int sum, int line ) {
    int expected = init + TriangularSum(finish_index);
    CHECK_MESSAGE(expected == sum, "line " << line << ": sum[0.." << finish_index << "] should be = " << expected << ", but was computed as " << sum << "\n");
}

const int MAXN = 20000;

enum AddendFlag {
    UNUSED=0,
    USED_NONFINAL=1,
    USED_FINAL=2
};

//! Array recording how each addend was used.
/** 'unsigned char' instead of AddendFlag for sake of compactness. */
static unsigned char AddendHistory[MAXN];

std::atomic<long> NumberOfLiveStorage;

template<typename T>
struct Storage {
    T my_total;
    Range my_range;
    Storage(T init) :
        my_total(init), my_range(-1, -1, 1) {
        ++NumberOfLiveStorage;
    }
    ~Storage() {
        --NumberOfLiveStorage;
    }
    Storage(const Storage& strg) :
        my_total(strg.my_total), my_range(strg.my_range) {
        ++NumberOfLiveStorage;
    }
    Storage & operator=(const Storage& strg) {
        my_total = strg.my_total;
        my_range = strg.my_range;
        return *this;
    }
};

template<typename T>
Storage<T> JoinStorages(const Storage<T>& left, const Storage<T>& right) {
    Storage<T> result = right;
    CHECK(ScanIsRunning);
    CHECK(left.my_range.end() == right.my_range.begin());
    result.my_total += left.my_total;
    result.my_range = Range(left.my_range.begin(), right.my_range.end(), 1);
    CHECK(ScanIsRunning);
    return result;
}

template<typename T>
void Scan(const Range & r, bool is_final, Storage<T> & storage, std::vector<T> & sum, const std::vector<T> & addend) {
    CHECK((!is_final || (storage.my_range.begin() == 0 && storage.my_range.end() == r.begin()) || (storage.my_range.empty() && r.begin() == 0)));
    for (long i = r.begin(); i < r.end(); ++i) {
        storage.my_total += addend[i];
        if (is_final) {
            CHECK_MESSAGE(AddendHistory[i] < USED_FINAL, "addend used 'finally' twice?");
            AddendHistory[i] |= USED_FINAL;
            sum[i] = storage.my_total;
            VerifySum(42, i, int(sum[i]), __LINE__);
        }
        else {
            CHECK_MESSAGE(AddendHistory[i] == UNUSED, "addend used too many times");
            AddendHistory[i] |= USED_NONFINAL;
        }
    }
    if (storage.my_range.empty())
        storage.my_range = r;
    else
        storage.my_range = Range(storage.my_range.begin(), r.end(), 1);
}

template<typename T>
Storage<T> ScanWithInit(const Range & r, T init, bool is_final, Storage<T> & storage, std::vector<T> & sum, const std::vector<T> & addend) {
    if (r.begin() == 0)
        storage.my_total = init;
    Scan(r, is_final, storage, sum, addend);
    return storage;
}

template<typename T>
class Accumulator {
    const  std::vector<T> &my_array;
    std::vector<T> & my_sum;
    Storage<T> storage;
    enum state_type {
        full,       // Accumulator has sufficient information for final scan,
                    // i.e. has seen all iterations to its left.
                    // It's either the original Accumulator provided by the user
                    // or a Accumulator constructed by a splitting constructor *and* subsequently
                    // subjected to a reverse_join with a full accumulator.

        partial,    // Accumulator has only enough information for pre_scan.
                    // i.e. has not seen all iterations to its left.
                    // It's an Accumulator created by a splitting constructor that
                    // has not yet been subjected to a reverse_join with a full accumulator.

        summary,    // Accumulator has summary of iterations processed, but not necessarily
                    // the information required for a final_scan or pre_scan.
                    // It's the result of "assign".

        trash       // Accumulator with possibly no useful information.
                    // It was the source for "assign".

    };
    mutable state_type my_state;
    //! Equals this while object is fully constructed, nullptr otherwise.
    /** Used to detect premature destruction and accidental bitwise copy. */
    Accumulator* self;
    Accumulator& operator= (const Accumulator& other);
public:
    Accumulator( T init, const std::vector<T> & array, std::vector<T> & sum ) :
        my_array(array), my_sum(sum), storage(init), my_state(full)
    {
        // Set self as last action of constructor, to indicate that object is fully constructed.
        self = this;
    }
    ~Accumulator() {
        // Clear self as first action of destructor, to indicate that object is not fully constructed.
        self = nullptr;
    }
    Accumulator( Accumulator& a, tbb::split ) :
        my_array(a.my_array), my_sum(a.my_sum), storage(0), my_state(partial)
    {
        if (!(a.my_state == partial))
            CHECK(a.my_state == full);
        if (!(a.my_state == full))
            CHECK(a.my_state == partial);
        CHECK(ScanIsRunning);
        // Set self as last action of constructor, to indicate that object is fully constructed.
        self = this;
    }
    template<typename Tag>
    void operator()( const Range& r, Tag /*tag*/ ) {
        if(Tag::is_final_scan())
            CHECK(my_state == full);
        else
            CHECK(my_state == partial);
        Scan(r, Tag::is_final_scan(), storage, my_sum, my_array);
        CHECK_MESSAGE(self==this, "this Accumulator corrupted or prematurely destroyed");
    }
    void reverse_join( const Accumulator& left_body) {
        const Storage<T> & left = left_body.storage;
        Storage<T> & right = storage;
        CHECK(my_state == partial);
        CHECK( ((left_body.my_state == full) || (left_body.my_state==partial)) );

        right = JoinStorages(left, right);

        CHECK(left_body.self == &left_body);
        my_state = left_body.my_state;
    }
    void assign( const Accumulator& other ) {
        CHECK(other.my_state == full);
        CHECK(my_state == full);
        storage.my_total = other.storage.my_total;
        storage.my_range = other.storage.my_range;
        CHECK(self == this);
        CHECK_MESSAGE(other.self==&other, "other Accumulator corrupted or prematurely destroyed");
        my_state = summary;
        other.my_state = trash;
    }
    T get_total() {
        return storage.my_total;
    }
};


template<typename T, typename Scan, typename ReverseJoin>
T ParallelScanFunctionalInvoker(const Range& range, T idx, const Scan& scan, const ReverseJoin& reverse_join, int mode) {
    switch (mode%3) {
    case 0:
        return tbb::parallel_scan(range, idx, scan, reverse_join);
        break;
    case 1:
        return tbb::parallel_scan(range, idx, scan, reverse_join, tbb::simple_partitioner());
        break;
    default:
        return tbb::parallel_scan(range, idx, scan, reverse_join, tbb::auto_partitioner());
    }
}

template<typename T>
class ScanBody {
    const std::vector<T> &my_addend;
    std::vector<T> &my_sum;
    const T my_init;
    ScanBody& operator= (const ScanBody&);
public:
    ScanBody(T init, const std::vector<T> &addend, std::vector<T> &sum) :my_addend(addend), my_sum(sum), my_init(init) {}
    template<typename S, typename Tag>
    Storage<S> operator()(const Range& r, Storage<S> storage, Tag) const {
        return ScanWithInit(r, my_init, Tag::is_final_scan(), storage, my_sum, my_addend);
    }
};

class JoinBody {
public:
    template<typename T>
    Storage<T> operator()(const Storage<T>& left, const Storage<T>& right) const {
        return JoinStorages(left, right);
    }
};

struct ParallelScanTemplateFunctor {
    template<typename T>
    T operator()(Range range, T init, const std::vector<T> &addend, std::vector<T> &sum, int mode) {
        for (long i = 0; i<MAXN; ++i) {
            AddendHistory[i] = UNUSED;
        }
        ScanIsRunning = true;
        ScanBody<T> sb(init, addend, sum);
        JoinBody jb;
        Storage<T> res = ParallelScanFunctionalInvoker(range, Storage<T>(0), sb, jb, mode);
        ScanIsRunning = false;
        if (range.empty())
            res.my_total = init;
        return res.my_total;
    }
};

struct ParallelScanLambda {
    template<typename T>
    T operator()(Range range, T init, const std::vector<T> &addend, std::vector<T> &sum, int mode) {
        for (long i = 0; i<MAXN; ++i) {
            AddendHistory[i] = UNUSED;
        }
        ScanIsRunning = true;
        Storage<T> res = ParallelScanFunctionalInvoker(range, Storage<T>(0),
            [&addend, &sum, init](const Range& r, Storage<T> storage, bool is_final_scan /*tag*/) -> Storage<T> {
                return ScanWithInit(r, init, is_final_scan, storage, sum, addend);
            },
            [](const Storage<T>& left, const Storage<T>& right) -> Storage<T> {
                return JoinStorages(left, right);
            },
            mode);
        ScanIsRunning = false;
        if (range.empty())
            res.my_total = init;
        return res.my_total;
    }
};

void TestAccumulator( int mode ) {
    typedef int T;
    std::vector<T> addend(MAXN);
    std::vector<T> sum(MAXN);
    std::vector<T> control_sum(MAXN);
    T control_total;
    for( int n=0; n<=MAXN; n = n <=128? n+1: n*3) {
        for( int gs : {1, 2, 100, 511, 12345, n/ 111, n/17, n-1, n}) {
            if(gs<=0 || gs > n)
                continue;
            control_total = 42;
            for( long i=0; i<MAXN; ++i ) {
                addend[i] = -1;
                sum[i] = -2;
                control_sum[i] = -2;
                AddendHistory[i] = UNUSED;
            }
            for (long i = 0; i<n; ++i) {
                addend[i] = i;
                control_total += addend[i];
                control_sum[i] = control_total;
            }

            Accumulator<T> acc( 42, addend, sum);
            ScanIsRunning = true;

            switch (mode) {
                case 0:
                    tbb::parallel_scan( Range( 0, n,  gs ), acc );
                break;
                case 1:
                    tbb::parallel_scan( Range( 0, n, gs ), acc, tbb::simple_partitioner() );
                break;
                case 2:
                    tbb::parallel_scan( Range( 0, n, gs ), acc, tbb::auto_partitioner() );
                break;
            }

            ScanIsRunning = false;

            for( long i=0; i<n; ++i )
                CHECK_MESSAGE((AddendHistory[i]&USED_FINAL), "failed to use addend[" << i << "] " << (AddendHistory[i] & USED_NONFINAL ? "(but used nonfinal)\n" : "\n"));
            for( long i=0; i<n; ++i ) {
                VerifySum( 42, i, sum[i], __LINE__ );
            }
            if( n )
                CHECK(acc.get_total()==sum[n-1]);
            else
                CHECK(acc.get_total()==42);
            CHECK(control_total ==acc.get_total());
            CHECK(control_sum==sum);
        }
    }
}

template<typename ParallelScanWrapper>
void TestInterface( int mode, ParallelScanWrapper parallel_scan_wrapper ) {
    using T = int;
    std::vector<T> addend(MAXN);
    std::vector<T> control_sum(MAXN);
    T control_total(42);
    for( long i=0; i<MAXN; ++i ) {
        addend[i] = i;
        control_total += addend[i];
        control_sum[i] = control_total;
        AddendHistory[i] = UNUSED;
    }

    std::vector<T> sum(MAXN);
    for (long i = 0; i<MAXN; ++i)
        sum[i] = -2;
    ScanIsRunning = true;
    T total = parallel_scan_wrapper(Range(0, MAXN, 1), 42, addend, sum, mode);
    ScanIsRunning = false;

    CHECK_MESSAGE(control_total==total, "Parallel prefix sum is not equal to serial");
    CHECK_MESSAGE(control_sum==sum, "Parallel prefix vector is not equal to serial");
}


#if __TBB_CPP14_GENERIC_LAMBDAS_PRESENT
struct ParallelScanGenericLambda {
    template<typename T>
    T operator()(Range range, T init, const std::vector<T> &addend, std::vector<T> &sum, int mode) {
        for (long i = 0; i<MAXN; ++i) {
            AddendHistory[i] = UNUSED;
        }
        ScanIsRunning = true;
        Storage<T> res = ParallelScanFunctionalInvoker(range, Storage<T>(0),
            [&addend, &sum, init](const auto& rng, auto storage, bool is_final_scan) {
                return ScanWithInit(rng, init, is_final_scan, storage, sum, addend);
            },
            [](const auto& left, const auto& right) {
                return JoinStorages(left, right);
            },
            mode);
        ScanIsRunning = false;
        if (range.empty())
            res.my_total = init;
        return res.my_total;
    }
};
#endif /* __TBB_CPP14_GENERIC_LAMBDAS_PRESENT */

#if __TBB_CPP20_CONCEPTS_PRESENT
template <typename... Args>
concept can_call_parallel_scan_basic = requires( Args&&... args ) {
    tbb::parallel_scan(std::forward<Args>(args)...);
};

template <typename Range, typename Body>
concept can_call_imperative_pscan = can_call_parallel_scan_basic<const Range&, Body&> &&
                                    can_call_parallel_scan_basic<const Range&, Body&, const tbb::simple_partitioner&> &&
                                    can_call_parallel_scan_basic<const Range&, Body&, const tbb::auto_partitioner&>;

template <typename Range, typename Value, typename Func, typename Combine>
concept can_call_functional_pscan = can_call_parallel_scan_basic<const Range&, const Value&, const Func&, const Combine&> &&
                                    can_call_parallel_scan_basic<const Range&, const Value&, const Func&, const Combine&, const tbb::simple_partitioner&> &&
                                    can_call_parallel_scan_basic<const Range&, const Value&, const Func&, const Combine&, const tbb::auto_partitioner&>;

using CorrectRange = test_concepts::range::Correct;

template <typename Range>
using CorrectBody = test_concepts::parallel_scan_body::Correct<Range>;

template <typename Range, typename T>
using CorrectFunc = test_concepts::parallel_scan_function::Correct<Range, T>;

template <typename T>
using CorrectCombine = test_concepts::parallel_scan_combine::Correct<T>;

void test_pscan_range_constraints() {
    using namespace test_concepts::range;
    static_assert(can_call_imperative_pscan<Correct, CorrectBody<Correct>>);
    static_assert(!can_call_imperative_pscan<NonCopyable, CorrectBody<NonCopyable>>);
    static_assert(!can_call_imperative_pscan<NonDestructible, CorrectBody<NonDestructible>>);
    static_assert(!can_call_imperative_pscan<NonSplittable, CorrectBody<NonSplittable>>);
    static_assert(!can_call_imperative_pscan<NoEmpty, CorrectBody<NoEmpty>>);
    static_assert(!can_call_imperative_pscan<EmptyNonConst, CorrectBody<EmptyNonConst>>);
    static_assert(!can_call_imperative_pscan<WrongReturnEmpty, CorrectBody<WrongReturnEmpty>>);
    static_assert(!can_call_imperative_pscan<NoIsDivisible, CorrectBody<NoIsDivisible>>);
    static_assert(!can_call_imperative_pscan<IsDivisibleNonConst, CorrectBody<IsDivisibleNonConst>>);
    static_assert(!can_call_imperative_pscan<WrongReturnIsDivisible, CorrectBody<WrongReturnIsDivisible>>);

    static_assert(can_call_functional_pscan<Correct, int, CorrectFunc<Correct, int>, CorrectCombine<int>>);
    static_assert(!can_call_functional_pscan<NonCopyable, int, CorrectFunc<NonCopyable, int>, CorrectCombine<int>>);
    static_assert(!can_call_functional_pscan<NonDestructible, int, CorrectFunc<NonDestructible, int>, CorrectCombine<int>>);
    static_assert(!can_call_functional_pscan<NonSplittable, int, CorrectFunc<NonSplittable, int>, CorrectCombine<int>>);
    static_assert(!can_call_functional_pscan<NoEmpty, int, CorrectFunc<NoEmpty, int>, CorrectCombine<int>>);
    static_assert(!can_call_functional_pscan<EmptyNonConst, int, CorrectFunc<EmptyNonConst, int>, CorrectCombine<int>>);
    static_assert(!can_call_functional_pscan<WrongReturnEmpty, int, CorrectFunc<WrongReturnEmpty, int>, CorrectCombine<int>>);
    static_assert(!can_call_functional_pscan<NoIsDivisible, int, CorrectFunc<NoIsDivisible, int>, CorrectCombine<int>>);
    static_assert(!can_call_functional_pscan<IsDivisibleNonConst, int, CorrectFunc<IsDivisibleNonConst, int>, CorrectCombine<int>>);
    static_assert(!can_call_functional_pscan<WrongReturnIsDivisible, int, CorrectFunc<WrongReturnIsDivisible, int>, CorrectCombine<int>>);
}

void test_pscan_body_constraints() {
    using namespace test_concepts::parallel_scan_body;
    static_assert(can_call_imperative_pscan<CorrectRange, Correct<CorrectRange>>);
    static_assert(!can_call_imperative_pscan<CorrectRange, NonSplittable<CorrectRange>>);
    static_assert(!can_call_imperative_pscan<CorrectRange, NoPreScanOperatorRoundBrackets<CorrectRange>>);
    static_assert(!can_call_imperative_pscan<CorrectRange, WrongFirstInputPreScanOperatorRoundBrackets<CorrectRange>>);
    static_assert(!can_call_imperative_pscan<CorrectRange, WrongSecondInputPreScanOperatorRoundBrackets<CorrectRange>>);
    static_assert(!can_call_imperative_pscan<CorrectRange, NoFinalScanOperatorRoundBrackets<CorrectRange>>);
    static_assert(!can_call_imperative_pscan<CorrectRange, WrongFirstInputFinalScanOperatorRoundBrackets<CorrectRange>>);
    static_assert(!can_call_imperative_pscan<CorrectRange, WrongSecondInputFinalScanOperatorRoundBrackets<CorrectRange>>);
    static_assert(!can_call_imperative_pscan<CorrectRange, NoReverseJoin<CorrectRange>>);
    static_assert(!can_call_imperative_pscan<CorrectRange, WrongInputReverseJoin<CorrectRange>>);
    static_assert(!can_call_imperative_pscan<CorrectRange, NoAssign<CorrectRange>>);
    static_assert(!can_call_imperative_pscan<CorrectRange, WrongInputAssign<CorrectRange>>);
}

void test_pscan_func_constraints() {
    using namespace test_concepts::parallel_scan_function;
    static_assert(can_call_functional_pscan<CorrectRange, int, Correct<CorrectRange, int>, CorrectCombine<int>>);
    static_assert(!can_call_functional_pscan<CorrectRange, int, NoOperatorRoundBrackets<CorrectRange, int>, CorrectCombine<int>>);
    static_assert(!can_call_functional_pscan<CorrectRange, int, OperatorRoundBracketsNonConst<CorrectRange, int>, CorrectCombine<int>>);
    static_assert(!can_call_functional_pscan<CorrectRange, int, WrongFirstInputOperatorRoundBrackets<CorrectRange, int>, CorrectCombine<int>>);
    static_assert(!can_call_functional_pscan<CorrectRange, int, WrongSecondInputOperatorRoundBrackets<CorrectRange, int>, CorrectCombine<int>>);
    static_assert(!can_call_functional_pscan<CorrectRange, int, WrongReturnOperatorRoundBrackets<CorrectRange, int>, CorrectCombine<int>>);
}

void test_pscan_combine_constraints() {
    using namespace test_concepts::parallel_scan_combine;
    static_assert(can_call_functional_pscan<CorrectRange, int, CorrectFunc<CorrectRange, int>, Correct<int>>);
    static_assert(!can_call_functional_pscan<CorrectRange, int, CorrectFunc<CorrectRange, int>, NoOperatorRoundBrackets<int>>);
    static_assert(!can_call_functional_pscan<CorrectRange, int, CorrectFunc<CorrectRange, int>, OperatorRoundBracketsNonConst<int>>);
    static_assert(!can_call_functional_pscan<CorrectRange, int, CorrectFunc<CorrectRange, int>, WrongFirstInputOperatorRoundBrackets<int>>);
    static_assert(!can_call_functional_pscan<CorrectRange, int, CorrectFunc<CorrectRange, int>, WrongSecondInputOperatorRoundBrackets<int>>);
    static_assert(!can_call_functional_pscan<CorrectRange, int, CorrectFunc<CorrectRange, int>, WrongReturnOperatorRoundBrackets<int>>);
}

#endif // __TBB_CPP20_CONCEPTS_PRESENT

// Test for parallel_scan with with different partitioners
//! \brief \ref error_guessing \ref resource_usage
TEST_CASE("parallel_scan testing with different partitioners") {
    for (auto concurrency_level : utils::concurrency_range()) {
        tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_level);
        for (int mode = 0; mode < 3; mode++) {
            NumberOfLiveStorage = 0;
            TestAccumulator(mode);
            // Test that all workers sleep when no work
            TestCPUUserTime(concurrency_level);

            // Checking has to be done late, because when parallel_scan makes copies of
            // the user's "Body", the copies might be destroyed slightly after parallel_scan
            // returns.
            CHECK(NumberOfLiveStorage == 0);
        }
    }
}

// Test for parallel_scan with template functors
//! \brief \ref error_guessing \ref interface \ref resource_usage
TEST_CASE("parallel_scan testing with template functor") {
    for (auto concurrency_level : utils::concurrency_range()) {
        tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_level);
        for (int mode = 0; mode < 3; mode++) {
            NumberOfLiveStorage = 0;
            TestInterface(mode,  ParallelScanTemplateFunctor());
            // Test that all workers sleep when no work
            TestCPUUserTime(concurrency_level);

            // Checking has to be done late, because when parallel_scan makes copies of
            // the user's "Body", the copies might be destroyed slightly after parallel_scan
            // returns.
            CHECK(NumberOfLiveStorage == 0);
        }
    }
}

// Test for parallel_scan with lambdas
//! \brief \ref error_guessing \ref interface \ref resource_usage
TEST_CASE("parallel_scan testing with lambdas") {
    for (auto concurrency_level : utils::concurrency_range()) {
        tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_level);
        for (int mode = 0; mode < 3; mode++) {
            NumberOfLiveStorage = 0;
            TestInterface(mode,  ParallelScanLambda());

            // Test that all workers sleep when no work
            TestCPUUserTime(concurrency_level);

            // Checking has to be done late, because when parallel_scan makes copies of
            // the user's "Body", the copies might be destroyed slightly after parallel_scan
            // returns.
            CHECK(NumberOfLiveStorage == 0);
        }
    }
}

#if __TBB_CPP14_GENERIC_LAMBDAS_PRESENT
// Test for parallel_scan with genetic lambdas
//! \brief \ref error_guessing \ref interface \ref resource_usage
TEST_CASE("parallel_scan testing with generic lambdas") {
    for (auto concurrency_level : utils::concurrency_range()) {
        tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_level);
        for (int mode = 0; mode < 3; mode++) {
            NumberOfLiveStorage = 0;
            TestInterface(mode,  ParallelScanGenericLambda());
            // Test that all workers sleep when no work
            TestCPUUserTime(concurrency_level);

            // Checking has to be done late, because when parallel_scan makes copies of
            // the user's "Body", the copies might be destroyed slightly after parallel_scan
            // returns.
            CHECK(NumberOfLiveStorage == 0);
        }
    }
}
#endif /* __TBB_CPP14_GENERIC_LAMBDAS_PRESENT */

#if __TBB_CPP20_CONCEPTS_PRESENT
//! \brief \ref error_guessing
TEST_CASE("parallel_scan constraints") {
    test_pscan_range_constraints();
    test_pscan_body_constraints();
    test_pscan_func_constraints();
    test_pscan_combine_constraints();
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT
