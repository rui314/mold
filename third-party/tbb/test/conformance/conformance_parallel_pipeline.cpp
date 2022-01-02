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
#include "common/checktype.h"
#include "common/spin_barrier.h"
#include "common/utils_concurrency_limit.h"

#include "oneapi/tbb/parallel_pipeline.h"
#include "oneapi/tbb/global_control.h"
#include "oneapi/tbb/task_group.h"

#include <atomic>
#include <thread>
#include <string.h>
#include <memory>
#include <tuple>

//! \file conformance_parallel_pipeline.cpp
//! \brief Test for [algorithms.parallel_pipeline algorithms.parallel_pipeline.flow_control] specification

constexpr std::size_t n_tokens = 8;

constexpr int max_counter = 1024;

static std::atomic<int> input_counter{ max_counter };

template<typename U>
class input_filter {
public:
    U operator()( oneapi::tbb::flow_control& control ) const {
        if( --input_counter < 0 ) {
            control.stop();
            input_counter = max_counter;
        }
        return U();
    }
};

template<typename T, typename U>
class middle_filter {
public:
    U operator()(T) const {
        U out{};
        return out;
    }
};

template<typename T>
class output_filter {
public:
    void operator()(T ) const {}
};

static const oneapi::tbb::filter_mode filter_table[] = { oneapi::tbb::filter_mode::parallel,
                                                 oneapi::tbb::filter_mode::serial_in_order,
                                                 oneapi::tbb::filter_mode::serial_out_of_order};

template<typename Body, typename... Cotnext>
void TestSingleFilter(Body body, Cotnext&... context) {

    for(int i =0; i <3; i++)
    {
        oneapi::tbb::filter_mode mode = filter_table[i];

        oneapi::tbb::filter<void, void> one_filter( mode, body );
        oneapi::tbb::parallel_pipeline( n_tokens, one_filter, context...   );

        oneapi::tbb::parallel_pipeline( n_tokens, oneapi::tbb::filter<void, void>(mode, body), context... );

        oneapi::tbb::parallel_pipeline( n_tokens, oneapi::tbb::make_filter<void, void>(mode, body), context...);
    }
}

void TestSingleFilterFunctor() {

    input_filter<void> i_filter;

    TestSingleFilter(i_filter);

    oneapi::tbb::task_group_context context;
    TestSingleFilter(i_filter,  context);
}


void TestSingleFilterLambda() {


    TestSingleFilter([]( oneapi::tbb::flow_control& control ) {
                    if(input_counter-- == 0 ) {
                        control.stop();
                        input_counter = max_counter;
                        }
                    } );

    oneapi::tbb::task_group_context context;
    TestSingleFilter([]( oneapi::tbb::flow_control& control ) {
                     if(input_counter-- == 0 ) {
                        control.stop();
                        input_counter = max_counter;
                        }
                    },  context);
}

template<typename I, typename O>
void RunPipeline(const oneapi::tbb::filter<I, O> &filter)
{
    bool flag{false};

    auto f_beg = oneapi::tbb::make_filter<void, I>(oneapi::tbb::filter_mode::serial_out_of_order,
                                        [&flag](oneapi::tbb::flow_control& fc) -> I{
                                            if(flag) {
                                                fc.stop();
                                            }
                                            flag = true;
                                            return I();
                                        });

    auto f_end = oneapi::tbb::make_filter<O, void>(oneapi::tbb::filter_mode::serial_in_order,
                                            [](O) {});

    oneapi::tbb::parallel_pipeline(n_tokens, f_beg & filter & f_end);
}

void RunPipeline(const oneapi::tbb::filter<void, void> &filter)
{
    oneapi::tbb::parallel_pipeline(n_tokens, filter);
}

template<typename Iterator1, typename Iterator2>
void RootSequence( Iterator1 first, Iterator1 last, Iterator2 res) {
    using ValueType = typename Iterator1::value_type;
    oneapi::tbb::parallel_pipeline( n_tokens,
        oneapi::tbb::make_filter<void,ValueType>(
            oneapi::tbb::filter_mode::serial_in_order,
            [&first, &last](oneapi::tbb::flow_control& fc)-> ValueType{
                if( first<last ) {
                    ValueType val  = *first;
                    ++first;
                    return val;
                 } else {
                    fc.stop();
                    return ValueType{};
                }
            }
        ) &
        oneapi::tbb::make_filter<ValueType,ValueType>(
            oneapi::tbb::filter_mode::parallel,
            [](ValueType p){return p*p;}
        ) &
        oneapi::tbb::make_filter<ValueType,void>(
            oneapi::tbb::filter_mode::serial_in_order,
            [&res](ValueType x) {
                *res = x;
                ++res; }
        )
    );
}

