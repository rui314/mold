/*
    Copyright (c) 2019-2022 Intel Corporation

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

#ifndef __TBB_test_common_arena_constraints_H_
#define __TBB_test_common_arena_constraints_H_

#if _WIN32 || _WIN64
#define _CRT_SECURE_NO_WARNINGS
#endif

#include "common/test.h"
#include "common/spin_barrier.h"
#include "common/utils.h"
#include "common/memory_usage.h"
#include "common/utils_concurrency_limit.h"

#include "oneapi/tbb/task_arena.h"
#include "oneapi/tbb/spin_mutex.h"

#include <vector>
#include <unordered_set>

#if (_WIN32 || _WIN64) && __TBB_HWLOC_VALID_ENVIRONMENT
#include <windows.h>
int get_processors_group_count() {
    SYSTEM_INFO si;
    GetNativeSystemInfo(&si);
    DWORD_PTR pam, sam, m = 1;
    GetProcessAffinityMask( GetCurrentProcess(), &pam, &sam );
    int nproc = 0;
    for ( std::size_t i = 0; i < sizeof(DWORD_PTR) * CHAR_BIT; ++i, m <<= 1 ) {
        if ( pam & m )
            ++nproc;
    }
    // Setting up processor groups in case the process does not restrict affinity mask and more than one processor group is present
    if ( nproc == (int)si.dwNumberOfProcessors  ) {
        // The process does not have restricting affinity mask and multiple processor groups are possible
        return (int)GetActiveProcessorGroupCount();
    } else {
        return 1;
    }
}
#else
int get_processors_group_count() { return 1; }
#endif

//TODO: Write a test that checks for memory leaks during dynamic link/unlink of TBBbind.
#if __TBB_HWLOC_VALID_ENVIRONMENT
#include "oneapi/tbb/concurrent_unordered_set.h"

#include <atomic>
#include <algorithm>

#if _MSC_VER
#if __clang__
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wunused-parameter"
#else
#pragma warning( push )
#pragma warning( disable : 4100 )
#endif
#endif
#include <hwloc.h>
#if _MSC_VER
#if __clang__
#pragma GCC diagnostic pop
#else
#pragma warning( pop )
#endif
#endif

#define __HWLOC_HYBRID_CPUS_INTERFACES_PRESENT (HWLOC_API_VERSION >= 0x20400)
#define __HWLOC_TOPOLOGY_FLAG_RESTRICT_TO_CPUBINDING_PRESENT (HWLOC_API_VERSION >= 0x20500)
// At this moment the hybrid CPUs HWLOC interfaces returns unexpected results on some Windows machines
// in the 32-bit arch mode.
#define __HWLOC_HYBRID_CPUS_INTERFACES_VALID (!_WIN32 || _WIN64)

#define __HYBRID_CPUS_TESTING __HWLOC_HYBRID_CPUS_INTERFACES_PRESENT && __HWLOC_HYBRID_CPUS_INTERFACES_VALID

// Macro to check hwloc interfaces return codes
#define hwloc_require_ex(command, ...)                                          \
        REQUIRE_MESSAGE(command(__VA_ARGS__) >= 0, "Error occurred inside hwloc call.");

struct index_info {
    int index{-1};
    int concurrency{-1};
    hwloc_bitmap_t cpuset{nullptr};

    index_info() = default;
    index_info(const index_info& src)
        : index{src.index}
        , concurrency{src.concurrency}
        , cpuset{hwloc_bitmap_dup(src.cpuset)}
    {}

    index_info& operator=(index_info src) {
        index = src.index;
        concurrency = src.concurrency;
        std::swap(cpuset, src.cpuset);
        return *this;
    }

    ~index_info() {
        hwloc_bitmap_free(cpuset);
    }
};

struct core_info {
    hwloc_bitmap_t cpuset{nullptr};

    core_info() = default;
    core_info(hwloc_bitmap_t _cpuset)
        : cpuset{hwloc_bitmap_dup(_cpuset)}
    {}
    core_info(const core_info& src)
        : cpuset{hwloc_bitmap_dup(src.cpuset)}
    {}

    core_info& operator=(core_info src) {
        std::swap(cpuset, src.cpuset);
        return *this;
    }

    ~core_info() {
        hwloc_bitmap_free(cpuset);
    }
};

class system_info {
    hwloc_topology_t topology;
    hwloc_cpuset_t process_cpuset{nullptr};

    std::vector<index_info> numa_node_infos{};
    std::vector<index_info> cpu_kind_infos{};
    std::vector<core_info> core_infos{};

    // hwloc_cpuset_t and hwloc_nodeset_t (inherited from hwloc_bitmap_t ) is pointers,
    // so we must manage memory allocation and deallocation
    using memory_handler_t = tbb::concurrent_unordered_set<hwloc_bitmap_t>;
    memory_handler_t memory_handler{};

    static system_info* system_info_ptr;

public:
    static void initialize() {
        static system_info topology_instance;
        system_info_ptr = &topology_instance;
    }

private:
    static system_info& instance() {
        REQUIRE_MESSAGE(system_info_ptr, "Get access to the uninitialize system info.(reference)");
        return *system_info_ptr;
    }

    system_info() {
        hwloc_require_ex(hwloc_topology_init, &topology);
#if __HWLOC_TOPOLOGY_FLAG_RESTRICT_TO_CPUBINDING_PRESENT
        if ( get_processors_group_count() == 1 ) {
            REQUIRE(
                hwloc_topology_set_flags(topology,
                    HWLOC_TOPOLOGY_FLAG_IS_THISSYSTEM |
                    HWLOC_TOPOLOGY_FLAG_RESTRICT_TO_CPUBINDING) == 0
            );
        }
#endif
        hwloc_require_ex(hwloc_topology_load, topology);

        if ( get_processors_group_count() > 1 ) {
            process_cpuset = hwloc_bitmap_dup(hwloc_topology_get_complete_cpuset(topology));
        } else {
            process_cpuset = hwloc_bitmap_alloc();
            hwloc_require_ex(hwloc_get_cpubind, topology, process_cpuset, 0);
        }

        hwloc_obj_t current_numa_node = nullptr;
        index_info current_node_info{};
        while ((current_numa_node = hwloc_get_next_obj_by_type(topology,
                                                               HWLOC_OBJ_NUMANODE,
                                                               current_numa_node)) != nullptr) {
            current_node_info.index = static_cast<int>(current_numa_node->logical_index);
            current_node_info.cpuset = hwloc_bitmap_dup(current_numa_node->cpuset);
            hwloc_bitmap_and(current_node_info.cpuset, current_node_info.cpuset, process_cpuset);
            current_node_info.concurrency = hwloc_bitmap_weight(current_node_info.cpuset);
            if(current_node_info.concurrency) {
                numa_node_infos.push_back(current_node_info);
            }
        }

        if (numa_node_infos.empty()) {
            current_node_info.index = 0;
            current_node_info.cpuset = hwloc_bitmap_dup(process_cpuset);
            current_node_info.concurrency = hwloc_bitmap_weight(process_cpuset);
            numa_node_infos.push_back(current_node_info);
        }

        std::sort(numa_node_infos.begin(), numa_node_infos.end(), 
            [](const index_info& a, const index_info& b) {
                return a.index < b.index;
            }
        );

        bool core_types_parsing_broken = true;
#if __HYBRID_CPUS_TESTING
        // Getting CPU kinds info
        auto num_cpu_kinds = hwloc_cpukinds_get_nr(topology, 0);
        REQUIRE_MESSAGE(num_cpu_kinds >= 0,
            "HWLOC cannot detect the number of cpukinds.(reference)");

        core_types_parsing_broken = num_cpu_kinds == 0;
        int current_efficiency = -1;
        cpu_kind_infos.resize(num_cpu_kinds);
        for (auto kind_index = 0; kind_index < num_cpu_kinds; ++kind_index) {
            auto& cki = cpu_kind_infos[kind_index];

            cki.cpuset = hwloc_bitmap_alloc();
            CHECK_MESSAGE(
                cki.cpuset,
                "HWLOC was unable to allocate bitmap. Following checks might fail.(reference)"
            );

            hwloc_require_ex(
                hwloc_cpukinds_get_info, topology, kind_index, cki.cpuset, &current_efficiency,
                /*nr_infos*/nullptr, /*infos*/nullptr, /*flags*/0
            );
            if (current_efficiency < 0) {
                core_types_parsing_broken = true;
                break;
            }
            hwloc_bitmap_and(cki.cpuset, cki.cpuset, process_cpuset);

            cki.index = hwloc_cpukinds_get_by_cpuset(topology, cki.cpuset, /*flags*/0);
            REQUIRE_MESSAGE(cki.index >= 0,
                "hwloc failed obtaining kind index via cpuset.(reference)");

            cki.concurrency = hwloc_bitmap_weight(cki.cpuset);
        }
