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

#include "test_mutex.h"

#include <tbb/spin_mutex.h>
#include "oneapi/tbb/mutex.h"
#include <tbb/spin_rw_mutex.h>
#include "oneapi/tbb/rw_mutex.h"
#include <tbb/queuing_mutex.h>
#include <tbb/queuing_rw_mutex.h>
#include <tbb/null_mutex.h>
#include <tbb/null_rw_mutex.h>
#include <tbb/parallel_for.h>
#include <oneapi/tbb/detail/_utils.h>
#include <oneapi/tbb/detail/_machine.h>

//! \file test_mutex.cpp
//! \brief Test for [mutex.spin_mutex mutex.spin_rw_mutex mutex.queuing_mutex mutex.queuing_rw_mutex mutex.mutex mutex.rw_mutex mutex.speculative_spin_mutex mutex.speculative_spin_rw_mutex] specifications

// TODO: Investigate why RTM doesn't work on some macOS.
// TODO: Consider adding Thread Sanitizer (note that accesses inside the transaction
// considered as races by Thread Sanitizer)
#if __TBB_TSX_INTRINSICS_PRESENT && !__APPLE__ && !__TBB_USE_THREAD_SANITIZER

inline static bool IsInsideTx() {
    return _xtest() != 0;
}

bool have_TSX() {
    bool result = false;
    const int rtm_ebx_mask = 1 << 11;
#if _MSC_VER
    int info[4] = { 0,0,0,0 };
    const int reg_ebx = 1;
    __cpuidex(info, 7, 0);
    result = (info[reg_ebx] & rtm_ebx_mask) != 0;
#elif __GNUC__ || __SUNPRO_CC
    int32_t reg_ebx = 0;
    int32_t reg_eax = 7;
    int32_t reg_ecx = 0;
    __asm__ __volatile__("movl %%ebx, %%esi\n"
        "cpuid\n"
        "movl %%ebx, %0\n"
        "movl %%esi, %%ebx\n"
        : "=a"(reg_ebx) : "0" (reg_eax), "c" (reg_ecx) : "esi",
#if __TBB_x86_64
        "ebx",
#endif
        "edx"
    );
    result = (reg_ebx & rtm_ebx_mask) != 0;
#endif
    return result;
}

//! Function object for use with parallel_for.h to see if a transaction is actually attempted.
std::atomic<std::size_t> n_transactions_attempted;
template<typename C>
struct AddOne_CheckTransaction {

    AddOne_CheckTransaction& operator=(const AddOne_CheckTransaction&) = delete;
    AddOne_CheckTransaction(const AddOne_CheckTransaction&) = default;
    AddOne_CheckTransaction() = default;

    C& counter;
    /** Increments counter once for each iteration in the iteration space. */
    void operator()(tbb::blocked_range<size_t>& range) const {
        for (std::size_t i = range.begin(); i != range.end(); ++i) {
            bool transaction_attempted = false;
            {
                typename C::mutex_type::scoped_lock lock(counter.mutex);
                if (IsInsideTx()) transaction_attempted = true;
                counter.value = counter.value + 1;
            }
            if (transaction_attempted) ++n_transactions_attempted;
            tbb::detail::machine_pause(static_cast<int32_t>(i));
        }
    }
    AddOne_CheckTransaction(C& counter_) : counter(counter_) {}
};

/* TestTransaction() checks if a speculative mutex actually uses transactions. */
template<typename M>
void TestTransaction(const char* name)
{
    utils::Counter<M> counter;
    constexpr int n = 550;

    n_transactions_attempted = 0;
    for (int i = 0; i < 5 && n_transactions_attempted.load(std::memory_order_relaxed) == 0; ++i) {
        counter.value = 0;
        tbb::parallel_for(tbb::blocked_range<std::size_t>(0, n, 2), AddOne_CheckTransaction<utils::Counter<M>>(counter));
        REQUIRE(counter.value == n);
    }
    REQUIRE_MESSAGE(n_transactions_attempted.load(std::memory_order_relaxed), "ERROR for " << name << ": transactions were never attempted");
}


