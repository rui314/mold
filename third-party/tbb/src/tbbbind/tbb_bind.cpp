/*
    Copyright (c) 2019-2021 Intel Corporation

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

#include <vector>
#include <mutex>

#include "../tbb/assert_impl.h" // Out-of-line TBB assertion handling routines are instantiated here.
#include "oneapi/tbb/detail/_assert.h"
#include "oneapi/tbb/detail/_config.h"

#if _MSC_VER && !__INTEL_COMPILER && !__clang__
#pragma warning( push )
#pragma warning( disable : 4100 )
#elif _MSC_VER && __clang__
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wunused-parameter"
#endif
#include <hwloc.h>
#if _MSC_VER && !__INTEL_COMPILER && !__clang__
#pragma warning( pop )
#elif _MSC_VER && __clang__
#pragma GCC diagnostic pop
#endif

#define __TBBBIND_HWLOC_HYBRID_CPUS_INTERFACES_PRESENT (HWLOC_API_VERSION >= 0x20400)
#define __TBBBIND_HWLOC_TOPOLOGY_FLAG_RESTRICT_TO_CPUBINDING_PRESENT (HWLOC_API_VERSION >= 0x20500)

// Most of hwloc calls returns negative exit code on error.
// This macro tracks error codes that are returned from the hwloc interfaces.
#define assertion_hwloc_wrapper(command, ...) \
        __TBB_ASSERT_EX( (command(__VA_ARGS__)) >= 0, "Error occurred during call to hwloc API.");

namespace tbb {
namespace detail {
namespace r1 {

//------------------------------------------------------------------------
// Information about the machine's hardware TBB is happen to work on
//------------------------------------------------------------------------
class system_topology {
    friend class binding_handler;

    // Common topology members
    hwloc_topology_t topology{nullptr};
    hwloc_cpuset_t   process_cpu_affinity_mask{nullptr};
    hwloc_nodeset_t  process_node_affinity_mask{nullptr};
    std::size_t number_of_processors_groups{1};

    // NUMA API related topology members
    std::vector<hwloc_cpuset_t> numa_affinity_masks_list{};
    std::vector<int> numa_indexes_list{};
    int numa_nodes_count{0};

    // Hybrid CPUs API related topology members
    std::vector<hwloc_cpuset_t> core_types_affinity_masks_list{};
    std::vector<int> core_types_indexes_list{};

    enum init_stages { uninitialized,
                       started,
                       topology_allocated,
                       topology_loaded,
                       topology_parsed } initialization_state;

    // Binding threads that locate in another Windows Processor groups
    // is allowed only if machine topology contains several Windows Processors groups
    // and process affinity mask wasn`t limited manually (affinity mask cannot violates
    // processors group boundaries).
    bool intergroup_binding_allowed(std::size_t groups_num) { return groups_num > 1; }

private:
    void topology_initialization(std::size_t groups_num) {
        initialization_state = started;

        // Parse topology
        if ( hwloc_topology_init( &topology ) == 0 ) {
            initialization_state = topology_allocated;
#if __TBBBIND_HWLOC_TOPOLOGY_FLAG_RESTRICT_TO_CPUBINDING_PRESENT
            if ( groups_num == 1 &&
                 hwloc_topology_set_flags(topology,
                     HWLOC_TOPOLOGY_FLAG_IS_THISSYSTEM |
                     HWLOC_TOPOLOGY_FLAG_RESTRICT_TO_CPUBINDING
                 ) != 0
            ) {
                return;
            }
#endif
            if ( hwloc_topology_load( topology ) == 0 ) {
                initialization_state = topology_loaded;
            }
        }
        if ( initialization_state != topology_loaded )
            return;

        // Getting process affinity mask
        if ( intergroup_binding_allowed(groups_num) ) {
            process_cpu_affinity_mask  = hwloc_bitmap_dup(hwloc_topology_get_complete_cpuset (topology));
            process_node_affinity_mask = hwloc_bitmap_dup(hwloc_topology_get_complete_nodeset(topology));
        } else {
            process_cpu_affinity_mask  = hwloc_bitmap_alloc();
            process_node_affinity_mask = hwloc_bitmap_alloc();

            assertion_hwloc_wrapper(hwloc_get_cpubind, topology, process_cpu_affinity_mask, 0);
            hwloc_cpuset_to_nodeset(topology, process_cpu_affinity_mask, process_node_affinity_mask);
        }

        number_of_processors_groups = groups_num;
    }

    void numa_topology_parsing() {
        // Fill parameters with stubs if topology parsing is broken.
        if ( initialization_state != topology_loaded ) {
            numa_nodes_count = 1;
            numa_indexes_list.push_back(-1);
            return;
        }

        // If system contains no NUMA nodes, HWLOC 1.11 returns an infinitely filled bitmap.
        // hwloc_bitmap_weight() returns negative value for such bitmaps, so we use this check
        // to change way of topology initialization.
        numa_nodes_count = hwloc_bitmap_weight(process_node_affinity_mask);
        if (numa_nodes_count <= 0) {
            // numa_nodes_count may be empty if the process affinity mask is empty too (invalid case)
            // or if some internal HWLOC error occurred.
            // So we place -1 as index in this case.
            numa_indexes_list.push_back(numa_nodes_count == 0 ? -1 : 0);
            numa_nodes_count = 1;

            numa_affinity_masks_list.push_back(hwloc_bitmap_dup(process_cpu_affinity_mask));
        } else {
            // Get NUMA logical indexes list
            unsigned counter = 0;
            int i = 0;
            int max_numa_index = -1;
            numa_indexes_list.resize(numa_nodes_count);
            hwloc_obj_t node_buffer;
            hwloc_bitmap_foreach_begin(i, process_node_affinity_mask) {
                node_buffer = hwloc_get_numanode_obj_by_os_index(topology, i);
                numa_indexes_list[counter] = static_cast<int>(node_buffer->logical_index);

                if ( numa_indexes_list[counter] > max_numa_index ) {
                    max_numa_index = numa_indexes_list[counter];
                }

                counter++;
            } hwloc_bitmap_foreach_end();
            __TBB_ASSERT(max_numa_index >= 0, "Maximal NUMA index must not be negative");

            // Fill concurrency and affinity masks lists
            numa_affinity_masks_list.resize(max_numa_index + 1);
            int index = 0;
            hwloc_bitmap_foreach_begin(i, process_node_affinity_mask) {
                node_buffer = hwloc_get_numanode_obj_by_os_index(topology, i);
                index = static_cast<int>(node_buffer->logical_index);

                hwloc_cpuset_t& current_mask = numa_affinity_masks_list[index];
                current_mask = hwloc_bitmap_dup(node_buffer->cpuset);

                hwloc_bitmap_and(current_mask, current_mask, process_cpu_affinity_mask);
                __TBB_ASSERT(!hwloc_bitmap_iszero(current_mask), "hwloc detected unavailable NUMA node");
            } hwloc_bitmap_foreach_end();
        }
    }

    void core_types_topology_parsing() {
        // Fill parameters with stubs if topology parsing is broken.
        if ( initialization_state != topology_loaded ) {
            core_types_indexes_list.push_back(-1);
            return;
        }
#if __TBBBIND_HWLOC_HYBRID_CPUS_INTERFACES_PRESENT
        __TBB_ASSERT(hwloc_get_api_version() >= 0x20400, "Hybrid CPUs support interfaces required HWLOC >= 2.4");
        // Parsing the hybrid CPU topology
        int core_types_number = hwloc_cpukinds_get_nr(topology, 0);
        bool core_types_parsing_broken = core_types_number <= 0;
        if (!core_types_parsing_broken) {
            core_types_affinity_masks_list.resize(core_types_number);
            int efficiency{-1};

            for (int core_type = 0; core_type < core_types_number; ++core_type) {
                hwloc_cpuset_t& current_mask = core_types_affinity_masks_list[core_type];
                current_mask = hwloc_bitmap_alloc();

                if (!hwloc_cpukinds_get_info(topology, core_type, current_mask, &efficiency, nullptr, nullptr, 0)
                    && efficiency >= 0
                ) {
                    hwloc_bitmap_and(current_mask, current_mask, process_cpu_affinity_mask);

                    if (hwloc_bitmap_weight(current_mask) > 0) {
                        core_types_indexes_list.push_back(core_type);
                    }
                    __TBB_ASSERT(hwloc_bitmap_weight(current_mask) >= 0, "Infinivitely filled core type mask");
                } else {
                    core_types_parsing_broken = true;
                    break;
                }
            }
        }
#else /*!__TBBBIND_HWLOC_HYBRID_CPUS_INTERFACES_PRESENT*/
        bool core_types_parsing_broken{true};
