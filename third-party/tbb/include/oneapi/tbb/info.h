/*
    Copyright (c) 2019-2022 Intel Corporation
    Copyright (c) 2026 UXL Foundation Contributors

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

#ifndef __TBB_info_H
#define __TBB_info_H

#include "detail/_config.h"
#include "detail/_namespace_injection.h"
#include "detail/_utils.h"
#include "version.h"

#include <tuple>
#include <vector>
#include <cstdint>

namespace tbb {
namespace detail {

namespace d1{

using numa_node_id = int;
using core_type_id = int;

// TODO: consider version approach to resolve backward compatibility potential issues.
struct constraints {
#if !__TBB_CPP20_PRESENT
    constraints(numa_node_id id = -1, int maximal_concurrency = -1)
        : numa_id(id)
        , max_concurrency(maximal_concurrency)
    {}
#endif /*!__TBB_CPP20_PRESENT*/

    constraints& set_numa_id(numa_node_id id) {
        numa_id = id;
        return *this;
    }
    constraints& set_max_concurrency(int maximal_concurrency) {
        max_concurrency = maximal_concurrency;
        return *this;
    }
    constraints& set_core_type(core_type_id id) {
        core_type = id;
        return *this;
    }
    constraints& set_max_threads_per_core(int threads_number) {
        max_threads_per_core = threads_number;
        return *this;
    }

    numa_node_id numa_id = -1;
    int max_concurrency = -1;
    core_type_id core_type = -1;
    int max_threads_per_core = -1;
};

} // namespace d1

namespace r1 {
TBB_EXPORT unsigned __TBB_EXPORTED_FUNC numa_node_count();
TBB_EXPORT void __TBB_EXPORTED_FUNC fill_numa_indices(int* index_array);
TBB_EXPORT int __TBB_EXPORTED_FUNC numa_default_concurrency(int numa_id);

// Reserved fields are required to save binary backward compatibility in case of future changes.
// They must be defined to 0 at this moment.
TBB_EXPORT unsigned __TBB_EXPORTED_FUNC core_type_count(intptr_t reserved = 0);
TBB_EXPORT void __TBB_EXPORTED_FUNC fill_core_type_indices(int* index_array, intptr_t reserved = 0);

TBB_EXPORT int __TBB_EXPORTED_FUNC constraints_default_concurrency(const d1::constraints& c, intptr_t reserved = 0);
TBB_EXPORT int __TBB_EXPORTED_FUNC constraints_threads_per_core(const d1::constraints& c, intptr_t reserved = 0);
} // namespace r1

namespace d1 {

inline std::vector<numa_node_id> numa_nodes() {
    std::vector<numa_node_id> node_indices(r1::numa_node_count());
    r1::fill_numa_indices(node_indices.data());
    return node_indices;
}

inline int default_concurrency(numa_node_id id = -1) {
    return r1::numa_default_concurrency(id);
}

inline std::vector<core_type_id> core_types() {
    std::vector<int> core_type_indexes(r1::core_type_count());
    r1::fill_core_type_indices(core_type_indexes.data());
    return core_type_indexes;
}

inline int default_concurrency(constraints c) {
    if (c.max_concurrency > 0) { return c.max_concurrency; }
    return r1::constraints_default_concurrency(c);
}

#if __TBB_PREVIEW_TASK_ARENA_CORE_TYPE_SELECTOR
// Call a custom selector on the available core type(s) and encode those selected
template <typename Selector>
inline core_type_id apply_core_type_selector(Selector selector) {
    constexpr core_type_id automatic = -1;

    auto ids = core_types();
    size_t total = ids.size();
    if (total < 2) {
        // Not enough core types to select from, so use the default
        return automatic;
    }

    int max_score = 0, max_score_id = -1, num_zero_scores = 0;
    std::vector<core_type_id> selected_core_types;
    for (size_t index = 0; index < total; ++index) {
        int score = selector(std::make_tuple(ids[index], index, total));
        if (score > 0) {
            selected_core_types.push_back(ids[index]);
        }
        else if (score == 0) {
            ++num_zero_scores;
        }

        if (TBB_runtime_interface_version() < 12180) {
            if (score > max_score) {
                max_score = score;
                max_score_id = ids[index];
            }
        }
    }
    if (TBB_runtime_interface_version() < 12180) {
        // No runtime multi core type support, so select all or one
        if (selected_core_types.size() + num_zero_scores == total) {
            selected_core_types.clear(); // all
        }
        else if (!selected_core_types.empty()) {
            selected_core_types = { max_score_id }; // the one with the highest score
        }
    }
    return multi_core_type_codec::encode(selected_core_types);
}

template <typename Selector>
inline int default_concurrency(constraints c, Selector selector) {
    constexpr core_type_id selectable = -2;
    if (c.core_type == selectable) {
        c.core_type = apply_core_type_selector(selector);
    }
    return default_concurrency(c);
}
#endif

} // namespace d1
} // namespace detail

inline namespace v1 {
using detail::d1::numa_node_id;
using detail::d1::core_type_id;

namespace info {
using detail::d1::numa_nodes;
using detail::d1::core_types;

using detail::d1::default_concurrency;
} // namespace info
} // namespace v1

} // namespace tbb

#endif /*__TBB_info_H*/
