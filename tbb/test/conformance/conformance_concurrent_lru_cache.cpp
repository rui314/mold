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

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#define TBB_PREVIEW_CONCURRENT_LRU_CACHE 1

#include "common/test.h"
#include "oneapi/tbb/concurrent_lru_cache.h"
#include <common/concurrent_lru_cache_common.h>

//! \file conformance_concurrent_lru_cache.cpp
//! \brief Test for [preview] functionality

//-----------------------------------------------------------------------------
// Concurrent LRU Cache Tests: Cache Test Cases
//-----------------------------------------------------------------------------

//! \brief \ref interface \ref requirement
TEST_CASE("basic test for creation and use") {
    using preset = concurrent_lru_cache_presets::preset_default<int, int>;

    auto callback = [](int key) -> int { return key; };
    std::size_t number_of_lru_history_items = 8;

    preset preset_object{callback, number_of_lru_history_items};
    preset::cache_type& cache = preset_object.cache;

    int dummy_key = 0;
    preset::handle_type h = cache[dummy_key];
    int value = h.value();
    (void)value;
}

//! \brief \ref interface \ref requirement
TEST_CASE("basic test for std::move") {
    using preset = concurrent_lru_cache_presets::preset1;
    preset preset_object{};
    preset::cache_type& cache = preset_object.cache;

    // retain handle object to keep an item in the cache
    // if it is still active without aging
    preset::handle_type sheep = cache["sheep"];
    preset::handle_type horse = cache["horse"];
    preset::handle_type bull = cache["bull"];

    // store handler objects in vector
    std::vector<preset::handle_type> animals;
    animals.reserve(5);
    animals.emplace_back(std::move(sheep));
    animals.emplace_back(std::move(horse));
    animals[0] = std::move(bull);

    // after resize() vec will be full of default constructed handlers
    // with null pointers on item in cache and on cache which item belongs to
    animals.resize(10);
}

//! \brief \ref interface \ref requirement
TEST_CASE("basic test for to bool conversion") {
    using concurrent_lru_cache_presets::preset1;

    preset1 preset_instance{};
    preset1::cache_type& cache = preset_instance.cache;


    preset1::handle_type handle;
    preset1::handle_type foobar = preset1::handle_type();

    handle = cache["handle"];

    preset1::handle_type foo = cache["bar"];

    preset1::handle_type handle1(std::move(handle));

    handle = std::move(handle1);

    REQUIRE_MESSAGE(
        !preset1::handle_type(),
        "user-defined to-bool conversion does not work");
    REQUIRE_MESSAGE(
        handle,
        "user-defined to-bool conversion does not work");

    handle = preset1::handle_type();
}

//! \brief \ref requirement
TEST_CASE("basic test for cache hit") {
    using preset = concurrent_lru_cache_presets::preset_call_count<__LINE__>;
    preset preset_object{};
    preset::cache_type& cache = preset_object.cache;

    int dummy_key = 0;
    cache[dummy_key];
    cache[dummy_key];

    REQUIRE_MESSAGE(
        preset::counter_type::calls == 1,
        "value function should be called only on a cache miss");
}

//! \brief \ref requirement
TEST_CASE("basic test for unused objects") {
    using preset = concurrent_lru_cache_presets::preset_instance_count;
    preset preset_object{};

    for (std::size_t i = 0; i < preset_object.number_of_lru_history_items; ++i)
        preset_object.cache[i];

    REQUIRE_MESSAGE(
        preset_object.source.instances_count() == preset_object.number_of_lru_history_items+1,
        "cache should respect number of stored unused objects to number passed in constructor");
}

