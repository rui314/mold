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

#ifndef TBB_examples_convex_hull_H
#define TBB_examples_convex_hull_H

#include <cassert>
#include <cstdlib>
#include <cstring>
#include <climits>

#include <iostream>
#include <iomanip>
#include <sstream>
#include <vector>
#include <string>
#include <algorithm>
#include <functional>

#include "oneapi/tbb/tick_count.h"
#include "oneapi/tbb/global_control.h"

#include "common/utility/utility.hpp"
#include "common/utility/get_default_num_threads.hpp"
#include "common/utility/fast_random.hpp"

namespace cfg {
// convex hull problem user set parameters
long numberOfPoints = 5000000; // problem size

// convex hull grain sizes for 3 subproblems. Be sure 16*GS < 512Kb
const std::size_t generateGrainSize = 25000;
const std::size_t findExtremumGrainSize = 25000;
const std::size_t divideGrainSize = 25000;
}; // namespace cfg

namespace util {
bool silent = false;
bool verbose = false;
std::vector<std::string> OUTPUT;

// utility functionality
void ParseInputArgs(int argc, char* argv[], utility::thread_number_range& threads) {
    utility::parse_cli_arguments(
        argc,
        argv,
        utility::cli_argument_pack()
            //"-h" option for displaying help is present implicitly
            .positional_arg(threads, "n-of-threads", utility::thread_number_range_desc)
            .positional_arg(cfg::numberOfPoints, "n-of-points", "number of points")
            .arg(silent, "silent", "no output except elapsed time")
            .arg(verbose, "verbose", "turns verbose ON"));
    //disabling verbose if silent is specified
    if (silent)
        verbose = false;
    ;
}

template <typename T>
struct point {
    T x;
    T y;
    //According to subparagraph 4 of paragraph 12.6.2 "Initializing bases and members" [class.base.init]
    //of ANSI-ISO-IEC C++ 2003 standard, POD members will _not_ be initialized if they are not mentioned
    //in the base-member initializer list.

    //For more details why this needed please see comment in FillRNDPointsVector_buf
    point() {}
    point(T _x, T _y) : x(_x), y(_y) {}
};

std::ostream& operator<<(std::ostream& o, point<double> const& p) {
    return o << "(" << p.x << "," << p.y << ")";
}

struct rng {
    static const std::size_t max_rand = USHRT_MAX;
    utility::FastRandom my_fast_random;
    rng(std::size_t seed) : my_fast_random(seed) {}
    unsigned short operator()() {
        return my_fast_random.get();
    }
    unsigned short operator()(std::size_t& seed) {
        return my_fast_random.get(seed);
    }
};

template <typename T, typename rng_functor_type>
point<T> GenerateRNDPoint(std::size_t& count, rng_functor_type random, std::size_t rand_max) {
    /* generates random points on 2D plane so that the cluster
        is somewhat circle shaped */
    const std::size_t maxsize = 500;
    T x = random() * 2.0 / (double)rand_max - 1;
    T y = random() * 2.0 / (double)rand_max - 1;
    T r = (x * x + y * y);
    if (r > 1) {
        count++;
        if (count > 10) {
            if (random() / (double)rand_max > 0.5)
                x /= r;
            if (random() / (double)rand_max > 0.5)
                y /= r;
            count = 0;
        }
        else {
            x /= r;
            y /= r;
        }
    }

    x = (x + 1) * 0.5 * maxsize;
    y = (y + 1) * 0.5 * maxsize;

    return point<T>(x, y);
}

template <typename Index>
struct edge {
    Index start;
    Index end;
    edge(Index _p1, Index _p2) : start(_p1), end(_p2){};
};

template <typename T>
std::ostream& operator<<(std::ostream& _ostr, point<T> _p) {
    return _ostr << '(' << _p.x << ',' << _p.y << ')';
}

template <typename T>
std::istream& operator>>(std::istream& _istr, point<T> _p) {
    return _istr >> _p.x >> _p.y;
}

template <typename T>
bool operator==(point<T> p1, point<T> p2) {
    return (p1.x == p2.x && p1.y == p2.y);
}

template <typename T>
bool operator!=(point<T> p1, point<T> p2) {
    return !(p1 == p2);
}

template <typename T>
double cross_product(const point<T>& start, const point<T>& end1, const point<T>& end2) {
    return ((end1.x - start.x) * (end2.y - start.y) - (end2.x - start.x) * (end1.y - start.y));
}

// Timing functions are based on TBB to always obtain wall-clock time
typedef oneapi::tbb::tick_count my_time_t;

my_time_t gettime() {
    return oneapi::tbb::tick_count::now();
}

double time_diff(my_time_t start, my_time_t end) {
    return (end - start).seconds();
}

void WriteResults(int nthreads, double initTime, double calcTime) {
    if (verbose) {
        std::cout << " Step by step hull construction:"
                  << "\n";
        for (std::size_t i = 0; i < OUTPUT.size(); ++i)
            std::cout << OUTPUT[i] << "\n";
    }
    if (!silent) {
        std::cout << "  Number of nodes:" << cfg::numberOfPoints
                  << "  Number of threads:" << nthreads << "  Initialization time:" << std::setw(10)
                  << std::setprecision(3) << initTime << "  Calculation time:" << std::setw(10)
                  << std::setprecision(3) << calcTime << "\n";
    }
}
}; // namespace util

#endif /* TBB_examples_convex_hull_H */