#endif /*__HYBRID_CPUS_TESTING*/

        if (core_types_parsing_broken) {
            cpu_kind_infos.resize(1);
            cpu_kind_infos[0].index = -1;
            cpu_kind_infos[0].cpuset = hwloc_bitmap_dup(process_cpuset);
            cpu_kind_infos[0].concurrency = hwloc_bitmap_weight(process_cpuset);
        }

        hwloc_bitmap_t core_affinity = hwloc_bitmap_alloc();
        hwloc_obj_t current_core = nullptr;
        while ((current_core = hwloc_get_next_obj_by_type(topology, HWLOC_OBJ_CORE, current_core)) != nullptr) {
            hwloc_bitmap_and(core_affinity, process_cpuset, current_core->cpuset);
            if (hwloc_bitmap_weight(core_affinity) > 0) {
                core_infos.emplace_back(core_affinity);
            }
        }
        hwloc_bitmap_free(core_affinity);

        testing_reference_topology_parsing_validation();
    }

    ~system_info() {
        for (auto& allocated_mask: memory_handler) {
            hwloc_bitmap_free(allocated_mask);
        }
        hwloc_topology_destroy(topology);

        hwloc_bitmap_free(process_cpuset);
    }

    void testing_reference_topology_parsing_validation() {
        hwloc_cpuset_t buffer_cpu_set = hwloc_bitmap_alloc();

        REQUIRE_MESSAGE(numa_node_infos.size() > 0, "Negative NUMA nodes count.(reference)");
        REQUIRE_MESSAGE(cpu_kind_infos.size() > 0, "Negative core types count.(reference)");
        REQUIRE_MESSAGE(core_infos.size() > 0, "Negative available cores count.(reference)");
#if NUMA_NODES_NUMBER
        REQUIRE_MESSAGE(numa_node_infos.size() == NUMA_NODES_NUMBER,
            "Manual NUMA nodes count validation fails.(reference)");
#endif /*NUMA_NODES_NUMBER*/
#if CORE_TYPES_NUMBER
        REQUIRE_MESSAGE(cpu_kind_infos.size() == CORE_TYPES_NUMBER,
            "Manual core types count validation fails.(reference)");
#endif /*CORE_TYPES_NUMBER*/

        // NUMA topology verification
        hwloc_bitmap_zero(buffer_cpu_set);
        for (const auto& numa_node_info: numa_node_infos) {
            REQUIRE_MESSAGE(!hwloc_bitmap_intersects(buffer_cpu_set, numa_node_info.cpuset),
                "NUMA nodes related CPUset have the same bits. "
                "It seems like error during HWLOC topology parsing.(reference)");
            hwloc_bitmap_or(buffer_cpu_set, buffer_cpu_set, numa_node_info.cpuset);
        }
        REQUIRE_MESSAGE( hwloc_bitmap_isequal(buffer_cpu_set, process_cpuset),
            "Intersected NUMA nodes masks should be equal to process affinity.(reference)");

        // Core types topology verification
        hwloc_bitmap_zero(buffer_cpu_set);
        for (const auto& cpu_kind_info: cpu_kind_infos) {
            REQUIRE_FALSE_MESSAGE(hwloc_bitmap_intersects(buffer_cpu_set, cpu_kind_info.cpuset),
                "core types related CPUset have the same bits. "
                "It seems like error during HWLOC topology parsing.(reference)");
            hwloc_bitmap_or(buffer_cpu_set, buffer_cpu_set, cpu_kind_info.cpuset);
        }
        REQUIRE_MESSAGE(hwloc_bitmap_isequal(buffer_cpu_set, process_cpuset),
            "Intersected core type masks should be equal to process affinity.(reference)");

        hwloc_bitmap_free(buffer_cpu_set);
    }

