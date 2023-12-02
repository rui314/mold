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

#ifndef SINGLE_TASK_HEADER
#define SINGLE_TASK_HEADER

#include "task_emulation_layer.h"

#include <iostream>
#include <numeric>
#include <utility>

extern int cutoff;

long serial_fib_1(int n) {
    return n < 2 ? n : serial_fib_1(n - 1) + serial_fib_1(n - 2);
}

struct single_fib_task : task_emulation::base_task {
    enum class state {
        compute,
        sum
    };

    single_fib_task(int n, int* x) : n(n), x(x), s(state::compute)
    {}

    void execute() override {
        switch (s) {
            case state::compute : {
                compute_impl();
                break;
            }
            case state::sum : {
                *x = x_l + x_r;
                break;
            }
        }
    }

    void compute_impl() {
        if (n < cutoff) {
            *x = serial_fib_1(n);
        }
        else {
            auto bypass = this->allocate_child_and_increment<single_fib_task>(n - 2, &x_r);
            task_emulation::run_task(this->allocate_child_and_increment<single_fib_task>(n - 1, &x_l));

            // Recycling
            this->s = state::sum;
            this->recycle_as_continuation();

            // Bypass is not supported by task_emulation and next_task executed directly.
            // However, the old-TBB bypass behavior can be achieved with
            // `return task_group::defer()` (check Migration Guide).
            // Consider submit another task if recursion call is not acceptable
            // i.e. instead of Direct Body call
            // submit task_emulation::run_task(this->allocate_child_and_increment<single_fib_task>(n - 2, &x_r));
            bypass->operator()();
        }
    }


    int n;
    int* x;
    state s;

    int x_l{ 0 }, x_r{ 0 };
};

int fibonacci_single_task(int n) {
    int sum{};
    tbb::task_group tg;
    task_emulation::run_and_wait(tg, task_emulation::allocate_root_task<single_fib_task>(/* for root task = */ tg, n, &sum));
    return sum;
}

#endif // SINGLE_TASK_HEADER
