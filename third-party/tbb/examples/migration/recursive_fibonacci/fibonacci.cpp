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

#include "fibonacci_single_task.h"
#include "fibonacci_two_tasks.h"

#include <iostream>
#include <numeric>
#include <utility>

int cutoff;
bool testing_enabled;

template <typename F>
std::pair</* result */ unsigned long, /* time */ unsigned long> measure(F&& f,
                                                                        int number,
                                                                        unsigned long ntrial) {
    std::vector<unsigned long> times;

    unsigned long result;
    for (unsigned long i = 0; i < ntrial; ++i) {
        auto t1 = std::chrono::steady_clock::now();
        result = f(number);
        auto t2 = std::chrono::steady_clock::now();

        auto time = std::chrono::duration_cast<std::chrono::milliseconds>(t2 - t1).count();
        times.push_back(time);
    }

    return std::make_pair(
        result,
        static_cast<unsigned long>(std::accumulate(times.begin(), times.end(), 0) / times.size()));
}

int main(int argc, char* argv[]) {
    int numbers = argc > 1 ? strtol(argv[1], nullptr, 0) : 50;
    cutoff = argc > 2 ? strtol(argv[2], nullptr, 0) : 16;
    unsigned long ntrial = argc > 3 ? (unsigned long)strtoul(argv[3], nullptr, 0) : 20;
    testing_enabled = argc > 4 ? (bool)strtol(argv[4], nullptr, 0) : false;

    auto res = measure(fibonacci_two_tasks, numbers, ntrial);
    std::cout << "Fibonacci two tasks impl N = " << res.first << " Avg time = " << res.second
              << " ms" << std::endl;

    res = measure(fibonacci_single_task, numbers, ntrial);
    std::cout << "Fibonacci single task impl N = " << res.first << " Avg time = " << res.second
              << " ms" << std::endl;
}