public:
    typedef hwloc_bitmap_t affinity_mask;
    typedef hwloc_const_bitmap_t const_affinity_mask;

    static hwloc_const_bitmap_t get_process_affinity_mask() {
        return instance().process_cpuset;
    }

    static std::size_t get_maximal_threads_per_core() {
        auto max_threads_it =
            std::max_element(
                std::begin(instance().core_infos), std::end(instance().core_infos),
                [](const core_info& first, const core_info& second) {
                    return hwloc_bitmap_weight(first.cpuset) < hwloc_bitmap_weight(second.cpuset);
                }
            );
        __TBB_ASSERT(hwloc_bitmap_weight(max_threads_it->cpuset) > 0,
            "Not positive maximal threads per core value.(reference)");
        return hwloc_bitmap_weight(max_threads_it->cpuset);
    }

    static affinity_mask allocate_empty_affinity_mask() {
        affinity_mask result = hwloc_bitmap_alloc();
        instance().memory_handler.insert(result);
        return result;
    }

    static affinity_mask allocate_current_affinity_mask() {
        affinity_mask result = hwloc_bitmap_alloc();
        instance().memory_handler.insert(result);
        hwloc_require_ex(hwloc_get_cpubind, instance().topology, result, HWLOC_CPUBIND_THREAD);
        REQUIRE_MESSAGE(!hwloc_bitmap_iszero(result), "Empty current affinity mask.");
        return result;
    }

    static std::vector<index_info> get_cpu_kinds_info() {
        return instance().cpu_kind_infos;
    }

    static std::vector<index_info> get_numa_nodes_info() {
        return instance().numa_node_infos;
    }

    static std::vector<core_info> get_cores_info() {
        return instance().core_infos;
    }

    static std::vector<int> get_available_max_threads_values() {
        std::vector<int> result{};
        for (int value = -1; value <= (int)get_maximal_threads_per_core(); ++value) {
            if (value != 0) {
                result.push_back(value);
            }
        }
        return result;
    }
}; // class system_info

