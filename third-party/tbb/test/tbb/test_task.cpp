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
#include "common/dummy_body.h"
#include "common/spin_barrier.h"
#include "common/utils_concurrency_limit.h"
#include "common/cpu_usertime.h"

#include "tbb/task.h"
#include "tbb/task_group.h"
#include "tbb/parallel_for.h"
#include "tbb/cache_aligned_allocator.h"
#include "tbb/global_control.h"
#include "tbb/concurrent_vector.h"

#include <atomic>
#include <thread>
#include <thread>

//! \file test_task.cpp
//! \brief Test for [internal] functionality
struct EmptyBody {
    void operator()() const {}
};

#if _MSC_VER && !defined(__INTEL_COMPILER)
// unreachable code
#pragma warning( push )
#pragma warning( disable: 4702 )
#endif

template <typename Body = EmptyBody>
class CountingTask : public tbb::detail::d1::task {
public:
    CountingTask( Body body, tbb::detail::d1::wait_context& wait ) : my_body(body), my_wait(wait) {}

    CountingTask( tbb::detail::d1::wait_context& wait ) : my_wait(wait) {}

    task* execute( tbb::detail::d1::execution_data& ) override {
        ++my_execute_counter;
        my_body();
        my_wait.release();
        return nullptr;
    }

    task* cancel( tbb::detail::d1::execution_data& ) override {
        ++my_cancel_counter;
        my_wait.release();
        return nullptr;
    }

    static void reset() {
        my_execute_counter = 0;
        my_cancel_counter = 0;
    }

    static std::size_t execute_counter() { return my_execute_counter; }
    static std::size_t cancel_counter() { return my_cancel_counter; }

private:
    Body my_body;
    tbb::detail::d1::wait_context& my_wait;

    static std::atomic<std::size_t> my_execute_counter;
    static std::atomic<std::size_t> my_cancel_counter;
}; // struct CountingTask


#if _MSC_VER && !defined(__INTEL_COMPILER)
#pragma warning( pop )
#endif // warning 4702 is back

template <typename Body>
std::atomic<std::size_t> CountingTask<Body>::my_execute_counter(0);

template <typename Body>
std::atomic<std::size_t> CountingTask<Body>::my_cancel_counter(0);

#if TBB_USE_EXCEPTIONS
void test_cancellation_on_exception( bool reset_ctx ) {
    tbb::detail::d1::wait_context wait(1);
    tbb::task_group_context test_context;
    auto throw_body = [] {
        throw 1;
    };
    CountingTask<decltype(throw_body)> task(throw_body, wait);

    constexpr std::size_t iter_counter = 1000;
    for (std::size_t i = 0; i < iter_counter; ++i) {
        try {
            tbb::detail::d1::execute_and_wait(task, test_context, wait, test_context);
        } catch(int ex) {
            REQUIRE(ex == 1);
        }
        if (reset_ctx) {
            test_context.reset();
        }
        wait.reserve(1);
    }
    wait.release(1);

    REQUIRE_MESSAGE(task.execute_counter() == (reset_ctx ? iter_counter : 1), "Some task was not executed");
    REQUIRE_MESSAGE(task.cancel_counter() == iter_counter, "Some task was not canceled after the exception occurs");
    task.reset();
}
#endif // TBB_USE_EXCEPTIONS

//! \brief \ref error_guessing
TEST_CASE("External threads sleep") {
    if (utils::get_platform_max_threads() < 2) return;
    utils::SpinBarrier barrier(2);

    tbb::task_group test_gr;

    test_gr.run([&] {
        barrier.wait();
        TestCPUUserTime(2);
    });

    barrier.wait();

    test_gr.wait();
}

//! \brief \ref error_guessing
TEST_CASE("Test that task was executed p times") {
    tbb::detail::d1::wait_context wait(1);
    tbb::task_group_context test_context;
    CountingTask<> test_task(wait);

    constexpr std::size_t iter_counter = 10000;
    for (std::size_t i = 0; i < iter_counter; ++i) {
        tbb::detail::d1::execute_and_wait(test_task, test_context, wait, test_context);
        wait.reserve(1);
    }

    wait.release(1);

    REQUIRE_MESSAGE(CountingTask<>::execute_counter() == iter_counter, "The task was not executed necessary times");
    REQUIRE_MESSAGE(CountingTask<>::cancel_counter() == 0, "Some instance of the task was canceled");
    CountingTask<>::reset();
}

