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

#ifndef __TBB_test_common_concurrent_lru_cache_common
#define __TBB_test_common_concurrent_lru_cache_common


#include "test.h"
#include "utils.h"
#include <tbb/concurrent_lru_cache.h>

//-----------------------------------------------------------------------------
// Concurrent LRU Cache Tests: Cache Helpers
//-----------------------------------------------------------------------------

namespace concurrent_lru_cache_helpers {

    // call counter

    template<std::size_t id>
    struct tag{};

    template<typename Tag, typename Type>
    struct call_counter {
        static int calls;

        static Type call(Type t) {
            ++calls;
            return t;
        }
    };

    template<typename Tag, typename Type>
    int call_counter<Tag, Type>::calls = 0;

    // cloner

    template<typename ValueType>
    struct cloner {
        ValueType& my_ref_to_original;

        cloner(ValueType& ref_to_original) : my_ref_to_original(ref_to_original) {}

        template<typename KeyType>
        ValueType operator()(KeyType) const { return my_ref_to_original; }
    };

    // map searcher

    template <typename KeyType, typename ValueType>
    struct map_searcher {
        using map_type = std::map<KeyType, ValueType>;
        using map_iter = typename map_type::iterator;

        map_type& my_map_ref;

        map_searcher(map_type& map_ref) : my_map_ref(map_ref) {}

        ValueType& operator()(KeyType k) {
            map_iter it = my_map_ref.find(k);
            if (it == my_map_ref.end())
                it = my_map_ref.insert(it, std::make_pair(k, ValueType()));

            return it->second;
        }
    };

    // array searcher

    template<typename key_type, typename value_type, std::size_t array_size>
    struct array_searcher {
        using array_type = value_type[array_size];

        array_type const& my_array_ref;

        array_searcher(array_type const& array_ref) : my_array_ref(array_ref) {}

        const value_type& operator()(key_type k) const {
            std::size_t index = k;
            REQUIRE_MESSAGE(k < array_size, "incorrect test setup");
            return my_array_ref[index];
        }
    };

    // instance counter

    template<typename CounterType = std::size_t>
    struct instance_counter {
        CounterType* my_p_count;

        instance_counter() : my_p_count(new CounterType) {
            *my_p_count = 1;
        }

        instance_counter(instance_counter const& other) : my_p_count(other.my_p_count) {
            ++(*my_p_count);
        }

        instance_counter& operator=(instance_counter other) {
            std::swap(this->my_p_count, other.my_p_count);
            return *this;
        }

        ~instance_counter() {
            bool is_last = ! --(*my_p_count);
#if __GNUC__ == 12
        // GCC 12 warns about using my_p_count after delete.
        // The test was investigated and no problems were detected
        // The following statement silence the warning
        static bool unused_is_last = is_last;
        utils::suppress_unused_warning(unused_is_last);
#endif
            if (is_last)
                delete(my_p_count);
        }
        std::size_t instances_count() const { return *my_p_count; }
    };

    using instance_serial_counter = instance_counter<>;
    using instance_concurrent_counter = instance_counter<std::atomic<std::size_t>>;
};

//-----------------------------------------------------------------------------
// Concurrent LRU Cache Tests: Cache Presets
//-----------------------------------------------------------------------------

namespace concurrent_lru_cache_presets {

    namespace helpers = concurrent_lru_cache_helpers;

    // base preset with common typedefs and fields

    template<typename... CacheTypes>
    struct preset_base {
        using cache_type = tbb::concurrent_lru_cache<CacheTypes...>;
        using handle_type = typename cache_type::handle;

        preset_base() {}
        preset_base(const preset_base&) = delete;
        preset_base(preset_base&&) = delete;
        preset_base& operator=(const preset_base&) = delete;
        preset_base& operator=(preset_base&&) = delete;
    };

    // default preset

    template<typename Key, typename Value>
    struct preset_default : preset_base<Key, Value> {
        using cache_type = typename preset_base<Key, Value>::cache_type;
        using handle_type = typename cache_type::handle;
        using callback_type = typename cache_type::value_function_type;

        const std::size_t number_of_lru_history_items;
        cache_type cache;

        preset_default(callback_type callback, std::size_t history_items) :
            number_of_lru_history_items(history_items),
            cache(callback, number_of_lru_history_items) {};
    };

    // preset1

    struct preset1 : preset_base<std::string, std::string> {
        const std::size_t number_of_lru_history_items;
        cache_type cache;
        handle_type default_ctor_check;

        static std::string callback(std::string key) { return key; }

        preset1() :
            number_of_lru_history_items(1),
            cache(&callback, number_of_lru_history_items) {};
    };

    // preset for call counting

    template<std::size_t tag_id>
    struct preset_call_count : preset_base<int, int> {
        using cache_miss_tag = helpers::tag<tag_id>;
        using counter_type = helpers::call_counter<cache_miss_tag, int>;

        const std::size_t number_of_lru_history_items;
        cache_type cache;

        preset_call_count() :
            number_of_lru_history_items(8),
            cache(&counter_type::call, number_of_lru_history_items) {}
    };

    // preset for instance counting

    struct preset_instance_count : preset_base<
        std::size_t, helpers::instance_serial_counter,
        helpers::cloner<helpers::instance_serial_counter>> {

        using cloner_type = helpers::cloner<helpers::instance_serial_counter>;

        helpers::instance_serial_counter source;
        cloner_type cloner;
        const std::size_t number_of_lru_history_items;
        cache_type cache;

        preset_instance_count() :
            cloner(source),
            number_of_lru_history_items(8),
            cache(cloner, number_of_lru_history_items) {}
    };

    // preset for instance counting with external map

    struct preset_map_instance_count : preset_base<
        std::size_t, helpers::instance_serial_counter,
        helpers::map_searcher<std::size_t, helpers::instance_serial_counter>> {

        using map_searcher_type = helpers::map_searcher<std::size_t, helpers::instance_serial_counter>;
        using objects_map_type = map_searcher_type::map_type;

        static const std::size_t number_of_lru_history_items;
        map_searcher_type::map_type objects_map;
        cache_type cache;

        preset_map_instance_count() :
            cache(map_searcher_type(objects_map), number_of_lru_history_items) {}

        bool is_evicted(std::size_t key) {
            objects_map_type::iterator it = objects_map.find(key);

            REQUIRE_MESSAGE(
                it != objects_map.end(),
                "no value for key - error in test logic ?");

            return it->second.instances_count() == 1;
        }

        void fill_up_cache(std::size_t lower_bound, std::size_t upper_bound) {
            for (std::size_t i = lower_bound; i < upper_bound; ++i)
                cache[i];
        }
    };

    const std::size_t preset_map_instance_count::number_of_lru_history_items = 8;
};

#endif // __TBB_test_common_concurrent_lru_cache_common