system_info* system_info::system_info_ptr{nullptr};

system_info::affinity_mask prepare_reference_affinity_mask(const tbb::task_arena::constraints& c) {
    auto reference_affinity = system_info::allocate_empty_affinity_mask();
    hwloc_bitmap_copy(reference_affinity, system_info::get_process_affinity_mask());

    if (c.numa_id != tbb::task_arena::automatic) {
        const auto& numa_nodes_info = system_info::get_numa_nodes_info();
        auto required_info = std::find_if(numa_nodes_info.begin(), numa_nodes_info.end(),
            [&](const index_info& info) { return info.index == c.numa_id; }
        );

        REQUIRE_MESSAGE(required_info != numa_nodes_info.end(), "Constraints instance has wrong NUMA index.");
        hwloc_bitmap_and(reference_affinity, reference_affinity, required_info->cpuset);
    }

    if (c.core_type != tbb::task_arena::automatic) {
        const auto& core_types_info = system_info::get_cpu_kinds_info();
        auto required_info = std::find_if(core_types_info.begin(), core_types_info.end(),
            [&](index_info info) { return info.index == c.core_type; }
        );
        REQUIRE_MESSAGE(required_info != core_types_info.end(), "Constraints instance has wrong core type index.");
        hwloc_bitmap_and(reference_affinity, reference_affinity, required_info->cpuset);
    }

    return reference_affinity;
}

