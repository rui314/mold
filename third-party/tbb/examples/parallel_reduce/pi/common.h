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

#ifndef TBB_examples_pi_H
#define TBB_examples_pi_H

#include <cstdlib>

typedef std::size_t number_t;
typedef double pi_t;

extern const number_t chunk_size;
extern number_t num_intervals;
extern pi_t step;

extern bool silent;

inline pi_t pi_kernel(number_t i) {
    pi_t dx = (pi_t(i) + pi_t(0.5)) * step;
    return pi_t(4.0) / (pi_t(1.0) + dx * dx);
}

inline double pi_slice_kernel(number_t slice, number_t slice_size = chunk_size) {
    pi_t pi = pi_t(0.0);
    for (number_t i = slice; i < slice + slice_size; ++i) {
        pi += pi_kernel(i);
    }
    return pi;
}

struct threading {
    threading(int p);
    ~threading();
};

double compute_pi_parallel();

#endif //  TBB_examples_pi_H
