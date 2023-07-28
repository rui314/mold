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

#define TBB_PREVIEW_CONCURRENT_LRU_CACHE 1

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#include "common/test.h"
#include "common/utils.h"
#include <tbb/concurrent_lru_cache.h>
#include <common/concurrent_lru_cache_common.h>

//! \file test_concurrent_lru_cache.cpp
//! \brief Test for [preview] functionality

//-----------------------------------------------------------------------------
// Concurrent LRU Cache Tests: Cache Test Cases
//-----------------------------------------------------------------------------

//! \brief \ref error_guessing
TEST_CASE("basic test for return value") {
    using preset = concurrent_lru_cache_presets::preset_default<int, int>;

    auto dummy_f = [](int /*key*/) -> int { return 0xDEADBEEF; };
    std::size_t number_of_lru_history_items = 8;

    preset preset_object{dummy_f, number_of_lru_history_items};
    preset::cache_type& cache = preset_object.cache;

    int dummy_key = 1;
    REQUIRE_MESSAGE(
        dummy_f(dummy_key) == cache[dummy_key].value(),
        "cache operator() must return only values obtained from value function");
}

//! \brief \ref error_guessing
TEST_CASE("basic test for unused objects") {
    using preset = concurrent_lru_cache_presets::preset_instance_count;
    preset preset_object{};

    for (std::size_t i = 0; i < preset_object.number_of_lru_history_items; ++i)
        preset_object.cache[i];

    REQUIRE_MESSAGE(
        preset_object.source.instances_count() > 1,
        "cache should store some unused objects");
}

//! \brief \ref error_guessing
TEST_CASE("basic test for unused object limit") {
    using preset = concurrent_lru_cache_presets::preset_instance_count;
    preset preset_object{};

    for (std::size_t i = 0; i < preset_object.number_of_lru_history_items + 1; ++i)
        preset_object.cache[i];

    REQUIRE_MESSAGE(
        preset_object.source.instances_count() == preset_object.number_of_lru_history_items + 1,
        "cache should respect number of stored unused objects to number passed in constructor");
}

//! \brief \ref error_guessing
TEST_CASE("basic test for eviction order") {
    using preset = concurrent_lru_cache_presets::preset_map_instance_count;
    preset preset_object{};

    REQUIRE_MESSAGE(
        preset_object.number_of_lru_history_items > 2,
        "incorrect test setup");

    preset_object.fill_up_cache(0, preset_object.number_of_lru_history_items);

    // heat up first element
    preset_object.cache[0];

    // cause eviction;
    preset_object.cache[preset_object.number_of_lru_history_items];

    bool is_correct = preset_object.is_evicted(1) && !preset_object.is_evicted(0);
    REQUIRE_MESSAGE(is_correct, "cache should evict items in lru order");
}

//! \brief \ref error_guessing
TEST_CASE("basic test for eviction of only unused items") {
    using preset = concurrent_lru_cache_presets::preset_map_instance_count;
    preset preset_object{};

    preset::handle_type h = preset_object.cache[0];

    //cause eviction
    preset_object.fill_up_cache(1, preset_object.number_of_lru_history_items+2);

    bool is_correct = preset_object.is_evicted(1) && !preset_object.is_evicted(0);
    REQUIRE_MESSAGE(is_correct, "cache should not evict items in use");
}

//! \brief \ref error_guessing
TEST_CASE("basic test for eviction of only unused items 2") {
    using preset = concurrent_lru_cache_presets::preset_map_instance_count;
    preset preset_object{};

    preset::handle_type h = preset_object.cache[0];
    {
        preset::handle_type h1 = preset_object.cache[0];
    }

    //cause eviction
    preset_object.fill_up_cache(1,preset_object.number_of_lru_history_items+2);

    bool is_correct = preset_object.is_evicted(1) && !preset_object.is_evicted(0);
    REQUIRE_MESSAGE(is_correct, "cache should not evict items in use");
}

//! \brief \ref error_guessing
TEST_CASE("basic test for handling case when number_of_lru_history_items is zero") {
    auto foo = [] (int) {
        return utils::LifeTrackableObject{};
    };
    using cache_type =  tbb::concurrent_lru_cache<int, utils::LifeTrackableObject, decltype(foo)>;
    cache_type cache{foo, 0};
    
    for(int i = 0; i < 10; ++i) {
        // Check that no history is stored when my_history_list_capacity is 0.
        // In this case, when trying to fill the cache, the items will be deleted if reference was not taken.
        const utils::LifeTrackableObject* obj_addr = &cache[1].value();
        REQUIRE_MESSAGE(utils::LifeTrackableObject::is_alive(obj_addr) == false, "when number_of_lru_history_items is zero, element must be erased after use");
    }

    cache_type::handle h = cache[1];
    const utils::LifeTrackableObject* obj_addr = &h.value();
    auto& object_set = utils::LifeTrackableObject::set();
    for(int i = 0; i < 10; ++i) {
        // Verify that item will still be alive if there is a handle holding that item.
        cache[1];
        REQUIRE_MESSAGE(utils::LifeTrackableObject::is_alive(obj_addr), "the object with the key=1 was destroyed but should not");
        REQUIRE_MESSAGE(object_set.size() == 1, "no other values should be added");
    }
}
