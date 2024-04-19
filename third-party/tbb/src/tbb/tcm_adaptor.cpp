/*
    Copyright (c) 2023 Intel Corporation

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

#include "oneapi/tbb/detail/_intrusive_list_node.h"
#include "oneapi/tbb/detail/_template_helpers.h"
#include "oneapi/tbb/task_arena.h"

#include "pm_client.h"
#include "dynamic_link.h"
#include "misc.h"
#include "tcm.h"
#include "tcm_adaptor.h"

#include <iostream>

namespace tbb {
namespace detail {
namespace r1 {

namespace {
#if __TBB_WEAK_SYMBOLS_PRESENT
#pragma weak tcmConnect
#pragma weak tcmDisconnect
#pragma weak tcmRequestPermit
#pragma weak tcmGetPermitData
#pragma weak tcmReleasePermit
#pragma weak tcmIdlePermit
#pragma weak tcmDeactivatePermit
#pragma weak tcmActivatePermit
#pragma weak tcmRegisterThread
#pragma weak tcmUnregisterThread
#pragma weak tcmGetVersionInfo
#endif /* __TBB_WEAK_SYMBOLS_PRESENT */

tcm_result_t(*tcm_connect)(tcm_callback_t callback, tcm_client_id_t* client_id){nullptr};
tcm_result_t(*tcm_disconnect)(tcm_client_id_t client_id){ nullptr };
tcm_result_t(*tcm_request_permit)(tcm_client_id_t client_id, tcm_permit_request_t request,
    void* callback_arg, tcm_permit_handle_t* permit_handle, tcm_permit_t* permit){nullptr};
tcm_result_t(*tcm_get_permit_data)(tcm_permit_handle_t permit_handle, tcm_permit_t* permit){nullptr};
tcm_result_t(*tcm_release_permit)(tcm_permit_handle_t permit){nullptr};
tcm_result_t(*tcm_idle_permit)(tcm_permit_handle_t permit_handle){nullptr};
tcm_result_t(*tcm_deactivate_permit)(tcm_permit_handle_t permit_handle){nullptr};
tcm_result_t(*tcm_activate_permit)(tcm_permit_handle_t permit_handle){nullptr};
tcm_result_t(*tcm_register_thread)(tcm_permit_handle_t permit_handle){nullptr};
tcm_result_t(*tcm_unregister_thread)(){nullptr};
tcm_result_t (*tcm_get_version_info)(char* buffer, uint32_t buffer_size){nullptr};

static const dynamic_link_descriptor tcm_link_table[] = {
    DLD(tcmConnect, tcm_connect),
    DLD(tcmDisconnect, tcm_disconnect),
    DLD(tcmRequestPermit, tcm_request_permit),
    DLD(tcmGetPermitData, tcm_get_permit_data),
    DLD(tcmReleasePermit, tcm_release_permit),
    DLD(tcmIdlePermit, tcm_idle_permit),
    DLD(tcmDeactivatePermit, tcm_deactivate_permit),
    DLD(tcmActivatePermit, tcm_activate_permit),
    DLD(tcmRegisterThread, tcm_register_thread),
    DLD(tcmUnregisterThread, tcm_unregister_thread),
    DLD(tcmGetVersionInfo, tcm_get_version_info)
};

#if TBB_USE_DEBUG
#define DEBUG_SUFFIX "_debug"
#else
#define DEBUG_SUFFIX
#endif /* TBB_USE_DEBUG */

#if _WIN32 || _WIN64
#define LIBRARY_EXTENSION ".dll"
#define LIBRARY_PREFIX
#elif __unix__
#define LIBRARY_EXTENSION ".so.1"
#define LIBRARY_PREFIX "lib"
#else
#define LIBRARY_EXTENSION
#define LIBRARY_PREFIX
#endif /* __unix__ */

#define TCMLIB_NAME LIBRARY_PREFIX "tcm" DEBUG_SUFFIX LIBRARY_EXTENSION

static bool tcm_functions_loaded{ false };
}

class tcm_client : public pm_client {
    using tcm_client_mutex_type = d1::mutex;
public:
    tcm_client(tcm_adaptor& adaptor, arena& a) : pm_client(a), my_tcm_adaptor(adaptor) {}

