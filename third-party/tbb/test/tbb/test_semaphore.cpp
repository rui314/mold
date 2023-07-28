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

//! \file test_semaphore.cpp
//! \brief Test for [internal] functionality

#if _WIN32 || _WIN64
#define _CRT_SECURE_NO_WARNINGS
#endif

// Test for counting semaphore
#include "common/test.h"
#include "common/utils.h"
#include "common/spin_barrier.h"
#include "tbb/blocked_range.h"
#include "tbb/tick_count.h"
#include "../../src/tbb/semaphore.h"
#include <atomic>
#include <vector>

using tbb::detail::r1::semaphore;

std::atomic<int> pCount;
utils::SpinBarrier sBarrier;

// Semaphore basis function:
//  set semaphore to initial value
// see that semaphore only allows that number of threads to be active
class Body : utils::NoAssign {
    const int nIters;
    semaphore& mySem;
    std::vector<int>& ourCounts;
    std::vector<double>& tottime;

    static constexpr int tickCounts = 1; // millisecond
    static constexpr int innerWait = 5; // millisecond
public:
    Body( int nThread, int nIter, semaphore& sem,
          std::vector<int>& our_counts, std::vector<double>& tot_time )
        : nIters(nIter), mySem(sem), ourCounts(our_counts), tottime(tot_time)
    {
        sBarrier.initialize(nThread);
        pCount = 0;
    }

    void operator()( const int tid ) const {
        sBarrier.wait();

        for (int i = 0; i < nIters; ++i) {
            utils::Sleep(tid * tickCounts);
            tbb::tick_count t0 = tbb::tick_count::now();
            mySem.P();
            tbb::tick_count t1 = tbb::tick_count::now();
            tottime[tid] += (t1 - t0).seconds();

            int curval = ++pCount;
            if (curval > ourCounts[tid]) {
                ourCounts[tid] = curval;
            }
            utils::Sleep(innerWait);
            --pCount;
            REQUIRE(int(pCount) >= 0);
            mySem.V();
        }
    }
}; // class Body

void test_semaphore( int sem_init_cnt, int extra_threads ) {
    semaphore my_sem(sem_init_cnt);
    int n_threads = sem_init_cnt + extra_threads;

    std::vector<int> max_vals(n_threads);
    std::vector<double> tot_times(n_threads);

    int n_iters = 10;
    Body body(n_threads, n_iters, my_sem, max_vals, tot_times);

    pCount = 0;
    utils::NativeParallelFor(n_threads, body);
    REQUIRE_MESSAGE(!pCount, "not all threads decremented pCount");

    int max_count = -1;
    for (auto item : max_vals) {
        max_count = utils::max(max_count, item);
    }
    REQUIRE_MESSAGE(max_count <= sem_init_cnt, "Too many threads in semaphore-protected increment");
}

#include "../../src/tbb/semaphore.cpp"
#if _WIN32 || _WIN64
#include "../../src/tbb/dynamic_link.cpp"
#endif

constexpr std::size_t N_TIMES = 1000;

template <typename S>
struct Counter {
    std::atomic<long> value;
    S my_sem;
    Counter() : value(0) {}
}; // struct Counter

// Function object for use with parallel_for.h
template <typename C>
struct AddOne : utils::NoAssign {
    C& my_counter;

    // Increments counter once for each iteration in the iteration space
    void operator()( int ) const {
        for (std::size_t i = 0; i < N_TIMES; ++i) {
            my_counter.my_sem.P();
            ++my_counter.value;
            my_counter.my_sem.V();
        }
    }

    AddOne( C& c ) : my_counter(c) {
        my_counter.my_sem.V();
    }
}; // struct AddOne

void test_binary_semaphore( int n_threads ) {
    Counter<tbb::detail::r1::binary_semaphore> counter;
    AddOne<decltype(counter)> AddOneBody(counter);
    utils::NativeParallelFor(n_threads, AddOneBody);
    REQUIRE_MESSAGE(n_threads * N_TIMES == counter.value, "Binary semaphore operations P()/V() have a race");
}

// Power of 2, the most tokens that can be in flight
constexpr std::size_t MAX_TOKENS = 32;
enum FilterType { imaProducer, imaConsumer };

class FilterBase : utils::NoAssign {
protected:
    FilterType ima;
    unsigned totTokens; // total number of tokens to be emitted, only used by producer
    std::atomic<unsigned>& myTokens;
    std::atomic<unsigned>& otherTokens;

    unsigned myWait;
    semaphore& my_sem;
    semaphore& next_sem;