#if TBB_USE_EXCEPTIONS
//! \brief \ref error_guessing
TEST_CASE("Test cancellation on exception") {
    test_cancellation_on_exception(/*reset_ctx = */true);
    test_cancellation_on_exception(/*reset_ctx = */false);
}
#endif // TBB_USE_EXCEPTIONS

//! \brief \ref error_guessing
TEST_CASE("Simple test parallelism usage") {
    std::uint32_t threads_num = static_cast<std::uint32_t>(utils::get_platform_max_threads());
    utils::SpinBarrier barrier(threads_num);

    auto barrier_wait = [&barrier] {
        barrier.wait();
    };

    tbb::detail::d1::wait_context wait(threads_num);
    tbb::detail::d1::task_group_context test_context;
    using task_type = CountingTask<decltype(barrier_wait)>;

    std::vector<task_type, tbb::cache_aligned_allocator<task_type>> vector_test_task(threads_num, task_type(barrier_wait, wait));

    constexpr std::size_t iter_counter = 100;
    for (std::size_t i = 0; i < iter_counter; ++i) {
        for (std::size_t j = 0; j < threads_num; ++j) {
            tbb::detail::d1::spawn(vector_test_task[j], test_context);
        }
        tbb::detail::d1::wait(wait, test_context);
        wait.reserve(threads_num);
    }
    wait.release(threads_num);

    REQUIRE_MESSAGE(task_type::execute_counter() == iter_counter * threads_num, "Some task was not executed");
    REQUIRE_MESSAGE(task_type::cancel_counter() == 0, "Some task was canceled");
    task_type::reset();
}

//! \brief \ref error_guessing
TEST_CASE("Test parallelism usage with parallel_for") {
    std::uint32_t task_threads_num = static_cast<std::uint32_t>(utils::get_platform_max_threads());
    utils::SpinBarrier barrier(task_threads_num);

    auto barrier_wait = [&barrier] {
        barrier.wait();
    };

    std::size_t pfor_iter_count = 10000;
    std::atomic<std::size_t> pfor_counter(0);

    auto parallel_for_func = [&pfor_counter, pfor_iter_count] {
        tbb::parallel_for(tbb::blocked_range<std::size_t>(0, pfor_iter_count),
                          [&pfor_counter] (tbb::blocked_range<std::size_t>& range) {
                              for (auto it = range.begin(); it != range.end(); ++it) {
                                  ++pfor_counter;
                              }
                           }
        );
    };

    tbb::detail::d1::wait_context wait(task_threads_num);
    tbb::detail::d1::task_group_context test_context;
    using task_type = CountingTask<decltype(barrier_wait)>;
    std::vector<task_type, tbb::cache_aligned_allocator<task_type>> vector_test_task(task_threads_num, task_type(barrier_wait, wait));

    constexpr std::size_t iter_count = 10;
    constexpr std::size_t pfor_threads_num = 4;
    for (std::size_t i = 0; i < iter_count; ++i) {
        std::vector<std::thread> pfor_threads;

        for (std::size_t j = 0; j < task_threads_num; ++j) {
            tbb::detail::d1::spawn(vector_test_task[j], test_context);
        }

        for (std::size_t k = 0; k < pfor_threads_num; ++k) {
            pfor_threads.emplace_back(parallel_for_func);
        }

        tbb::detail::d1::wait(wait, test_context);

        for (auto& thread : pfor_threads) {
            if (thread.joinable()) {
                thread.join();
            }
        }

        wait.reserve(task_threads_num);
    }
    wait.release(task_threads_num);

    REQUIRE_MESSAGE(task_type::execute_counter() == task_threads_num * iter_count, "Some task was not executed");
    REQUIRE_MESSAGE(task_type::cancel_counter() == 0, "Some task was canceled");
    REQUIRE_MESSAGE(pfor_counter == iter_count * pfor_threads_num * pfor_iter_count, "Some parallel_for thread was not finished");
    task_type::reset();
}