//! \brief \ref error_guessing
TEST_CASE("Transaction test") {
    if (have_TSX()) {
        TestTransaction<tbb::speculative_spin_mutex>("Speculative Spin Mutex");
        TestTransaction<tbb::speculative_spin_rw_mutex>("Speculative Spin RW Mutex");
    }
}
#endif /* __TBB_TSX_TESTING_ENABLED_FOR_THIS_COMPILER */

//! \brief \ref error_guessing
TEST_CASE("test upgrade/downgrade with spin_rw_mutex") {
    test_rwm_upgrade_downgrade<tbb::spin_rw_mutex>();
}

//! \brief \ref error_guessing
TEST_CASE("test upgrade/downgrade with queueing_rw_mutex") {
    test_rwm_upgrade_downgrade<tbb::queuing_rw_mutex>();
}

//! \brief \ref error_guessing
TEST_CASE("test upgrade/downgrade with rw_mutex") {
    test_rwm_upgrade_downgrade<tbb::rw_mutex>();
}

//! \brief \ref error_guessing
TEST_CASE("test upgrade/downgrade with speculative_spin_rw_mutex") {
    test_rwm_upgrade_downgrade<tbb::speculative_spin_rw_mutex>();
}

//! \brief \ref error_guessing
TEST_CASE("test spin_mutex with native threads") {
    test_with_native_threads::test<tbb::spin_mutex>();
}

//! \brief \ref error_guessing
TEST_CASE("test queuing_mutex with native threads") {
    test_with_native_threads::test<tbb::queuing_mutex>();
}

//! \brief \ref error_guessing
TEST_CASE("test mutex with native threads") {
    test_with_native_threads::test<tbb::mutex>();
}

//! \brief \ref error_guessing
TEST_CASE("test spin_rw_mutex with native threads") {
    test_with_native_threads::test<tbb::spin_rw_mutex>();
    test_with_native_threads::test_rw<tbb::spin_rw_mutex>();
}

//! \brief \ref error_guessing
TEST_CASE("test queuing_rw_mutex with native threads") {
    test_with_native_threads::test<tbb::queuing_rw_mutex>();
    test_with_native_threads::test_rw<tbb::queuing_rw_mutex>();
}

//! \brief \ref error_guessing
TEST_CASE("test rw_mutex with native threads") {
    test_with_native_threads::test<tbb::rw_mutex>();
    test_with_native_threads::test_rw<tbb::rw_mutex>();
}

//! Test scoped_lock::is_writer getter
//! \brief \ref error_guessing
TEST_CASE("scoped_lock::is_writer") {
    TestIsWriter<oneapi::tbb::spin_rw_mutex>("spin_rw_mutex");
    TestIsWriter<oneapi::tbb::queuing_rw_mutex>("queuing_rw_mutex");
    TestIsWriter<oneapi::tbb::speculative_spin_rw_mutex>("speculative_spin_rw_mutex");
    TestIsWriter<oneapi::tbb::null_rw_mutex>("null_rw_mutex");
    TestIsWriter<oneapi::tbb::rw_mutex>("rw_mutex");
}

#if __TBB_CPP20_CONCEPTS_PRESENT
template <typename... Args>
concept mutexes = (... && tbb::detail::scoped_lockable<Args>);

template <typename... Args>
concept rw_mutexes = (... && tbb::detail::rw_scoped_lockable<Args>);

//! \brief \ref error_guessing
TEST_CASE("internal mutex concepts") {
    static_assert(mutexes<tbb::spin_mutex, oneapi::tbb::mutex, tbb::speculative_spin_mutex, tbb::null_mutex, tbb::queuing_mutex,
                          tbb::spin_rw_mutex, oneapi::tbb::rw_mutex, tbb::speculative_spin_rw_mutex, tbb::null_rw_mutex, tbb::queuing_rw_mutex>);
    static_assert(rw_mutexes<tbb::spin_rw_mutex, oneapi::tbb::rw_mutex, tbb::speculative_spin_rw_mutex,
                             tbb::null_rw_mutex, tbb::queuing_rw_mutex>);
}
#endif // __TBB_CPP20_CONCEPTS_PRESENT