void test_constraints_affinity_and_concurrency(tbb::task_arena::constraints constraints,
                                               system_info::affinity_mask arena_affinity) {
    int default_concurrency = tbb::info::default_concurrency(constraints);
    system_info::affinity_mask reference_affinity = prepare_reference_affinity_mask(constraints);
    int max_threads_per_core = static_cast<int>(system_info::get_maximal_threads_per_core());

    if (constraints.max_threads_per_core == tbb::task_arena::automatic || constraints.max_threads_per_core == max_threads_per_core) {
        REQUIRE_MESSAGE(hwloc_bitmap_isequal(reference_affinity, arena_affinity),
            "Wrong affinity mask was applied for the constraints instance.");
        REQUIRE_MESSAGE(hwloc_bitmap_weight(reference_affinity) == default_concurrency,
            "Wrong default_concurrency was returned for the constraints instance.");
    } else {
        REQUIRE_MESSAGE(constraints.max_threads_per_core < max_threads_per_core,
            "Constraints instance has wrong max_threads_per_core value.");
        REQUIRE_MESSAGE(hwloc_bitmap_isincluded(arena_affinity, reference_affinity),
            "If custom threads per core value is applied then the applied affinity"
            "should be a sub-set of the affinity applied to constraints without such restriction.");

        system_info::affinity_mask core_affinity = system_info::allocate_empty_affinity_mask();
        int threads_per_current_core = 0, valid_concurrency = 0;
        for (const auto& current_core : system_info::get_cores_info()) {
            hwloc_bitmap_and(core_affinity, reference_affinity, current_core.cpuset);
            threads_per_current_core = hwloc_bitmap_weight(core_affinity);
            if (threads_per_current_core > 0) { // current core should exist in the valid affinity mask
                hwloc_bitmap_and(core_affinity, arena_affinity, current_core.cpuset);
                threads_per_current_core = std::min(threads_per_current_core, constraints.max_threads_per_core);
                valid_concurrency += threads_per_current_core;
                REQUIRE_MESSAGE(hwloc_bitmap_weight(core_affinity) == threads_per_current_core ,
                    "Wrong number of threads may be scheduled to some core.");
            }
        }
        REQUIRE_MESSAGE(valid_concurrency == default_concurrency,
            "Wrong default_concurrency was returned for the constraints instance.");
        REQUIRE_MESSAGE(valid_concurrency == hwloc_bitmap_weight(arena_affinity),
            "Wrong number of bits inside the affinity mask.");
    }
}

system_info::affinity_mask get_arena_affinity(tbb::task_arena& ta) {
    system_info::affinity_mask arena_affinity;
    ta.execute([&]{
        arena_affinity = system_info::allocate_current_affinity_mask();
    });

    utils::SpinBarrier exit_barrier(ta.max_concurrency());
    tbb::spin_mutex affinity_mutex{};
    for (int i = 0; i < ta.max_concurrency() - 1; ++i) {
        ta.enqueue([&] {
            {
                tbb::spin_mutex::scoped_lock lock(affinity_mutex);
                system_info::affinity_mask thread_affinity = system_info::allocate_current_affinity_mask();
                if (get_processors_group_count() == 1) {
                    REQUIRE_MESSAGE(hwloc_bitmap_isequal(thread_affinity, arena_affinity),
                        "Threads have different masks on machine without several processors groups.");
                }
                hwloc_bitmap_or(arena_affinity, arena_affinity, thread_affinity);
            }
            exit_barrier.wait();
        });
    }
    exit_barrier.wait();
    return arena_affinity;
}