//! \brief \ref error_guessing
TEST_CASE("Test parallelism usage with spawn tasks in different threads") {
    std::uint32_t threads_num = static_cast<std::uint32_t>(utils::get_platform_max_threads());
    utils::SpinBarrier barrier(threads_num);

    auto barrier_wait = [&barrier] {
        barrier.wait();
    };

    tbb::detail::d1::wait_context wait(threads_num);
    tbb::detail::d1::task_group_context test_context;
    using task_type = CountingTask<decltype(barrier_wait)>;
    std::vector<task_type, tbb::cache_aligned_allocator<task_type>> vector_test_task(threads_num, task_type(barrier_wait, wait));

    auto thread_func = [&vector_test_task, &test_context] ( std::size_t idx ) {
        tbb::detail::d1::spawn(vector_test_task[idx], test_context);
    };

    constexpr std::size_t iter_count = 10;
    for (std::size_t i = 0; i < iter_count; ++i) {
        std::vector<std::thread> threads;

        for (std::size_t k = 0; k < threads_num - 1; ++k) {
            threads.emplace_back(thread_func, k);
        }

        for (auto& thread : threads) {
            if (thread.joinable()) {
                thread.join();
            }
        }

        tbb::detail::d1::execute_and_wait(vector_test_task[threads_num - 1], test_context, wait, test_context);
        wait.reserve(threads_num);
    }
    wait.release(threads_num);

    REQUIRE_MESSAGE(task_type::execute_counter() == iter_count * threads_num, "Some task was not executed");
    REQUIRE_MESSAGE(task_type::cancel_counter() == 0, "Some task was canceled");
    task_type::reset();
}

class SpawningTaskBody;

using SpawningTask = CountingTask<SpawningTaskBody>;

class SpawningTaskBody {
public:
    using task_pool_type = std::vector<SpawningTask, tbb::cache_aligned_allocator<SpawningTask>>;

    SpawningTaskBody( task_pool_type& task_pool, tbb::task_group_context& test_ctx )
        : my_task_pool(task_pool), my_test_ctx(test_ctx) {}

    void operator()() const {
        std::size_t delta = 7;
        std::size_t start_idx = my_current_task.fetch_add(delta);

        if (start_idx < my_task_pool.size()) {
            for (std::size_t idx = start_idx; idx != std::min(my_task_pool.size(), start_idx + delta); ++idx) {
                tbb::detail::d1::spawn(my_task_pool[idx], my_test_ctx);
            }
        }
    }
private:
    task_pool_type& my_task_pool;
    tbb::task_group_context& my_test_ctx;
    static std::atomic<std::size_t> my_current_task;
}; // class SpawningTaskBody

std::atomic<std::size_t> SpawningTaskBody::my_current_task(0);

//! \brief \ref error_guessing
TEST_CASE("Actively adding tasks") {
    std::uint32_t task_number = 500 * static_cast<std::uint32_t>(utils::get_platform_max_threads());

    tbb::detail::d1::wait_context wait(task_number + 1);
    tbb::task_group_context test_context;

    SpawningTaskBody::task_pool_type task_pool;

    SpawningTaskBody task_body{task_pool, test_context};
    for (std::size_t i = 0; i < task_number; ++i) {
        task_pool.emplace_back(task_body, wait);
    }

    SpawningTask first_task(task_body, wait);
    tbb::detail::d1::execute_and_wait(first_task, test_context, wait, test_context);

    REQUIRE_MESSAGE(SpawningTask::execute_counter() == task_number + 1, "Some tasks were not executed"); // Is it right?
    REQUIRE_MESSAGE(SpawningTask::cancel_counter() == 0, "Some tasks were canceled");
}

#if __TBB_RESUMABLE_TASKS
struct suspended_task : public tbb::detail::d1::task {

    suspended_task(tbb::task::suspend_point tag, tbb::detail::d1::wait_context& wait)
        : my_suspend_tag(tag), my_wait(wait)
    {}

    task* execute(tbb::detail::d1::execution_data&) override {
        tbb::parallel_for(tbb::blocked_range<std::size_t>(0, 100000),
            [] (const tbb::blocked_range<std::size_t>& range) {
                // Make some heavy work
                std::atomic<int> sum{};
                for (auto it = range.begin(); it != range.end(); ++it) {
                    ++sum;
                }
            },
            tbb::static_partitioner{}
        );

        my_wait.release();
        tbb::task::resume(my_suspend_tag);
        return nullptr;
    }

