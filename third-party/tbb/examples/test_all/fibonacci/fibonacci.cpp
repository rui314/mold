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

/* Example program that computes Fibonacci numbers in different ways.
   Arguments are: [ Number [Threads [Repeats]]]
   The defaults are Number=500 Threads=1:4 Repeats=1.

   The point of this program is to check that the library is working properly.
   Most of the computations are deliberately silly and not expected to
   show any speedup on multiprocessors.
*/

// enable assertions
#ifdef NDEBUG
#undef NDEBUG
#endif

#include <cstdio>
#include <cstdlib>
#include <cassert>

#include <utility>
#include <thread>
#include <atomic>
#include <mutex>

#include "oneapi/tbb/tick_count.h"
#include "oneapi/tbb/blocked_range.h"
#include "oneapi/tbb/concurrent_vector.h"
#include "oneapi/tbb/concurrent_queue.h"
#include "oneapi/tbb/concurrent_hash_map.h"
#include "oneapi/tbb/parallel_for.h"
#include "oneapi/tbb/parallel_reduce.h"
#include "oneapi/tbb/parallel_scan.h"
#include "oneapi/tbb/parallel_pipeline.h"
#include "oneapi/tbb/spin_mutex.h"
#include "oneapi/tbb/queuing_mutex.h"
#include "oneapi/tbb/global_control.h"

//! type used for Fibonacci number computations
typedef long long value;

//! Matrix 2x2 class
struct Matrix2x2 {
    //! Array of values
    value v[2][2];
    Matrix2x2() {}
    Matrix2x2(value v00, value v01, value v10, value v11) {
        v[0][0] = v00;
        v[0][1] = v01;
        v[1][0] = v10;
        v[1][1] = v11;
    }
    Matrix2x2 operator*(const Matrix2x2 &to) const; //< Multiply two Matrices
};
//! Identity matrix
static const Matrix2x2 MatrixIdentity(1, 0, 0, 1);
//! Default matrix to multiply
static const Matrix2x2 Matrix1110(1, 1, 1, 0);
//! Raw arrays matrices multiply
void Matrix2x2Multiply(const value a[2][2], const value b[2][2], value c[2][2]);

/////////////////////// Serial methods ////////////////////////

//! Plain serial sum
value SerialFib(int n) {
    if (n < 2)
        return n;
    value a = 0, b = 1, sum;
    int i;
    for (i = 2; i <= n; i++) { // n is really index of Fibonacci number
        sum = a + b;
        a = b;
        b = sum;
    }
    return sum;
}
//! Serial n-1 matrices multiplication
value SerialMatrixFib(int n) {
    value c[2][2], a[2][2] = { { 1, 1 }, { 1, 0 } }, b[2][2] = { { 1, 1 }, { 1, 0 } };
    int i;
    for (i = 2; i < n; i++) { // Using condition to prevent copying of values
        if (i & 1)
            Matrix2x2Multiply(a, c, b);
        else
            Matrix2x2Multiply(a, b, c);
    }
    return (i & 1) ? c[0][0] : b[0][0]; // get result from upper left cell
}
//! Recursive summing. Just for complete list of serial algorithms, not used
value SerialRecursiveFib(int n) {
    value result;
    if (n < 2)
        result = n;
    else
        result = SerialRecursiveFib(n - 1) + SerialRecursiveFib(n - 2);
    return result;
}

// GCC 4.8 C++ standard library implements std::this_thread::yield as no-op.
#if __TBB_GLIBCXX_THIS_THREAD_YIELD_BROKEN
static inline void yield() {
    sched_yield();
}
#else
using std::this_thread::yield;
#endif

//! Introducing of queue method in serial
value SerialQueueFib(int n) {
    oneapi::tbb::concurrent_queue<Matrix2x2> Q;
    for (int i = 1; i < n; i++)
        Q.push(Matrix1110);
    Matrix2x2 A, B;
    while (true) {
        while (!Q.try_pop(A))
            yield();
        if (Q.empty())
            break;
        while (!Q.try_pop(B))
            yield();
        Q.push(A * B);
    }
    return A.v[0][0];
}
//! Trying to use concurrent_vector
value SerialVectorFib(int n) {
    oneapi::tbb::concurrent_vector<value> A;
    A.grow_by(2);
    A[0] = 0;
    A[1] = 1;
    for (int i = 2; i <= n; i++) {
        A.grow_to_at_least(i + 1);
        A[i] = A[i - 1] + A[i - 2];
    }
    return A[n];
}

