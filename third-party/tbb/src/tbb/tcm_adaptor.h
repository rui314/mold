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

#ifndef _TBB_tcm_adaptor_H
#define _TBB_tcm_adaptor_H

#include "scheduler_common.h"

#include "permit_manager.h"
#include "pm_client.h"

namespace tbb {
namespace detail {
namespace r1 {

struct tcm_adaptor_impl;

//------------------------------------------------------------------------
// Class tcm_adaptor
//------------------------------------------------------------------------

class tcm_adaptor : public permit_manager {
public:
    tcm_adaptor();
    ~tcm_adaptor();

    pm_client* create_client(arena& a) override;
    void register_client(pm_client* client, d1::constraints& constraints) override;
    void unregister_and_destroy_client(pm_client& c) override;

    void set_active_num_workers(int soft_limit) override;

    void adjust_demand(pm_client& c, int mandatory_delta, int workers_delta)  override;

    bool is_connected();

    static void initialize();
    static bool is_initialized();
    static void print_version();
private:
    cache_aligned_unique_ptr<tcm_adaptor_impl> my_impl;

    friend class tcm_client;
}; // class tcm_adaptor

} // namespace r1
} // namespace detail
} // namespace tbb

#endif /* _TBB_tcm_adaptor_H */