    task* cancel(tbb::detail::d1::execution_data&) override {
        FAIL("The function should never be called.");
        return nullptr;
    }

    tbb::task::suspend_point my_suspend_tag;
    tbb::detail::d1::wait_context& my_wait;
};

//! \brief \ref error_guessing
TEST_CASE("Isolation + resumable tasks") {
    std::atomic<int> suspend_flag{};
    tbb::task_group_context test_context;

    std::atomic<int> suspend_count{};
    std::atomic<int> resume_count{};

    tbb::parallel_for(tbb::blocked_range<std::size_t>(0, 100000),
        [&suspend_flag, &test_context, &suspend_count, &resume_count] (const tbb::blocked_range<std::size_t>& range) {
            int ticket = 0;
            for (auto it = range.begin(); it != range.end(); ++it) {
                ticket = suspend_flag++;
            }

            if (ticket % 5 == 0) {
                std::vector<suspended_task, tbb::cache_aligned_allocator<suspended_task>> test_task;
                tbb::detail::d1::wait_context wait(1);
                ++suspend_count;
                tbb::this_task_arena::isolate([&wait, &test_context, &test_task] {
                    auto thread_id = std::this_thread::get_id();
                    tbb::task::suspend([&wait, &test_context, &test_task, thread_id] (tbb::task::suspend_point tag) {
                        CHECK(thread_id == std::this_thread::get_id());
                        test_task.emplace_back(tag, wait);
                        tbb::detail::d1::spawn(test_task[0], test_context);
                    });
                    }
                );
                tbb::detail::d1::wait(wait, test_context);
                ++resume_count;
            }
        }
    );

    CHECK(suspend_count == resume_count);
}

struct bypass_task : public tbb::detail::d1::task {
    using task_pool_type = std::vector<bypass_task, tbb::cache_aligned_allocator<bypass_task>>;

    bypass_task(tbb::detail::d1::wait_context& wait, task_pool_type& task_pool,
                std::atomic<int>& resume_flag, tbb::task::suspend_point& suspend_tag)
        : my_wait(wait), my_task_pool(task_pool), my_resume_flag(resume_flag), my_suspend_tag(suspend_tag)
    {}

    task* execute(tbb::detail::d1::execution_data&) override {
        utils::doDummyWork(10000);

        int expected = 1;
        if (my_resume_flag.compare_exchange_strong(expected, 2)) {
            tbb::task::resume(my_suspend_tag);
        }

        std::size_t ticket = my_current_task++;
        task* next = ticket < my_task_pool.size() ? &my_task_pool[ticket] : nullptr;

        if (!next && my_resume_flag != 2) {
            // Rarely all tasks can be executed before the suspend.
            // So, wait for the suspend before leaving.
            utils::SpinWaitWhileEq(my_resume_flag, 0);
            expected = 1;
            if (my_resume_flag.compare_exchange_strong(expected, 2)) {
                tbb::task::resume(my_suspend_tag);
            }
        }

        my_wait.release();
        return next;
    }

    task* cancel(tbb::detail::d1::execution_data&) override {
        FAIL("The function should never be called.");
        return nullptr;
    }

    tbb::detail::d1::wait_context& my_wait;
    task_pool_type& my_task_pool;
    std::atomic<int>& my_resume_flag;
    tbb::task::suspend_point& my_suspend_tag;
    static std::atomic<int> my_current_task;
};

std::atomic<int> bypass_task::my_current_task(0);

thread_local int test_tls = 0;