///////////////////// Parallel methods ////////////////////////

// *** Serial shared by mutexes *** //

//! Shared glabals
value SharedA = 0, SharedB = 1;
int SharedI = 1, SharedN;

//! Template task class which computes Fibonacci numbers with shared globals
template <typename M>
class SharedSerialFibBody {
    M &mutex;

public:
    SharedSerialFibBody(M &m) : mutex(m) {}
    //! main loop
    void operator()(const oneapi::tbb::blocked_range<int> &range) const {
        for (;;) {
            typename M::scoped_lock lock(mutex);
            if (SharedI >= SharedN)
                break;
            value sum = SharedA + SharedB;
            SharedA = SharedB;
            SharedB = sum;
            ++SharedI;
        }
    }
};

template <>
void SharedSerialFibBody<std::mutex>::operator()(
    const oneapi::tbb::blocked_range<int> &range) const {
    for (;;) {
        std::lock_guard<std::mutex> lock(mutex);
        if (SharedI >= SharedN)
            break;
        value sum = SharedA + SharedB;
        SharedA = SharedB;
        SharedB = sum;
        ++SharedI;
    }
}

//! Root function
template <class M>
value SharedSerialFib(int n) {
    SharedA = 0;
    SharedB = 1;
    SharedI = 1;
    SharedN = n;
    M mutex;
    parallel_for(oneapi::tbb::blocked_range<int>(0, 4, 1), SharedSerialFibBody<M>(mutex));
    return SharedB;
}

// *** Serial shared by concurrent hash map *** //

//! Hash comparer
struct IntHashCompare {
    bool equal(const int j, const int k) const {
        return j == k;
    }
    std::size_t hash(const int k) const {
        return (std::size_t)k;
    }
};
//! NumbersTable type based on concurrent_hash_map
typedef oneapi::tbb::concurrent_hash_map<int, value, IntHashCompare> NumbersTable;
//! task for serial method using shared concurrent_hash_map
class ConcurrentHashSerialFibTask {
    NumbersTable &Fib;
    int my_n;

public:
    //! constructor
    ConcurrentHashSerialFibTask(NumbersTable &cht, int n) : Fib(cht), my_n(n) {}
    //! executing task
    void operator()() const {
        for (int i = 2; i <= my_n; ++i) { // there is no difference in to recycle or to make loop
            NumbersTable::const_accessor f1, f2; // same as iterators
            if (!Fib.find(f1, i - 1) || !Fib.find(f2, i - 2)) {
                // Something is seriously wrong, because i-1 and i-2 must have been inserted
                // earlier by this thread or another thread.
                assert(0);
            }
            value sum = f1->second + f2->second;
            NumbersTable::const_accessor fsum;
            Fib.insert(fsum, std::make_pair(i, sum)); // inserting
            assert(fsum->second == sum); // check value
        }
    }
};

//! Root function
value ConcurrentHashSerialFib(int n) {
    NumbersTable Fib;
    bool okay;
    okay = Fib.insert(std::make_pair(0, 0));
    assert(okay); // assign initial values
    okay = Fib.insert(std::make_pair(1, 1));
    assert(okay);

    // task_list list;
    oneapi::tbb::task_group tg;
    // allocate tasks
    tg.run(ConcurrentHashSerialFibTask(Fib, n));
    tg.run(ConcurrentHashSerialFibTask(Fib, n));
    tg.wait();
    NumbersTable::const_accessor fresult;
    okay = Fib.find(fresult, n);
    assert(okay);
    return fresult->second;
}

// *** Queue with parallel_pipeline *** //

typedef oneapi::tbb::concurrent_queue<Matrix2x2> queue_t;
namespace parallel_pipeline_ns {
std::atomic<int> N; //< index of Fibonacci number minus 1
queue_t Queue;
} // namespace parallel_pipeline_ns

