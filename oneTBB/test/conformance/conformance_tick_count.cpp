/*
    Copyright (c) 2020-2021 Intel Corporation

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

#include "oneapi/tbb/tick_count.h"
#include "oneapi/tbb/detail/_template_helpers.h"
#include <type_traits>

//! \file conformance_tick_count.cpp
//! \brief Test for [timing] specification

//! Testing default construction
//! \brief \ref interface
TEST_CASE("Default construction") {
    oneapi::tbb::tick_count t1;
    utils::suppress_unused_warning(t1);
}

//! Testing subtraction operation
//! Subtraction of two equal tick_counts after call seconds must be equal to 0
//! \brief \ref interface \ref requirement
TEST_CASE("Subtraction of equal tick_counts") {
    oneapi::tbb::tick_count tick_f = oneapi::tbb::tick_count::now();
    oneapi::tbb::tick_count tick_s(tick_f);
    CHECK_EQ((tick_f - tick_s).seconds(), 0);
}

//! Testing subsequent timestamp
//! \brief \ref requirement
TEST_CASE("Subtraction subsequent timestamp") {
    oneapi::tbb::tick_count tick_f(oneapi::tbb::tick_count::now());
    oneapi::tbb::tick_count tick_s(oneapi::tbb::tick_count::now());
    while ((tick_s - tick_f).seconds() == 0) {
        tick_s = oneapi::tbb::tick_count::now();
    }
    CHECK_GT((tick_s - tick_f).seconds(), 0);
}

// Wait for given duration.
// The duration parameter is in units of seconds.
static void WaitForDuration( double duration ) {
    CHECK_GT(duration, 0);
    oneapi::tbb::tick_count start = oneapi::tbb::tick_count::now();
    double sec = 0;
    do {
        sec = (oneapi::tbb::tick_count::now() - start).seconds();
        CHECK_GE(sec, 0);
    } while (sec < duration);
}

// CHECK that two times in seconds are very close.
void CheckNear( double x, double y ) {
    CHECK_GE(x-y, -1.0E-10);
    CHECK_GE(1.0E-10, x-y);
}

//! Test arithmetic operators on tick_count::interval_t
//! \brief \ref interface \ref requirement
TEST_CASE("Arithmetic operators") {
    oneapi::tbb::tick_count t1 = oneapi::tbb::tick_count::now();
    WaitForDuration(0.000001);
    oneapi::tbb::tick_count t2 = oneapi::tbb::tick_count::now();
    WaitForDuration(0.0000012);
    oneapi::tbb::tick_count t3 = oneapi::tbb::tick_count::now();

    using interval_type = oneapi::tbb::tick_count::interval_t;
    interval_type i = t2 - t1;
    interval_type j = t3 - t2;
    interval_type k = t3 - t1;
    CHECK_EQ(std::is_same<oneapi::tbb::tick_count::interval_t, decltype(i - j)>::value, true);
    CHECK_EQ(std::is_same<oneapi::tbb::tick_count::interval_t, decltype(i + j)>::value, true);
    CheckNear((i + j).seconds(), k.seconds());
    CheckNear((k - j).seconds(), i.seconds());
    CheckNear(((k - j) + (j - i)).seconds(), k.seconds() - i.seconds());
    interval_type sum;
    sum += i;
    sum += j;
    CheckNear(sum.seconds(), k.seconds());
    sum -= i;
    CheckNear(sum.seconds(), j.seconds());
    sum -= j;
    CheckNear(sum.seconds(), 0.0);
}


//! Test resolution of oneapi::tbb::tick_count::interval_t
//! \brief \ref interface \ref requirement
TEST_CASE("oneapi::tbb::tick_count::interval_t resolution") {
    static double target_value = 0.314159265358979323846264338327950288419;
    static double step_value = 0.00027182818284590452353602874713526624977572;
    static int range_value = 100;
    for (int i = -range_value; i <= range_value; ++i) {
        double my_time = target_value + step_value * i;
        oneapi::tbb::tick_count::interval_t t0(my_time);
        double interval_time = t0.seconds();
        //! time always truncates
        CHECK_GE(interval_time, 0);
        CHECK_LT(my_time - interval_time, oneapi::tbb::tick_count::resolution());
    }
}