//! \brief \ref error_guessing
TEST_CASE("Bypass suspended by resume") {
    std::uint32_t task_number = 500 * static_cast<std::uint32_t>(utils::get_platform_max_threads());
    tbb::task_group_context test_context;
    tbb::detail::d1::wait_context wait(task_number + 1);

    test_tls = 1;

    std::atomic<int> resume_flag{0};
    tbb::task::suspend_point test_suspend_tag;

    std::vector<bypass_task, tbb::cache_aligned_allocator<bypass_task>> test_task_pool;

    for (std::size_t i = 0; i < task_number; ++i) {
        test_task_pool.emplace_back(wait, test_task_pool, resume_flag, test_suspend_tag);
    }

    for (std::size_t i = 0; i < utils::get_platform_max_threads(); ++i) {
        std::size_t ticket = bypass_task::my_current_task++;
        if (ticket < test_task_pool.size()) {
            tbb::detail::d1::spawn(test_task_pool[ticket], test_context);
        }
    }

    auto suspend_func = [&resume_flag, &test_suspend_tag] {
        auto thread_id = std::this_thread::get_id();
        tbb::task::suspend([&resume_flag, &test_suspend_tag, thread_id] (tbb::task::suspend_point tag) {
            CHECK(thread_id == std::this_thread::get_id());
            test_suspend_tag = tag;
            resume_flag = 1;
        });
    };
    using task_type = CountingTask<decltype(suspend_func)>;
    task_type suspend_task(suspend_func, wait);

    tbb::detail::d1::execute_and_wait(suspend_task, test_context, wait, test_context);
    CHECK(bypass_task::my_current_task >= test_task_pool.size());
    REQUIRE_MESSAGE(test_tls == 1, "The wrong thread came out");
}

//! \brief \ref error_guessing
TEST_CASE("Critical tasks + resume") {
    std::uint32_t task_number = 500 * static_cast<std::uint32_t>(utils::get_platform_max_threads());

    tbb::task_group_context test_context;
    tbb::detail::d1::wait_context wait{ 0 };

    // The test expects at least one thread in test_arena
    int num_threads_in_test_arena = std::max(2, int(utils::get_platform_max_threads()));
    tbb::global_control thread_limit(tbb::global_control::max_allowed_parallelism, num_threads_in_test_arena);
    tbb::task_arena test_arena(num_threads_in_test_arena);

    test_arena.initialize();

    std::atomic<bool> resume_flag{}, resumed{};
    tbb::task::suspend_point test_suspend_tag;

    auto task_body = [&resume_flag, &resumed, &test_suspend_tag] {
        // Make some work
        utils::doDummyWork(1000);

        if (resume_flag.exchange(false)) {
            tbb::task::resume(test_suspend_tag);
            resumed = true;
        }
    };

    using task_type = CountingTask<decltype(task_body)>;
    std::vector<task_type, tbb::cache_aligned_allocator<task_type>> test_tasks;

    for (std::size_t i = 0; i < task_number; ++i) {
        test_tasks.emplace_back(task_body, wait);
    }

    wait.reserve(task_number / 2);
    for (std::size_t i = 0; i < task_number / 2; ++i) {
        submit(test_tasks[i], test_arena, test_context, true);
    }

    auto suspend_func = [&resume_flag, &test_suspend_tag] {
        auto thread_id = std::this_thread::get_id();
        tbb::task::suspend([&resume_flag, &test_suspend_tag, thread_id] (tbb::task::suspend_point tag) {
            CHECK(thread_id == std::this_thread::get_id());
            test_suspend_tag = tag;
            resume_flag.store(true, std::memory_order_release);
        });
    };
    using suspend_task_type = CountingTask<decltype(suspend_func)>;
    suspend_task_type suspend_task(suspend_func, wait);

    wait.reserve(1);
    submit(suspend_task, test_arena, test_context, true);

    test_arena.execute([&wait, &test_tasks, &test_arena, &test_context, &resumed, task_number] {
        tbb::this_task_arena::isolate([&wait, &test_tasks, &test_arena, &test_context, &resumed, task_number] {
            do {
                wait.reserve(task_number / 2);
                tbb::parallel_for(tbb::blocked_range<std::size_t>(task_number / 2, task_number),
                    [&test_tasks, &test_arena, &test_context] (tbb::blocked_range<std::size_t>& range) {
                        for (std::size_t i = range.begin(); i != range.end(); ++i) {
                            submit(test_tasks[i], test_arena, test_context, true);
                        }
                    }
                );
            } while (!resumed);
        });
    });

    test_arena.execute([&wait, &test_context] {
        tbb::detail::d1::wait(wait, test_context);
    });
}