#endif /*__TBBBIND_HWLOC_HYBRID_CPUS_INTERFACES_PRESENT*/

        if (core_types_parsing_broken) {
            for (auto& core_type_mask : core_types_affinity_masks_list) {
                hwloc_bitmap_free(core_type_mask);
            }
            core_types_affinity_masks_list.resize(1);
            core_types_indexes_list.resize(1);

            core_types_affinity_masks_list[0] = hwloc_bitmap_dup(process_cpu_affinity_mask);
            core_types_indexes_list[0] = -1;
        }
    }

    void enforce_hwloc_2_5_runtime_linkage() {
        // Without the call of this function HWLOC 2.4 can be successfully loaded during the tbbbind_2_5 loading.
        // It is possible since tbbbind_2_5 don't use any new entry points that were introduced in HWLOC 2.5
        // But tbbbind_2_5 compiles with HWLOC 2.5 header, therefore such situation requires binary forward compatibility
        // which are not guaranteed by the HWLOC library. To enforce linkage tbbbind_2_5 only with HWLOC >= 2.5 version
        // this function calls the interface that is available in the HWLOC 2.5 only.
#if HWLOC_API_VERSION >= 0x20500
        auto some_core = hwloc_get_next_obj_by_type(topology, HWLOC_OBJ_CORE, nullptr);
        hwloc_get_obj_with_same_locality(topology, some_core, HWLOC_OBJ_CORE, nullptr, nullptr, 0);
#endif
    }

  
    void initialize( std::size_t groups_num ) {
        if ( initialization_state != uninitialized )
            return;

        topology_initialization(groups_num);
        numa_topology_parsing();
        core_types_topology_parsing();

        enforce_hwloc_2_5_runtime_linkage();

        if (initialization_state == topology_loaded)
            initialization_state = topology_parsed;
    }

    static system_topology* instance_ptr;