    unsigned* myBuffer;
    unsigned* nextBuffer;
    unsigned curToken;
public:
    FilterBase( FilterType filter,
                unsigned tot_tokens,
                std::atomic<unsigned>& my_tokens,
                std::atomic<unsigned>& other_tokens,
                unsigned my_wait,
                semaphore& m_sem,
                semaphore& n_sem,
                unsigned* buf,
                unsigned* n_buf )
        : ima(filter), totTokens(tot_tokens), myTokens(my_tokens),
          otherTokens(other_tokens), myWait(my_wait), my_sem(m_sem),
          next_sem(n_sem), myBuffer(buf), nextBuffer(n_buf)
    {
        curToken = 0;
    }

    void Produce( const int );
    void Consume( const int );
    void operator()( const int tid ) {
        if (ima == imaConsumer) {
            Consume(tid);
        } else {
            Produce(tid);
        }
    }
}; // class FilterBase

class ProduceConsumeBody {
    FilterBase** my_filters;
public:
    ProduceConsumeBody( FilterBase** filters ) : my_filters(filters) {}

    void operator()( const int tid ) const {
        my_filters[tid]->operator()(tid);
    }
}; // class ProduceConsumeBody

// send a bunch of non-null "tokens" to consumer, then a nullptr
void FilterBase::Produce( const int ) {
    nextBuffer[0] = 0; // just in case we provide no tokens
    sBarrier.wait();
    while(totTokens) {
        while(!myTokens) {
            my_sem.P();
        }
        // we have a slot available
        --myTokens; // moving this down reduces spurious wakeups
        --totTokens;
        if (totTokens) {
            nextBuffer[curToken & (MAX_TOKENS - 1)] = curToken * 3 + 1;
        } else {
            nextBuffer[curToken & (MAX_TOKENS - 1)] = 0;
        }
        ++curToken;

        utils::Sleep(myWait);
        unsigned temp = ++otherTokens;
        if (temp == 1) {
            next_sem.V();
        }
    }
    next_sem.V(); // final wakeup
}

void FilterBase::Consume( const int ) {
    unsigned myToken;
    sBarrier.wait();
    do {
        while( !myTokens ) {
            my_sem.P();
        }
        // we have a slot available
        --myTokens;
        myToken = myBuffer[curToken & (MAX_TOKENS - 1)];
        if (myToken) {
            REQUIRE_MESSAGE(myToken == curToken * 3 + 1, "Error in received token");
            ++curToken;
            utils::Sleep(myWait);
            unsigned temp = ++otherTokens;
            if (temp == 1) {
                next_sem.V();
            }
        }
    } while(myToken);
    // end of processing
    REQUIRE_MESSAGE(curToken + 1 == totTokens, "Didn't receive enough tokens");
}

// test of producer/consumer with atomic buffer cnt and semaphore
// nTokens are total number of tokens through the pipe
// pWait is the wait time for the producer
// cWait is the wait time for the consumer
void test_producer_consumer( unsigned totTokens, unsigned nTokens, unsigned pWait, unsigned cWait ) {
    semaphore p_sem;
    semaphore c_sem;
    std::atomic<unsigned> p_tokens;
    std::atomic<unsigned> c_tokens(0);

    unsigned c_buffer[MAX_TOKENS];
    FilterBase* my_filters[2]; // one producer, one concumer

    REQUIRE_MESSAGE(nTokens <= MAX_TOKENS, "Not enough slots for tokens");

    my_filters[0] = new FilterBase(imaProducer, totTokens, p_tokens, c_tokens, pWait, c_sem, p_sem, nullptr, &(c_buffer[0]));
    my_filters[1] = new FilterBase(imaConsumer, totTokens, c_tokens, p_tokens, cWait, p_sem, c_sem, c_buffer, nullptr);

    p_tokens = nTokens;
    ProduceConsumeBody body(my_filters);
    sBarrier.initialize(2);
    utils::NativeParallelFor(2, body);
    delete my_filters[0];
    delete my_filters[1];
}

//! \brief \ref error_guessing
TEST_CASE("test binary semaphore") {
    test_binary_semaphore(utils::MaxThread);
}

//! \brief \ref error_guessing
TEST_CASE("test semaphore") {
    for (int sem_size = 1; sem_size <= int(utils::MaxThread); ++sem_size) {
        for (int ex_threads = 0; ex_threads <= int(utils::MaxThread) - sem_size; ++ex_threads) {
            test_semaphore(sem_size, ex_threads);
        }
    }
}

//! \brief \ref error_guessing
TEST_CASE("test producer-consumer") {
    test_producer_consumer(10, 2, 5, 5);
    test_producer_consumer(10, 2, 20, 5);
    test_producer_consumer(10, 2, 5, 20);

    test_producer_consumer(10, 1, 5, 5);
    test_producer_consumer(20, 10, 5, 20);
    test_producer_consumer(64, 32, 1, 20);
}