//! functor to fills queue
struct InputFunc {
    InputFunc() {}
    queue_t *operator()(oneapi::tbb::flow_control &fc) const {
        using namespace parallel_pipeline_ns;

        int n = --N;
        if (n <= 0) {
            fc.stop();
            return nullptr;
        }
        Queue.push(Matrix1110);
        return &Queue;
    }
};
//! functor to process queue
struct MultiplyFunc {
    MultiplyFunc() {}
    void operator()(queue_t *queue) const {
        //concurrent_queue<Matrix2x2> &Queue = *static_cast<concurrent_queue<Matrix2x2> *>(p);
        Matrix2x2 m1, m2;
        // get two elements
        while (!queue->try_pop(m1))
            yield();
        while (!queue->try_pop(m2))
            yield();
        m1 = m1 * m2; // process them
        queue->push(m1); // and push back
    }
};
//! Root function
value ParallelPipeFib(int n) {
    using namespace parallel_pipeline_ns;

    N = n - 1;
    Queue.push(Matrix1110);

    oneapi::tbb::parallel_pipeline(
        n,
        oneapi::tbb::make_filter<void, queue_t *>(oneapi::tbb::filter_mode::parallel, InputFunc()) &
            oneapi::tbb::make_filter<queue_t *, void>(oneapi::tbb::filter_mode::parallel,
                                                      MultiplyFunc()));

    assert(Queue.unsafe_size() == 1);
    Matrix2x2 M;
    bool result = Queue.try_pop(M); // get last element
    assert(result);
    value res = M.v[0][0]; // get value
    Queue.clear();
    return res;
}

// *** parallel_reduce *** //

//! Functor for parallel_reduce
struct parallel_reduceFibBody {
    Matrix2x2 sum;
    int split_flag; //< flag to make one less operation for split bodies
    //! Constructor fills sum with initial matrix
    parallel_reduceFibBody() : sum(Matrix1110), split_flag(0) {}
    //! Splitting constructor
    parallel_reduceFibBody(parallel_reduceFibBody &other, oneapi::tbb::split)
            : sum(Matrix1110),
              split_flag(1 /*note that it is split*/) {}
    //! Join point
    void join(parallel_reduceFibBody &s) {
        sum = sum * s.sum;
    }
    //! Process multiplications
    void operator()(const oneapi::tbb::blocked_range<int> &r) {
        for (int k = r.begin() + split_flag; k < r.end(); ++k)
            sum = sum * Matrix1110;
        split_flag = 0; // reset flag, because this method can be reused for next range
    }
};
//! Root function
value parallel_reduceFib(int n) {
    parallel_reduceFibBody b;
    oneapi::tbb::parallel_reduce(oneapi::tbb::blocked_range<int>(2, n, 3),
                                 b); // do parallel reduce on range [2, n) for b
    return b.sum.v[0][0];
}

// *** parallel_scan *** //

//! Functor for parallel_scan
struct parallel_scanFibBody {
    /** Though parallel_scan is usually used to accumulate running sums,
        it can be used to accumulate running products too. */
    Matrix2x2 product;
    /** Pointer to output sequence */
    value *const output;
    //! Constructor sets product to identity matrix
    parallel_scanFibBody(value *output_) : product(MatrixIdentity), output(output_) {}
    //! Splitting constructor
    parallel_scanFibBody(parallel_scanFibBody &b, oneapi::tbb::split)
            : product(MatrixIdentity),
              output(b.output) {}
    //! Method for merging summary information from a, which was split off from *this, into *this.
    void reverse_join(parallel_scanFibBody &a) {
        // When using non-commutative reduction operation, reverse_join
        // should put argument "a" on the left side of the operation.
        // The reversal from the argument order is why the method is
        // called "reverse_join" instead of "join".
        product = a.product * product;
    }
    //! Method for assigning final result back to original body.
    void assign(parallel_scanFibBody &b) {
        product = b.product;
    }
    //! Compute matrix running product.
    /** Tag indicates whether is is the final scan over the range, or
        just a helper "prescan" that is computing a partial reduction. */
    template <typename Tag>
    void operator()(const oneapi::tbb::blocked_range<int> &r, Tag tag) {
        for (int k = r.begin(); k < r.end(); ++k) {
            // Code performs an "exclusive" scan, which outputs a value *before* updating the product.
            // For an "inclusive" scan, output the value after the update.
            if (tag.is_final_scan())
                output[k] = product.v[0][1];
            product = product * Matrix1110;
        }
    }
};
//! Root function
value parallel_scanFib(int n) {
    value *output = new value[n];
    parallel_scanFibBody b(output);
    oneapi::tbb::parallel_scan(oneapi::tbb::blocked_range<int>(0, n, 3), b);
    // output[0..n-1] now contains the Fibonacci sequence (modulo integer wrap-around).
    // Check the last two values for correctness.
    assert(n < 2 || output[n - 2] + output[n - 1] == b.product.v[0][1]);
    delete[] output;
    return b.product.v[0][1];
}