public:
    typedef hwloc_cpuset_t             affinity_mask;
    typedef hwloc_const_cpuset_t const_affinity_mask;

    bool is_topology_parsed() { return initialization_state == topology_parsed; }

    static void construct( std::size_t groups_num ) {
        if (instance_ptr == nullptr) {
            instance_ptr = new system_topology();
            instance_ptr->initialize(groups_num);
        }
    }

    static system_topology& instance() {
        __TBB_ASSERT(instance_ptr != nullptr, "Getting instance of non-constructed topology");
        return *instance_ptr;
    }

    static void destroy() {
        __TBB_ASSERT(instance_ptr != nullptr, "Destroying non-constructed topology");
        delete instance_ptr;
    }

    ~system_topology() {
        if ( is_topology_parsed() ) {
            for (auto& numa_node_mask : numa_affinity_masks_list) {
                hwloc_bitmap_free(numa_node_mask);
            }

            for (auto& core_type_mask : core_types_affinity_masks_list) {
                hwloc_bitmap_free(core_type_mask);
            }

            hwloc_bitmap_free(process_node_affinity_mask);
            hwloc_bitmap_free(process_cpu_affinity_mask);
        }

        if ( initialization_state >= topology_allocated ) {
            hwloc_topology_destroy(topology);
        }

        initialization_state = uninitialized;
    }

    void fill_topology_information(
        int& _numa_nodes_count, int*& _numa_indexes_list,
        int& _core_types_count, int*& _core_types_indexes_list
    ) {
        __TBB_ASSERT(is_topology_parsed(), "Trying to get access to uninitialized system_topology");
        _numa_nodes_count = numa_nodes_count;
        _numa_indexes_list = numa_indexes_list.data();

        _core_types_count = (int)core_types_indexes_list.size();
        _core_types_indexes_list = core_types_indexes_list.data();
    }

    void fill_constraints_affinity_mask(affinity_mask input_mask, int numa_node_index, int core_type_index, int max_threads_per_core) {
        __TBB_ASSERT(is_topology_parsed(), "Trying to get access to uninitialized system_topology");
        __TBB_ASSERT(numa_node_index < (int)numa_affinity_masks_list.size(), "Wrong NUMA node id");
        __TBB_ASSERT(core_type_index < (int)core_types_affinity_masks_list.size(), "Wrong core type id");
        __TBB_ASSERT(max_threads_per_core == -1 || max_threads_per_core > 0, "Wrong max_threads_per_core");

        hwloc_cpuset_t constraints_mask = hwloc_bitmap_alloc();
        hwloc_cpuset_t core_mask = hwloc_bitmap_alloc();

        hwloc_bitmap_copy(constraints_mask, process_cpu_affinity_mask);
        if (numa_node_index >= 0) {
            hwloc_bitmap_and(constraints_mask, constraints_mask, numa_affinity_masks_list[numa_node_index]);
        }
        if (core_type_index >= 0) {
            hwloc_bitmap_and(constraints_mask, constraints_mask, core_types_affinity_masks_list[core_type_index]);
        }
        if (max_threads_per_core > 0) {
            // clear input mask
            hwloc_bitmap_zero(input_mask);

            hwloc_obj_t current_core = nullptr;
            while ((current_core = hwloc_get_next_obj_by_type(topology, HWLOC_OBJ_CORE, current_core)) != nullptr) {
                hwloc_bitmap_and(core_mask, constraints_mask, current_core->cpuset);

                // fit the core mask to required bits number
                int current_threads_per_core = 0;
                for (int id = hwloc_bitmap_first(core_mask); id != -1; id = hwloc_bitmap_next(core_mask, id)) {
                    if (++current_threads_per_core > max_threads_per_core) {
                        hwloc_bitmap_clr(core_mask, id);
                    }
                }

                hwloc_bitmap_or(input_mask, input_mask, core_mask);
            }
        } else {
            hwloc_bitmap_copy(input_mask, constraints_mask);
        }

        hwloc_bitmap_free(core_mask);
        hwloc_bitmap_free(constraints_mask);
    }

    void fit_num_threads_per_core(affinity_mask result_mask, affinity_mask current_mask, affinity_mask constraints_mask) {
        hwloc_bitmap_zero(result_mask);
        hwloc_obj_t current_core = nullptr;
        while ((current_core = hwloc_get_next_obj_by_type(topology, HWLOC_OBJ_CORE, current_core)) != nullptr) {
            if (hwloc_bitmap_intersects(current_mask, current_core->cpuset)) {
                hwloc_bitmap_or(result_mask, result_mask, current_core->cpuset);
            }
        }
        hwloc_bitmap_and(result_mask, result_mask, constraints_mask);
    }

    int get_default_concurrency(int numa_node_index, int core_type_index, int max_threads_per_core) {
        __TBB_ASSERT(is_topology_parsed(), "Trying to get access to uninitialized system_topology");

        hwloc_cpuset_t constraints_mask = hwloc_bitmap_alloc();
        fill_constraints_affinity_mask(constraints_mask, numa_node_index, core_type_index, max_threads_per_core);

        int default_concurrency = hwloc_bitmap_weight(constraints_mask);
        hwloc_bitmap_free(constraints_mask);
        return default_concurrency;
    }

    affinity_mask allocate_process_affinity_mask() {
        __TBB_ASSERT(is_topology_parsed(), "Trying to get access to uninitialized system_topology");
        return hwloc_bitmap_dup(process_cpu_affinity_mask);
    }

    void free_affinity_mask( affinity_mask mask_to_free ) {
        hwloc_bitmap_free(mask_to_free); // If bitmap is nullptr, no operation is performed.
    }

    void store_current_affinity_mask( affinity_mask current_mask ) {
        assertion_hwloc_wrapper(hwloc_get_cpubind, topology, current_mask, HWLOC_CPUBIND_THREAD);

        hwloc_bitmap_and(current_mask, current_mask, process_cpu_affinity_mask);
        __TBB_ASSERT(!hwloc_bitmap_iszero(current_mask),
            "Current affinity mask must intersects with process affinity mask");
    }

    void set_affinity_mask( const_affinity_mask mask ) {
        if (hwloc_bitmap_weight(mask) > 0) {
            assertion_hwloc_wrapper(hwloc_set_cpubind, topology, mask, HWLOC_CPUBIND_THREAD);
        }
    }
};