//! \brief \ref error_guessing
TEST_CASE("Stress testing") {
    std::uint32_t task_number = static_cast<std::uint32_t>(utils::get_platform_max_threads());

    tbb::task_group_context test_context;
    tbb::detail::d1::wait_context wait(task_number);

    tbb::task_arena test_arena;

    test_arena.initialize();

    auto task_body = [] {
        tbb::parallel_for(tbb::blocked_range<std::size_t>(0, 1000), [] (tbb::blocked_range<std::size_t>&) {
            utils::doDummyWork(100);
        });
    };
    using task_type = CountingTask<decltype(task_body)>;

    std::size_t iter_counter = 20;

    std::vector<task_type, tbb::cache_aligned_allocator<task_type>> test_tasks;

    for (std::size_t j = 0; j < task_number; ++j) {
        test_tasks.emplace_back(task_body, wait);
    }

    test_arena.execute([&test_tasks, &task_body, &wait, &test_context, &test_arena, iter_counter, task_number] {
        for (std::size_t i = 0; i < iter_counter; ++i) {

            for (std::size_t j = 0; j < task_number; ++j) {
                test_arena.enqueue(task_body);
            }

            for (std::size_t j = 0; j < task_number / 2; ++j) {
                tbb::detail::d1::spawn(test_tasks[j], test_context);
            }

            for (std::size_t j = task_number / 2; j < task_number; ++j) {
                submit(test_tasks[j], test_arena, test_context, true);
            }

            tbb::detail::d1::wait(wait, test_context);
            wait.reserve(task_number);
        }
        wait.release(task_number);
    });


    REQUIRE_MESSAGE(task_type::execute_counter() == task_number * iter_counter, "Some task was not executed");
    REQUIRE_MESSAGE(task_type::cancel_counter() == 0, "Some task was canceled");
}

//! \brief \ref error_guessing
TEST_CASE("All workers sleep") {
    std::uint32_t thread_number = static_cast<std::uint32_t>(utils::get_platform_max_threads());
    tbb::concurrent_vector<tbb::task::suspend_point> suspend_points;

    tbb::task_group test_gr;

    utils::SpinBarrier barrier(thread_number);
    auto resumble_task = [&] {
        barrier.wait();
        auto thread_id = std::this_thread::get_id();
        tbb::task::suspend([&] (tbb::task::suspend_point sp) {
            CHECK(thread_id == std::this_thread::get_id());
            suspend_points.push_back(sp);
            barrier.wait();
        });
    };

    for (std::size_t i = 0; i < thread_number - 1; ++i) {
        test_gr.run(resumble_task);
    }

    barrier.wait();
    barrier.wait();
    TestCPUUserTime(thread_number);

    for (auto sp : suspend_points)
        tbb::task::resume(sp);
    test_gr.wait();
}

#endif // __TBB_RESUMABLE_TASKS

//! \brief \ref error_guessing
TEST_CASE("Enqueue with exception") {
    std::uint32_t task_number = 500 * static_cast<std::uint32_t>(utils::get_platform_max_threads());

    tbb::task_group_context test_context;
    tbb::detail::d1::wait_context wait(task_number);

    tbb::task_arena test_arena{int(std::thread::hardware_concurrency() + 1)};

    test_arena.initialize();

    auto task_body = [] {
        utils::doDummyWork(100);
    };

    std::atomic<bool> end_flag{false};
    auto check_body = [&end_flag] {
        end_flag.store(true, std::memory_order_relaxed);
    };

    using task_type = CountingTask<decltype(task_body)>;
    std::vector<task_type, tbb::cache_aligned_allocator<task_type>> test_tasks;

    for (std::size_t j = 0; j < task_number; ++j) {
        test_tasks.emplace_back(task_body, wait);
    }

    {
        tbb::global_control gc(tbb::global_control::max_allowed_parallelism, 1);
        test_arena.enqueue(task_body);
        // Initialize implicit arena
        tbb::parallel_for(0, 1, [] (int) {});
        tbb::task_arena test_arena2(tbb::task_arena::attach{});
        test_arena2.enqueue(task_body);
    }

    constexpr std::size_t iter_count = 10;
    for (std::size_t k = 0; k < iter_count; ++k) {
        tbb::global_control gc(tbb::global_control::max_allowed_parallelism, 1);
        test_arena.enqueue(check_body);

        while (!end_flag.load(std::memory_order_relaxed)) ;

        utils::Sleep(1);
        end_flag.store(false, std::memory_order_relaxed);

        test_arena.execute([&test_tasks, &wait, &test_context, task_number] {
            for (std::size_t j = 0; j < task_number; ++j) {
                tbb::detail::d1::spawn(test_tasks[j], test_context);
            }

            tbb::detail::d1::wait(wait, test_context);
            wait.reserve(task_number);
        });
    }
    wait.release(task_number);


    REQUIRE_MESSAGE(task_type::execute_counter() == task_number * iter_count, "Some task was not executed");
    REQUIRE_MESSAGE(task_type::cancel_counter() == 0, "Some task was canceled");
}