/////////////////////////// Main ////////////////////////////////////////////////////

//! A closed range of int.
struct IntRange {
    int low;
    int high;
    void set_from_string(const char *s);
    IntRange(int low_, int high_) : low(low_), high(high_) {}
};

void IntRange::set_from_string(const char *s) {
    char *end;
    high = low = strtol(s, &end, 0);
    switch (*end) {
        case ':': high = strtol(end + 1, nullptr, 0); break;
        case '\0': break;
        default: printf("unexpected character = %c\n", *end);
    }
}

//! Tick count for start
static oneapi::tbb::tick_count t0;

//! Verbose output flag
static bool Verbose = false;

typedef value (*MeasureFunc)(int);
//! Measure ticks count in loop [2..n]
value Measure(const char *name, MeasureFunc func, int n) {
    value result;
    if (Verbose)
        printf("%s", name);
    t0 = oneapi::tbb::tick_count::now();
    for (int number = 2; number <= n; number++)
        result = func(number);
    if (Verbose)
        printf("\t- in %f msec\n", (oneapi::tbb::tick_count::now() - t0).seconds() * 1000);
    return result;
}

//! program entry
int main(int argc, char *argv[]) {
    if (argc > 1)
        Verbose = true;
    int NumbersCount = argc > 1 ? strtol(argv[1], nullptr, 0) : 500;
    IntRange NThread(1, 4); // Number of threads to use.
    if (argc > 2)
        NThread.set_from_string(argv[2]);
    unsigned long ntrial = argc > 3 ? (unsigned long)strtoul(argv[3], nullptr, 0) : 1;
    value result, sum;

    if (Verbose)
        printf("Fibonacci numbers example. Generating %d numbers..\n", NumbersCount);

    result = Measure("Serial loop", SerialFib, NumbersCount);
    sum = Measure("Serial matrix", SerialMatrixFib, NumbersCount);
    assert(result == sum);
    sum = Measure("Serial vector", SerialVectorFib, NumbersCount);
    assert(result == sum);
    sum = Measure("Serial queue", SerialQueueFib, NumbersCount);
    assert(result == sum);
    // now in parallel
    for (unsigned long i = 0; i < ntrial; ++i) {
        for (int threads = NThread.low; threads <= NThread.high; threads *= 2) {
            oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism,
                                          threads);
            if (Verbose)
                printf("\nThreads number is %d\n", threads);

            sum = Measure("Shared serial (mutex)\t", SharedSerialFib<std::mutex>, NumbersCount);
            assert(result == sum);
            sum = Measure("Shared serial (spin_mutex)",
                          SharedSerialFib<oneapi::tbb::spin_mutex>,
                          NumbersCount);
            assert(result == sum);
            sum = Measure("Shared serial (queuing_mutex)",
                          SharedSerialFib<oneapi::tbb::queuing_mutex>,
                          NumbersCount);
            assert(result == sum);
            sum = Measure("Shared serial (Conc.HashTable)", ConcurrentHashSerialFib, NumbersCount);
            assert(result == sum);
            sum = Measure("Parallel pipe/queue\t", ParallelPipeFib, NumbersCount);
            assert(result == sum);
            sum = Measure("Parallel reduce\t\t", parallel_reduceFib, NumbersCount);
            assert(result == sum);
            sum = Measure("Parallel scan\t\t", parallel_scanFib, NumbersCount);
            assert(result == sum);
        }

#ifdef __GNUC__
        if (Verbose)
            printf("Fibonacci number #%d modulo 2^64 is %lld\n\n", NumbersCount, result);
#else
        if (Verbose)
            printf("Fibonacci number #%d modulo 2^64 is %I64d\n\n", NumbersCount, result);
#endif
    }
    if (!Verbose)
        printf("TEST PASSED\n");
    // flush to prevent bufferization on exit
    fflush(stdout);
    return 0;
}

// Utils

void Matrix2x2Multiply(const value a[2][2], const value b[2][2], value c[2][2]) {
    for (int i = 0; i <= 1; i++)
        for (int j = 0; j <= 1; j++)
            c[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j];
}

Matrix2x2 Matrix2x2::operator*(const Matrix2x2 &to) const {
    Matrix2x2 result;
    Matrix2x2Multiply(v, to.v, result.v);
    return result;
}