system_topology* system_topology::instance_ptr{nullptr};

class binding_handler {
    // Following vector saves thread affinity mask on scheduler entry to return it to this thread 
    // on scheduler exit.
    typedef std::vector<system_topology::affinity_mask> affinity_masks_container;
    affinity_masks_container affinity_backup;
    system_topology::affinity_mask handler_affinity_mask;

#ifdef _WIN32
    affinity_masks_container affinity_buffer;
    int my_numa_node_id;
    int my_core_type_id;
    int my_max_threads_per_core;
#endif

public:
    binding_handler( std::size_t size, int numa_node_id, int core_type_id, int max_threads_per_core )
        : affinity_backup(size)
#ifdef _WIN32
        , affinity_buffer(size)
        , my_numa_node_id(numa_node_id)
        , my_core_type_id(core_type_id)
        , my_max_threads_per_core(max_threads_per_core)
#endif
    {
        for (std::size_t i = 0; i < size; ++i) {
            affinity_backup[i] = system_topology::instance().allocate_process_affinity_mask();
#ifdef _WIN32
            affinity_buffer[i] = system_topology::instance().allocate_process_affinity_mask();
#endif
        }
        handler_affinity_mask = system_topology::instance().allocate_process_affinity_mask();
        system_topology::instance().fill_constraints_affinity_mask
            (handler_affinity_mask, numa_node_id, core_type_id, max_threads_per_core);
    }