//! Testing pipeline correctness
//! \brief \ref interface \ref requirement
TEST_CASE("Testing pipeline correctness")
{
    std::vector<double> input(max_counter);
    std::vector<double> output(max_counter);
    for(std::size_t i = 0; i < max_counter; i++)
        input[i] = (double)i;

    RootSequence(input.cbegin(), input.cend(), output.begin());
    for(int  i = 0; i < max_counter; i++) {
        CHECK_MESSAGE(output[i] == input[i]*input[i], "pipeline result is incorrect");
    }
}

//! Testing pipeline with single filter
//! \brief \ref interface \ref requirement
TEST_CASE("Testing pipeline with single filter")
{
    TestSingleFilterFunctor();
    TestSingleFilterLambda();
}

//! Testing single filter with different ways of creation
//! \brief \ref interface \ref requirement
TEST_CASE_TEMPLATE("Filter creation testing", T, std::tuple<size_t, int>,
                                                 std::tuple<int, double>,
                                                 std::tuple<unsigned int*, size_t>,
                                                 std::tuple<unsigned short, unsigned short*>,
                                                 std::tuple<double*, unsigned short*>,
                                                 std::tuple<std::unique_ptr<int>, std::unique_ptr<int> >)
{
    using I = typename std::tuple_element<0, T>::type;
    using O = typename std::tuple_element<1, T>::type;
    for(int i = 0; i < 3; i++)
    {
        oneapi::tbb::filter_mode mode = filter_table[i];
        oneapi::tbb::filter<I, O> default_filter;

        auto made_filter1 = oneapi::tbb::make_filter<I,O>(mode, [](I)->O{return O();});
        static_assert(std::is_same<oneapi::tbb::filter<I, O>, decltype(made_filter1)>::value, "make_filter wrong result type");
        RunPipeline(made_filter1);

        auto made_filter2 = oneapi::tbb::make_filter(mode, [](I)->O{return O();});
        static_assert(std::is_same<oneapi::tbb::filter<I, O>, decltype(made_filter2)>::value, "make_filter wrong result type");
        RunPipeline(made_filter2);

        oneapi::tbb::filter<I, O> one_filter(mode, [](I)->O{return O();});
        RunPipeline(one_filter);

        oneapi::tbb::filter<I, O> copy_filter(one_filter);
        RunPipeline(one_filter);

        default_filter = copy_filter;
        RunPipeline(default_filter);
        default_filter.clear();
    }
}

//! Testing filters concatenation
//! \brief \ref interface \ref requirement
TEST_CASE_TEMPLATE("Testing filters concatenation", T, std::tuple<size_t, int>,
                                                       std::tuple<int, double>,
                                                       std::tuple<unsigned int*, size_t>,
                                                       std::tuple<unsigned short, unsigned short*>,
                                                       std::tuple<double*, unsigned short*>,
                                                       std::tuple<std::unique_ptr<int>, std::unique_ptr<int> >)
{
    using I = typename std::tuple_element<0, T>::type;
    using O = typename std::tuple_element<1, T>::type;

    for(int fi = 0; fi< 27; fi++)
    {
        int i = fi%3;
        int j = (fi/3)%3;
        int k = (fi/9);
        auto filter_chain = oneapi::tbb::filter<void, I>(filter_table[i], input_filter<I>()) &
                            oneapi::tbb::filter<I, O>(filter_table[j], middle_filter<I,O>()) &
                            oneapi::tbb::filter<O, void>(filter_table[k], output_filter<O>());
        RunPipeline(filter_chain);

        oneapi::tbb::filter<void, I> filter1 = oneapi::tbb::filter<void, I>(filter_table[i], input_filter<I>());
        oneapi::tbb::filter<I, O> filter2 = oneapi::tbb::filter<I, O>(filter_table[j], middle_filter<I,O>());
        oneapi::tbb::filter<O, void> filter3 = oneapi::tbb::filter<O, void>(filter_table[k], output_filter<O>());

        auto fitler12 = filter1 & filter2;
        auto fitler23 = filter2 & filter3;
        auto fitler123 = filter1 & filter2 & filter3;

        RunPipeline(fitler12 & filter3);
        RunPipeline(filter1 & fitler23);
        RunPipeline(fitler123);
    }
}

void doWork() {
    for (int i = 0; i < 10; ++i)
        utils::yield();
}

