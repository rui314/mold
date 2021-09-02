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

#ifndef __TBB_test_common_parallel_invoke_common_H
#define __TBB_test_common_parallel_invoke_common_H

#include "config.h"

#include <cstddef>
#include <tuple>

#include "oneapi/tbb/parallel_invoke.h"
#include "oneapi/tbb/global_control.h"

#include "dummy_body.h"

// Helps generate a tuple of functional objects
template<std::size_t Counter, template <std::size_t> class Functor, class Generator, typename... Fs>
struct generate_tuple_impl :
    public generate_tuple_impl<Counter - 1, Functor, Generator, typename Generator::template type<Functor, Counter - 1>, Fs...> {};

template<template <std::size_t> class Functor, class Generator, typename... Fs>
struct generate_tuple_impl <0, Functor, Generator, Fs...> {
    using type = std::tuple<Fs...>;
};

template<std::size_t Size, template <std::size_t> class Functor, class Generator>
using generate_tuple = typename generate_tuple_impl<Size, Functor, Generator>::type;

// Defines how generate_tuple should propagate Counter to the tuple members
struct generator {
    template <template <std::size_t> class Functor, std::size_t NonTypeArg>
    using type = Functor<NonTypeArg>;
};

template <std::size_t Fixed>
struct fixed_generator {
    template <template <std::size_t> class Functor, std::size_t NonTypeArg>
    using type = Functor<Fixed>;
};

template<std::size_t LevelTaskCount, std::size_t MaxDepth, std::size_t WorkSize>
struct invoke_tree {
    struct invoke_tree_leaf {
        static constexpr std::size_t size = WorkSize;
        void operator()() const { utils::doDummyWork(size); }
    };

    // Make invoke_tree_leaf composable with generate_tuple interface
    template <std::size_t NonTypeArg>
    using generating_leaf_functor = invoke_tree_leaf;

    template<std::size_t CurrentDepth>
    struct invoke_tree_node {
        template<typename... Fs>
        void invoker(const std::tuple<Fs...>&) const {
            tbb::parallel_invoke(Fs()...);
        }

        // Template alias to workaround the strange Visual Studio issue
        template<std::size_t Depth>
        using generating_node_functor = invoke_tree_node<Depth>;

        void create_level(/*is_final_level*/std::false_type) const {
            invoker(generate_tuple<LevelTaskCount, generating_node_functor, fixed_generator<CurrentDepth + 1>>{});
        }

        void create_level(/*is_final_level*/std::true_type) const {
            invoker(generate_tuple<LevelTaskCount, generating_leaf_functor, generator>());
        }

        void operator()() const {
            create_level(/*is_final_level*/std::integral_constant<bool, CurrentDepth == MaxDepth>());
        }
    };

    static void generate_and_run() {
        invoke_tree_node<1> tree_root;
        tree_root();
    }
};

template<std::size_t TaskCount, template<std::size_t> class Functor>
class parallel_invoke_call {
    template<typename... Fs>
    static void invoke_tuple_args(tbb::task_group_context* context_ptr, const std::tuple<Fs...>&) {
        if (context_ptr != nullptr) {
            tbb::parallel_invoke(Fs()..., *context_ptr);
        } else {
            tbb::parallel_invoke(Fs()...);
        }
    }

public:
    static void perform(tbb::task_group_context* context_ptr = nullptr) {
        invoke_tuple_args(context_ptr, generate_tuple<TaskCount, Functor, generator>{});
    }
};

#endif // __TBB_test_common_parallel_invoke_common_H