struct resubmitting_task : public tbb::detail::d1::task {
    tbb::task_arena& my_arena;
    tbb::task_group_context& my_ctx;
    std::atomic<int> counter{100000};

    resubmitting_task(tbb::task_arena& arena, tbb::task_group_context& ctx) : my_arena(arena), my_ctx(ctx)
    {}

    tbb::detail::d1::task* execute(tbb::detail::d1::execution_data& ) override {
        if (counter-- > 0) {
            submit(*this, my_arena, my_ctx, true);
        }
        return nullptr;
    }

    tbb::detail::d1::task* cancel( tbb::detail::d1::execution_data& ) override {
        FAIL("The function should never be called.");
        return nullptr;
    }
};

//! \brief \ref error_guessing
TEST_CASE("Test with priority inversion") {
    if (!utils::can_change_thread_priority()) return;

    std::uint32_t thread_number = static_cast<std::uint32_t>(utils::get_platform_max_threads());
    tbb::global_control gc(tbb::global_control::max_allowed_parallelism, thread_number + 1);

    tbb::task_arena test_arena(2 * thread_number, thread_number);
    test_arena.initialize();
    utils::pinning_observer obsr(test_arena);
    CHECK_MESSAGE(obsr.is_observing(), "Arena observer has not been activated");

    std::uint32_t  critical_task_counter = 1000 * thread_number;
    std::atomic<std::size_t> task_counter{0};

    tbb::task_group_context test_context;
    tbb::detail::d1::wait_context wait(critical_task_counter);

    auto critical_work = [&] {
        utils::doDummyWork(10);
    };

    using suspend_task_type = CountingTask<decltype(critical_work)>;
    suspend_task_type critical_task(critical_work, wait);

    auto high_priority_thread_func = [&] {
        // Increase external threads priority
        utils::increase_thread_priority();
        // pin external threads
        test_arena.execute([]{});
        while (task_counter++ < critical_task_counter) {
            submit(critical_task, test_arena, test_context, true);
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
        }
    };

    resubmitting_task worker_task(test_arena, test_context);
    // warm up
    // take first core on execute
    utils::SpinBarrier barrier(thread_number + 1);
    test_arena.execute([&] {
        tbb::parallel_for(std::uint32_t(0), thread_number + 1, [&] (std::uint32_t&) {
            barrier.wait();
            submit(worker_task, test_arena, test_context, true);
        });
    });

    std::vector<std::thread> high_priority_threads;
    for (std::size_t i = 0; i < thread_number - 1; ++i) {
        high_priority_threads.emplace_back(high_priority_thread_func);
    }

    utils::increase_thread_priority();
    while (task_counter++ < critical_task_counter) {
        submit(critical_task, test_arena, test_context, true);
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }

    tbb::detail::d1::wait(wait, test_context);

    for (std::size_t i = 0; i < thread_number - 1; ++i) {
        high_priority_threads[i].join();
    }
}

// Explicit test for raii_guard move ctor because of copy elision optimization
// TODO: consider better test file for the test case
//! \brief \ref interface
TEST_CASE("raii_guard move ctor") {
    int count{0};
    auto func = [&count] {
        count++;
        CHECK(count == 1);
    };

    tbb::detail::d0::raii_guard<decltype(func)> guard1(func);
    tbb::detail::d0::raii_guard<decltype(func)> guard2(std::move(guard1));
}