    ~tcm_client() {
        if (my_permit_handle) {
            __TBB_ASSERT(tcm_release_permit, nullptr);
            auto res = tcm_release_permit(my_permit_handle);
            __TBB_ASSERT_EX(res == TCM_RESULT_SUCCESS, nullptr);
        }
    }

    int update_concurrency(uint32_t concurrency) {
        return my_arena.update_concurrency(concurrency);
    }

    unsigned priority_level() {
        return my_arena.priority_level();
    }

    tcm_permit_request_t& permit_request() {
        return my_permit_request;
    }

    tcm_permit_handle_t& permit_handle() {
        return my_permit_handle;
    }

    void actualize_permit() {
        __TBB_ASSERT(tcm_get_permit_data, nullptr);
        int delta{};
        {
            tcm_client_mutex_type::scoped_lock lock(my_permit_mutex);

            uint32_t new_concurrency{};
            tcm_permit_t new_permit{ &new_concurrency, nullptr, 1, TCM_PERMIT_STATE_VOID, {} };
            auto res = tcm_get_permit_data(my_permit_handle, &new_permit);
            __TBB_ASSERT_EX(res == TCM_RESULT_SUCCESS, nullptr);

            // The permit has changed during the reading, so the callback will be invoked soon one more time and
            // we can just skip this renegotiation iteration.
            if (!new_permit.flags.stale) {
                __TBB_ASSERT(
                    new_permit.state != TCM_PERMIT_STATE_INACTIVE || new_concurrency == 0,
                    "TCM did not nullify resources while deactivating the permit"
                );
                delta = update_concurrency(new_concurrency);
            }
        }
        if (delta) {
            my_tcm_adaptor.notify_thread_request(delta);
        }
    }

    void request_permit(tcm_client_id_t client_id) {
        __TBB_ASSERT(tcm_request_permit, nullptr);

        my_permit_request.max_sw_threads = max_workers();
        my_permit_request.min_sw_threads = my_permit_request.max_sw_threads == 0 ? 0 : min_workers();

        if (my_permit_request.constraints_size > 0) {
            my_permit_request.cpu_constraints->min_concurrency = my_permit_request.min_sw_threads;
            my_permit_request.cpu_constraints->max_concurrency = my_permit_request.max_sw_threads;
        }

        __TBB_ASSERT(my_permit_request.max_sw_threads >= my_permit_request.min_sw_threads, nullptr);

        tcm_result_t res = tcm_request_permit(client_id, my_permit_request, this, &my_permit_handle, nullptr);
        __TBB_ASSERT_EX(res == TCM_RESULT_SUCCESS, nullptr);
    }

    void deactivate_permit() {
         __TBB_ASSERT(tcm_deactivate_permit, nullptr);
        tcm_result_t res = tcm_deactivate_permit(my_permit_handle);
        __TBB_ASSERT_EX(res == TCM_RESULT_SUCCESS, nullptr);
    }

    void init(d1::constraints& constraints) {
        __TBB_ASSERT(tcm_request_permit, nullptr);
        __TBB_ASSERT(tcm_deactivate_permit, nullptr);

        if (constraints.core_type            != d1::task_arena::automatic ||
            constraints.numa_id              != d1::task_arena::automatic ||
            constraints.max_threads_per_core != d1::task_arena::automatic)
        {
            my_permit_constraints.max_concurrency = constraints.max_concurrency;
            my_permit_constraints.min_concurrency = 0;
            my_permit_constraints.core_type_id = constraints.core_type;
            my_permit_constraints.numa_id = constraints.numa_id;
            my_permit_constraints.threads_per_core = constraints.max_threads_per_core;

            my_permit_request.cpu_constraints = &my_permit_constraints;
            my_permit_request.constraints_size = 1;
        }

        my_permit_request.min_sw_threads = 0;
        my_permit_request.max_sw_threads = 0;
    }

    void register_thread() override {
        __TBB_ASSERT(tcm_register_thread, nullptr);
        auto return_code = tcm_register_thread(my_permit_handle);
        __TBB_ASSERT_EX(return_code == TCM_RESULT_SUCCESS, nullptr);
    }