//! Testing filter modes
//! \brief \ref interface \ref requirement
TEST_CASE("Testing filter modes")
{
    for ( auto concurrency_level : utils::concurrency_range() )
    {
        oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_level);

        short serial_checker{0};
        oneapi::tbb::filter<void,short> filter1(oneapi::tbb::filter_mode::serial_out_of_order,
                                [&serial_checker](oneapi::tbb::flow_control&fc)
                                {
                                    auto check_value = ++serial_checker;
                                    doWork();
                                    CHECK_MESSAGE(check_value == serial_checker, "a serial filter was executed concurrently");
                                    if(serial_checker>=(short)max_counter)
                                    {
                                        fc.stop();
                                    }
                                    return check_value;
                                });

        short serial_checker2{ 0 };
        oneapi::tbb::filter<short, short> filter2(oneapi::tbb::filter_mode::serial_in_order,
            [&serial_checker2](int)
            {
                auto check_value = ++serial_checker2;
                doWork();
                CHECK_MESSAGE(check_value == serial_checker2, "a serial filter was executed concurrently");
                return check_value;
            });

        utils::SpinBarrier spin_barrier(utils::min(concurrency_level, n_tokens), true);
        oneapi::tbb::filter<short,int> filter3(oneapi::tbb::filter_mode::parallel,
                                [&spin_barrier](int value)
                                {
                                    spin_barrier.wait();
                                    doWork();
                                    return value;
                                });


        short order_checker{0};
        oneapi::tbb::filter<int,void> filter4(oneapi::tbb::filter_mode::serial_in_order,
                                [&order_checker](int value)
                                {
                                    CHECK_MESSAGE(++order_checker == value, "the order of message was broken");
                                });

        oneapi::tbb::parallel_pipeline(n_tokens, filter1 & filter2 & filter3 & filter4);
    }
}

//! Testing max tocken number
//! \brief \ref interface \ref requirement
TEST_CASE("Testing max token number")
{
    for(unsigned int i = 1; i < n_tokens; i++)
    {
        std::atomic<short> active_tokens{0};

        oneapi::tbb::filter<void,int> filter1(oneapi::tbb::filter_mode::parallel,
                                [&active_tokens](oneapi::tbb::flow_control&fc)
                                {
                                    ++active_tokens;
                                    doWork();
                                    CHECK_MESSAGE(active_tokens <= n_tokens, "max number of tokens is exceed");
                                    --active_tokens;
                                    if (--input_counter < 0) {
                                        fc.stop();
                                        input_counter = max_counter;
                                    }
                                    return 0;
                                });

        oneapi::tbb::filter<int,int> filter2(oneapi::tbb::filter_mode::parallel,
                                [&active_tokens](int value)
                                {
                                    ++active_tokens;
                                    doWork();
                                    CHECK_MESSAGE(active_tokens <= n_tokens, "max number of tockens is exceed");
                                    --active_tokens;
                                    return value;
                                });

        oneapi::tbb::filter<int,void> filter3(oneapi::tbb::filter_mode::serial_out_of_order,
                                [&active_tokens](int)
                                {
                                    ++active_tokens;
                                    doWork();
                                    CHECK_MESSAGE(active_tokens <= n_tokens, "max number of tockens is exceed");
                                    --active_tokens;
                                });

        oneapi::tbb::parallel_pipeline(i, filter1 & filter2 & filter3);
    }
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

//! Testing deduction guides
//! \brief \ref interface \ref requirement
TEST_CASE_TEMPLATE("Deduction guides testing", T, int, unsigned int, double)
{
    input_filter<T> i_filter;
    oneapi::tbb::filter  fc1(oneapi::tbb::filter_mode::serial_in_order, i_filter);
    static_assert(std::is_same_v<decltype(fc1), oneapi::tbb::filter<void, T>>);

    oneapi::tbb::filter fc2 (fc1);
    static_assert(std::is_same_v<decltype(fc2), oneapi::tbb::filter<void, T>>);

    middle_filter<T, std::size_t> m_filter;
    oneapi::tbb::filter  fc3(oneapi::tbb::filter_mode::serial_in_order, m_filter);
    static_assert(std::is_same_v<decltype(fc3), oneapi::tbb::filter<T, std::size_t>>);

    oneapi::tbb::filter frv(oneapi::tbb::filter_mode::serial_in_order, [](int&&) -> double { return 0.0; });
    oneapi::tbb::filter fclv(oneapi::tbb::filter_mode::serial_in_order, [](const int&) -> double { return 0.0; });
    oneapi::tbb::filter fc(oneapi::tbb::filter_mode::serial_in_order, [](const int) -> double { return 0.0; });

    static_assert(std::is_same_v<decltype(frv), oneapi::tbb::filter<int, double>>);
    static_assert(std::is_same_v<decltype(fclv), oneapi::tbb::filter<int, double>>);
    static_assert(std::is_same_v<decltype(fc), oneapi::tbb::filter<int, double>>);
}
#endif  //__TBB_CPP17_DEDUCTION_GUIDES_PRESENT
