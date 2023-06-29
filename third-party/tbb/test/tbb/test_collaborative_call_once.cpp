/*
    Copyright (c) 2022 Intel Corporation

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

#if _MSC_VER && !defined(__INTEL_COMPILER)
// unreachable code
#pragma warning( push )
#pragma warning( disable: 4702 )
#endif

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#include "common/config.h"

// Include first to check missed header dependencies
#include "tbb/collaborative_call_once.h"

#include "common/test.h"
#include "common/exception_handling.h"
#include "common/utils_concurrency_limit.h"

#include "tbb/parallel_invoke.h"
#include "tbb/parallel_reduce.h"
#include "tbb/task_arena.h"

//! \file test_collaborative_call_once.cpp
//! \brief Tests for [algorithms.collaborative_call_once] functionality

struct increment_functor {
    int ct{0};

    void operator()() {
        ct++;
    }
};

struct sum_functor {
    int sum{0};

    template<typename T>
    void operator()(T first_op) {
        sum += first_op;
    }

    template<typename T, typename... Args>
    void operator()(T first_op, Args&&... args) {
        (*this)(first_op);
        (*this)(std::forward<Args>(args)...);
    }
};

struct move_only_type {
    const int* my_pointer;
    move_only_type(move_only_type && other): my_pointer(other.my_pointer){ other.my_pointer=nullptr; }
    explicit move_only_type(const int* value): my_pointer(value) {}
};


class call_once_exception : public std::exception {};

template<typename Fn, typename... Args>
void call_once_in_for_loop(std::size_t N, Fn&& body, Args&&... args) {
    tbb::collaborative_once_flag flag;
    for (std::size_t i = 0; i < N; ++i) {
        tbb::collaborative_call_once(flag, std::forward<Fn>(body), std::forward<Args>(args)...);
    }
}

template<typename Fn, typename... Args>
void call_once_in_parallel_for(std::size_t N, Fn&& body, Args&&... args) {
    tbb::collaborative_once_flag flag;
#if __TBB_GCC_PARAMETER_PACK_IN_LAMBDAS_BROKEN
    auto stored_pack = tbb::detail::d0::save_pack(std::forward<Args>(args)...);
    auto func = [&] { tbb::detail::d0::call(std::forward<Fn>(body), std::move(stored_pack)); };
#endif // __TBB_GCC_PARAMETER_PACK_IN_LAMBDAS_BROKEN

    tbb::parallel_for(tbb::blocked_range<size_t>(0, N), [&](const tbb::blocked_range<size_t>& range) {
        for (size_t i = range.begin(); i != range.end(); ++i) {
#if __TBB_GCC_PARAMETER_PACK_IN_LAMBDAS_BROKEN
            tbb::collaborative_call_once(flag, func);
#else
            tbb::collaborative_call_once(flag, std::forward<Fn>(body), std::forward<Args>(args)...);
#endif //__TBB_GCC_PARAMETER_PACK_IN_LAMBDAS_BROKEN
        }
    });
}

template<typename Fn, typename... Args>
void call_once_threads(std::size_t N, Fn&& body, Args&&... args) {
    tbb::collaborative_once_flag flag;
    std::vector<std::thread> threads;

#if __TBB_GCC_PARAMETER_PACK_IN_LAMBDAS_BROKEN
    auto stored_pack = tbb::detail::d0::save_pack(std::forward<Args>(args)...);
    auto func = [&] { tbb::detail::d0::call(std::forward<Fn>(body), std::move(stored_pack)); };
#endif // __TBB_GCC_PARAMETER_PACK_IN_LAMBDAS_BROKEN

    for (std::size_t i = 0; i < N; ++i)
    {
        threads.push_back(std::thread([&]() {
#if __TBB_GCC_PARAMETER_PACK_IN_LAMBDAS_BROKEN
            tbb::collaborative_call_once(flag, func);
#else
            tbb::collaborative_call_once(flag, std::forward<Fn>(body), std::forward<Args>(args)...);
#endif // __TBB_GCC_PARAMETER_PACK_IN_LAMBDAS_BROKEN
        }));
    }
    for (auto& thread : threads) {
        thread.join();
    }
}

//! Test for functor to be called only once
//! \brief \ref interface \ref requirement
TEST_CASE("only calls once 1") {
    {
        increment_functor f;

        call_once_in_for_loop(1024, f);

        REQUIRE( f.ct == 1);
    }
    {
        increment_functor f;

        call_once_in_parallel_for(100, f);

        REQUIRE(f.ct == 1);
    }
    {
        increment_functor f;

        call_once_threads(utils::get_platform_max_threads(), f);

        REQUIRE(f.ct == 1);
    }
}

//! Test for functor to be called only once
//! \brief \ref interface \ref requirement
TEST_CASE("only calls once 2") {
    {
        sum_functor f;

        call_once_in_for_loop(1024, f, 1, 2, 3 ,4);

        REQUIRE(f.sum == 10);
    }
    {
        sum_functor f;

        call_once_in_parallel_for(512, f, 1000, -1000);

        REQUIRE(f.sum == 0);
    }
    {
        sum_functor f;

        call_once_threads(utils::get_platform_max_threads(), f, 0, -1, -5);

        REQUIRE(f.sum == -6);
    }
}

//! Test for correct handling move-only arguments
//! \brief \ref interface \ref requirement
TEST_CASE("only calls once - move only argument") {
    const int value = 42;
    int ready{0};

    auto func = [&ready, &value] (move_only_type other) {
        REQUIRE(other.my_pointer == &value);
        ready++;
    };

    {
        move_only_type mv(&value);

        call_once_in_parallel_for(512, func, std::move(mv));

        REQUIRE(ready == 1);
        REQUIRE(mv.my_pointer == nullptr);
    }

    {
        move_only_type mv(&value);

        call_once_threads(utils::get_platform_max_threads(), func, std::move(mv));

        REQUIRE(ready == 2);
        REQUIRE(mv.my_pointer == nullptr);
    }
}

//! Stress test for functor to be called only once
//! \brief \ref interface \ref requirement \ref stress
TEST_CASE("only calls once - stress test") {
#if TBB_TEST_LOW_WORKLOAD
    constexpr std::size_t N = 32;
#elif __TBB_x86_32 || __arm__  || __ANDROID__
    // Some C++ implementations allocate 8MB stacks for std::thread on 32 bit platforms
    // that makes impossible to create more than ~500 threads.
    // Android has been added to decrease testing time.
    constexpr std::size_t N = tbb::detail::d0::max_nfs_size * 2;
#elif __TBB_USE_THREAD_SANITIZER
    // Reduce execution time under Thread Sanitizer
    constexpr std::size_t N = tbb::detail::d0::max_nfs_size + 64;
#else 
    constexpr std::size_t N = tbb::detail::d0::max_nfs_size * 4;
#endif
    {
        increment_functor f;

        call_once_threads(N, f);

        REQUIRE(f.ct == 1);
    }
    {
        increment_functor f;

        utils::SpinBarrier barrier{N};
        tbb::collaborative_once_flag flag;
        utils::NativeParallelFor(N, [&](std::size_t) {
            for (int i = 0; i < 100; ++i) {
                REQUIRE(f.ct == i);
                barrier.wait([&] {
                    flag.~collaborative_once_flag();
                    new (&flag) tbb::collaborative_once_flag{};
                });
                tbb::collaborative_call_once(flag, f);
            }
        });
    }
}

#if TBB_USE_EXCEPTIONS

//! Test for collaborative_call_once exception handling
//! \brief \ref error_guessing
TEST_CASE("handles exceptions - state reset") {
    bool b{ false };
    auto setB = [&b]() { b = true; };
    auto setBAndCancel = [&b]() {
        b = true;
        throw call_once_exception{};
    };

    tbb::collaborative_once_flag flag;
    REQUIRE_THROWS_AS(tbb::collaborative_call_once(flag, setBAndCancel), call_once_exception);
    REQUIRE(b);

    b = false;
    REQUIRE_THROWS_AS(tbb::collaborative_call_once(flag, setBAndCancel), call_once_exception);
    REQUIRE(b);

    b = false;
    tbb::collaborative_call_once(flag, setB);
    REQUIRE(b);

    b = false;
    tbb::collaborative_call_once(flag, setB); // Now the call_once flag should be set.
    REQUIRE(!b);

    b = false;
    REQUIRE_NOTHROW(tbb::collaborative_call_once(flag, setBAndCancel)); // Flag still set, so it shouldn't be called.
    REQUIRE(!b);
}

//! Stress test for collaborative_call_once exception handling
//! \brief \ref error_guessing \ref stress
TEST_CASE("handles exceptions - stress test") {
#if TBB_TEST_LOW_WORKLOAD
    constexpr std::size_t N = 32;
#elif __TBB_x86_32 || __arm__ || __ANDROID__
    // Some C++ implementations allocate 8MB stacks for std::thread on 32 bit platforms
    // that makes impossible to create more than ~500 threads.
    // Android has been added to decrease testing time.
    constexpr std::size_t N = tbb::detail::d0::max_nfs_size * 2;
#else 
    constexpr std::size_t N = tbb::detail::d0::max_nfs_size * 4;
#endif

    int data{0};
    std::atomic<bool> run_again{true};

    auto throwing_func = [&] {
        utils::doDummyWork(10000);
        if (data < 100) {
            data++;
            throw call_once_exception{};
        }
        run_again = false;
    };

    tbb::collaborative_once_flag flag;

    utils::NativeParallelFor(N, [&](std::size_t) {
        while(run_again) {
            try {
                tbb::collaborative_call_once(flag, throwing_func);
            } catch (const call_once_exception&) {
                // We expecting only const call_once_exception&
            } catch (...) {
                FAIL("Unexpected exception");
            }
        }
    });
    REQUIRE(data == 100);
}

#endif

//! Test for multiple help from moonlighting threads
//! \brief \ref interface \ref requirement
TEST_CASE("multiple help") {
    std::size_t num_threads = utils::get_platform_max_threads();
    utils::SpinBarrier barrier{num_threads};

    tbb::collaborative_once_flag flag;

    tbb::parallel_for<std::size_t>(0, num_threads, [&](std::size_t) {
        barrier.wait();
        tbb::collaborative_call_once(flag, [&] {
            tbb::parallel_for<std::size_t>(0, num_threads, [&](std::size_t) {
                barrier.wait();
            });
        });
    });
}

//! Test for collaborative work from different arenas
//! \brief \ref interface \ref requirement
TEST_CASE("multiple arenas") {
    int num_threads = static_cast<int>(utils::get_platform_max_threads());
    utils::SpinBarrier barrier(num_threads);
    tbb::task_arena a1(num_threads), a2(num_threads);

    tbb::collaborative_once_flag flag;
    for (auto i = 0; i < num_threads - 1; ++i) {
        a1.enqueue([&] {
            barrier.wait();
            barrier.wait();

            tbb::collaborative_call_once(flag, [] {
                FAIL("Unreachable code. collaborative_once_flag must be already initialized at this moment");
            });
            // To sync collaborative_once_flag lifetime
            barrier.wait();
        });
    }

    barrier.wait();

    a2.execute([&] {
        utils::ConcurrencyTracker ct;
        tbb::parallel_for(0, num_threads, [&](int) {
            CHECK(utils::ConcurrencyTracker::PeakParallelism() == 1);
        });
        tbb::collaborative_call_once(flag, [&] {
            barrier.wait();
            tbb::parallel_for(0, num_threads, [&](int) {
                barrier.wait();
            });
        });
        // To sync collaborative_once_flag lifetime
        barrier.wait();
    });
}

using FibBuffer = std::vector<std::pair<tbb::collaborative_once_flag, std::uint64_t>>;
std::uint64_t collaborative_recursive_fib(int n, FibBuffer& buffer) {
    if (n <= 1) {
        return 1;
    }
    tbb::collaborative_call_once(buffer[n].first, [&]() {
        std::uint64_t a, b;
        tbb::parallel_invoke([&] { a = collaborative_recursive_fib(n - 2, buffer); },
                             [&] { b = collaborative_recursive_fib(n - 1, buffer); });
        buffer[n].second = a + b;
    });
    return buffer[n].second;
}

std::uint64_t collaborative_recursive_fib(int n) {
    FibBuffer buffer(n);
    return collaborative_recursive_fib(n-1, buffer);
}

//! Correctness test for Fibonacci example
//! \brief \ref interface \ref requirement
TEST_CASE("fibonacci example") {
    constexpr int N = 93;
    constexpr std::uint64_t expected_result = 12200160415121876738ull;

    auto collaborative = collaborative_recursive_fib(N);

    REQUIRE(collaborative == expected_result);
}

#if _MSC_VER && !defined(__INTEL_COMPILER)
#pragma warning( pop )
#endif