    void unregister_thread() override {
        __TBB_ASSERT(tcm_unregister_thread, nullptr);
        auto return_code = tcm_unregister_thread();
        __TBB_ASSERT_EX(return_code == TCM_RESULT_SUCCESS, nullptr);
    }

private:
    tcm_cpu_constraints_t my_permit_constraints = TCM_PERMIT_REQUEST_CONSTRAINTS_INITIALIZER;
    tcm_permit_request_t my_permit_request = TCM_PERMIT_REQUEST_INITIALIZER;
    tcm_permit_handle_t my_permit_handle{};
    tcm_client_mutex_type my_permit_mutex;
    tcm_adaptor& my_tcm_adaptor;
};

//------------------------------------------------------------------------
// tcm_adaptor_impl
//------------------------------------------------------------------------

struct tcm_adaptor_impl {
    using demand_mutex_type = d1::mutex;
    demand_mutex_type my_demand_mutex;
    tcm_client_id_t client_id{};

    tcm_adaptor_impl(tcm_client_id_t id) : client_id(id)
    {}
};

//------------------------------------------------------------------------
// tcm_adaptor
//------------------------------------------------------------------------

tcm_result_t renegotiation_callback(tcm_permit_handle_t, void* client_ptr, tcm_callback_flags_t) {
    __TBB_ASSERT(client_ptr, nullptr);
    static_cast<tcm_client*>(client_ptr)->actualize_permit();
    return TCM_RESULT_SUCCESS;
}

void tcm_adaptor::initialize() {
    tcm_functions_loaded = dynamic_link(TCMLIB_NAME, tcm_link_table, /* tcm_link_table size = */ 11);
}

bool tcm_adaptor::is_initialized() {
    return tcm_functions_loaded;
}

void tcm_adaptor::print_version() {
    if (is_initialized()) {
        __TBB_ASSERT(tcm_get_version_info, nullptr);
        char buffer[1024];
        tcm_get_version_info(buffer, 1024);
        std::fprintf(stderr, "%.*s", 1024, buffer);
    }
}

tcm_adaptor::tcm_adaptor() {
    __TBB_ASSERT(tcm_connect, nullptr);
    tcm_client_id_t client_id{};
    auto return_code = tcm_connect(renegotiation_callback, &client_id);
    if (return_code == TCM_RESULT_SUCCESS) {
        my_impl = make_cache_aligned_unique<tcm_adaptor_impl>(client_id);
    }
}

tcm_adaptor::~tcm_adaptor() {
    if (my_impl) {
        __TBB_ASSERT(tcm_disconnect, nullptr);
        auto return_code = tcm_disconnect(my_impl->client_id);
        __TBB_ASSERT_EX(return_code == TCM_RESULT_SUCCESS, nullptr);
        my_impl = nullptr;
    }
}

bool tcm_adaptor::is_connected() {
    return my_impl != nullptr;
}

pm_client* tcm_adaptor::create_client(arena& a) {
    return new (cache_aligned_allocate(sizeof(tcm_client))) tcm_client(*this, a);
}

void tcm_adaptor::register_client(pm_client* c, d1::constraints& constraints) {
    static_cast<tcm_client*>(c)->init(constraints);
}

void tcm_adaptor::unregister_and_destroy_client(pm_client& c) {
    auto& client = static_cast<tcm_client&>(c);

    {
        tcm_adaptor_impl::demand_mutex_type::scoped_lock lock(my_impl->my_demand_mutex);
        client.~tcm_client();
    }
    cache_aligned_deallocate(&client);
}

void tcm_adaptor::set_active_num_workers(int) {}


void tcm_adaptor::adjust_demand(pm_client& c, int mandatory_delta, int workers_delta) {
    __TBB_ASSERT(-1 <= mandatory_delta && mandatory_delta <= 1, nullptr);

    auto& client = static_cast<tcm_client&>(c);
    {
        tcm_adaptor_impl::demand_mutex_type::scoped_lock lock(my_impl->my_demand_mutex);

        // Update client's state
        workers_delta = client.update_request(mandatory_delta, workers_delta);
        if (workers_delta == 0) return;

        if (client.max_workers() == 0) {
            client.deactivate_permit();
        } else {
            client.request_permit(my_impl->client_id);
        }
    }

    client.actualize_permit();
}

} // namespace r1
} // namespace detail
} // namespace tbb