    ~binding_handler() {
        for (std::size_t i = 0; i < affinity_backup.size(); ++i) {
            system_topology::instance().free_affinity_mask(affinity_backup[i]);
#ifdef _WIN32
            system_topology::instance().free_affinity_mask(affinity_buffer[i]);
#endif
        }
        system_topology::instance().free_affinity_mask(handler_affinity_mask);
    }

    void apply_affinity( unsigned slot_num ) {
        auto& topology = system_topology::instance();
        __TBB_ASSERT(slot_num < affinity_backup.size(),
            "The slot number is greater than the number of slots in the arena");
        __TBB_ASSERT(topology.is_topology_parsed(),
            "Trying to get access to uninitialized system_topology");

        topology.store_current_affinity_mask(affinity_backup[slot_num]);

#ifdef _WIN32
        // TBBBind supports only systems where NUMA nodes and core types do not cross the border
        // between several processor groups. So if a certain NUMA node or core type constraint
        // specified, then the constraints affinity mask will not cross the processor groups' border.

        // But if we have constraint based only on the max_threads_per_core setting, then the
        // constraints affinity mask does may cross the border between several processor groups
        // on machines with more then 64 hardware threads. That is why we need to use the special
        // function, which regulates the number of threads in the current threads mask.
        if (topology.number_of_processors_groups > 1 && my_max_threads_per_core != -1 &&
            (my_numa_node_id == -1 || topology.numa_indexes_list.size() == 1) &&
            (my_core_type_id == -1 || topology.core_types_indexes_list.size() == 1)
        ) {
            topology.fit_num_threads_per_core(affinity_buffer[slot_num], affinity_backup[slot_num], handler_affinity_mask);
            topology.set_affinity_mask(affinity_buffer[slot_num]);
            return;
        }
#endif
        topology.set_affinity_mask(handler_affinity_mask);
    }

    void restore_previous_affinity_mask( unsigned slot_num ) {
        auto& topology = system_topology::instance();
        __TBB_ASSERT(topology.is_topology_parsed(),
            "Trying to get access to uninitialized system_topology");
        topology.set_affinity_mask(affinity_backup[slot_num]);
    };

};

extern "C" { // exported to TBB interfaces

TBBBIND_EXPORT void __TBB_internal_initialize_system_topology(
    std::size_t groups_num,
    int& numa_nodes_count, int*& numa_indexes_list,
    int& core_types_count, int*& core_types_indexes_list
) {
    system_topology::construct(groups_num);
    system_topology::instance().fill_topology_information(
        numa_nodes_count, numa_indexes_list,
        core_types_count, core_types_indexes_list
    );
}

TBBBIND_EXPORT binding_handler* __TBB_internal_allocate_binding_handler(int number_of_slots, int numa_id, int core_type_id, int max_threads_per_core) {
    __TBB_ASSERT(number_of_slots > 0, "Trying to create numa handler for 0 threads.");
    return new binding_handler(number_of_slots, numa_id, core_type_id, max_threads_per_core);
}

TBBBIND_EXPORT void __TBB_internal_deallocate_binding_handler(binding_handler* handler_ptr) {
    __TBB_ASSERT(handler_ptr != nullptr, "Trying to deallocate nullptr pointer.");
    delete handler_ptr;
}

TBBBIND_EXPORT void __TBB_internal_apply_affinity(binding_handler* handler_ptr, int slot_num) {
    __TBB_ASSERT(handler_ptr != nullptr, "Trying to get access to uninitialized metadata.");
    handler_ptr->apply_affinity(slot_num);
}

TBBBIND_EXPORT void __TBB_internal_restore_affinity(binding_handler* handler_ptr, int slot_num) {
    __TBB_ASSERT(handler_ptr != nullptr, "Trying to get access to uninitialized metadata.");
    handler_ptr->restore_previous_affinity_mask(slot_num);
}

TBBBIND_EXPORT int __TBB_internal_get_default_concurrency(int numa_id, int core_type_id, int max_threads_per_core) {
    return system_topology::instance().get_default_concurrency(numa_id, core_type_id, max_threads_per_core);
}

void __TBB_internal_destroy_system_topology() {
    return system_topology::destroy();
}

} // extern "C"

} // namespace r1
} // namespace detail
} // namespace tbb

#undef assertion_hwloc_wrapper