#else /*__TBB_HWLOC_VALID_ENVIRONMENT*/

namespace system_info {
    // The values that seems meaningful for the most systems that exists at this moment
    // Should be used when we cannot parse the system topology
    std::vector<int> get_available_max_threads_values() { return {tbb::task_arena::automatic, 1, 2}; }
}

#endif /*!__TBB_HWLOC_VALID_ENVIRONMENT*/

struct constraints_hash {
    std::size_t operator()(const tbb::task_arena::constraints& c) const {
      return (std::hash<int>{}(c.numa_id) ^ std::hash<int>{}(c.core_type) ^ std::hash<int>{}(c.max_threads_per_core));
    }
};

struct constraints_equal {
    bool operator()(const tbb::task_arena::constraints& c1, const tbb::task_arena::constraints& c2) const {
        return (c1.numa_id == c2.numa_id &&
                c1.core_type == c2.core_type &&
                c1.max_threads_per_core == c2.max_threads_per_core);
  }
};

using constraints_container = std::unordered_set<tbb::task_arena::constraints, constraints_hash, constraints_equal>;

// Using unordered_set to get rid of duplicated elements
constraints_container generate_constraints_variety() {
    static constraints_container constraints_variety = [](){
        using constraints = tbb::task_arena::constraints;
        constraints_container results{};

        std::vector<tbb::numa_node_id> numa_nodes = tbb::info::numa_nodes();
        numa_nodes.push_back((tbb::numa_node_id)tbb::task_arena::automatic);

#if __HYBRID_CPUS_TESTING
        std::vector<tbb::core_type_id> core_types = tbb::info::core_types();
        core_types.push_back((tbb::core_type_id)tbb::task_arena::automatic);
#endif

        results.insert(constraints{});
        for (const auto& numa_node : numa_nodes) {
            results.insert(constraints{}.set_numa_id(numa_node));
#if __HYBRID_CPUS_TESTING
            for (const auto& core_type : core_types) {
                results.insert(constraints{}.set_core_type(core_type));

                results.insert(
                    constraints{}
                        .set_numa_id(numa_node)
                        .set_core_type(core_type)
                );

                for (const auto& max_threads_per_core: system_info::get_available_max_threads_values()) {
                    results.insert(
                        constraints{}
                            .set_max_threads_per_core(max_threads_per_core)
                    );

                    results.insert(
                        constraints{}
                            .set_numa_id(numa_node)
                            .set_max_threads_per_core(max_threads_per_core)
                    );

                    results.insert(
                        constraints{}
                            .set_core_type(core_type)
                            .set_max_threads_per_core(max_threads_per_core)
                    );

                    results.insert(
                        constraints{}
                            .set_numa_id(numa_node)
                            .set_core_type(core_type)
                            .set_max_threads_per_core(max_threads_per_core)
                    );
                }
            }
#endif /*__HYBRID_CPUS_TESTING*/
        }

        // Some constraints may cause unexpected behavior, which would be fixed later.
        if (get_processors_group_count() > 1) {
            for(auto it = results.begin(); it != results.end(); ++it) {
                if (it->max_threads_per_core != tbb::task_arena::automatic
                   && (it->numa_id == tbb::task_arena::automatic || tbb::info::numa_nodes().size() == 1)
#if __HYBRID_CPUS_TESTING
                   && (it->core_type == tbb::task_arena::automatic || tbb::info::core_types().size() == 1)
#endif /*__HYBRID_CPUS_TESTING*/
                ) {
                    it = results.erase(it);
                }
            }
        }
        return results;
    }();

    return constraints_variety;
}
#endif // __TBB_test_common_arena_constraints_H_
