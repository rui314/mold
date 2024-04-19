/*
    Copyright (c) 2023 Intel Corporation

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

#ifndef TWO_TASKS_HEADER
#define TWO_TASKS_HEADER

#include "task_emulation_layer.h"

#include <iostream>
#include <numeric>
#include <utility>
#include <functional>

extern int cutoff;

long serial_fib(int n) {
    return n < 2 ? n : serial_fib(n - 1) + serial_fib(n - 2);
}

struct fib_continuation : task_emulation::base_task {
    fib_continuation(int& s) : sum(s) {}

    void execute() override {
        sum = x + y;
    }

    int x{ 0 }, y{ 0 };
    int& sum;
};

struct fib_computation : task_emulation::base_task {
    fib_computation(int n, int* x) : n(n), x(x) {}

    void execute() override {
        if (n < cutoff) {
            *x = serial_fib(n);
        }
        else {
            // Continuation passing
            auto& c = *this->allocate_continuation<fib_continuation>(/* children_counter = */ 2, *x);
            task_emulation::run_task(c.create_child<fib_computation>(n - 1, &c.x));

            // Recycling
            this->recycle_as_child_of(c);
            n = n - 2;
            x = &c.y;

            // Bypass is not supported by task_emulation and next_task executed directly.
            // However, the old-TBB bypass behavior can be achieved with
            // `return task_group::defer()` (check Migration Guide).
            // Consider submit another task if recursion call is not acceptable
            // i.e. instead of Recycling + Direct Body call
            // submit task_emulation::run_task(c.create_child<fib_computation>(n - 2, &c.y));
            this->operator()();
        }
    }

    int n;
    int* x;
};

int fibonacci_two_tasks(int n) {
    int sum{};
    tbb::task_group tg;
    tg.run_and_wait(
        task_emulation::create_root_task<fib_computation>(/* for root task = */ tg, n, &sum));
    return sum;
}

#endif // TWO_TASKS_HEADER
