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

#ifndef __TBB_task_emulation_layer_H
#define __TBB_task_emulation_layer_H

#include "tbb/task_group.h"
#include "tbb/task_arena.h"

#include <atomic>

namespace task_emulation {

struct task_group_pool {
    task_group_pool() : pool_size(std::thread::hardware_concurrency()), task_submitters(new tbb::task_group[pool_size]) {}

    ~task_group_pool() {
        for (std::size_t i = 0; i < pool_size; ++i) {
            task_submitters[i].wait();
        }

        delete [] task_submitters;
    }

    tbb::task_group& operator[] (std::size_t idx) { return task_submitters[idx]; }

    const std::size_t pool_size;
    tbb::task_group* task_submitters;
};

static task_group_pool tg_pool;

class base_task {
public:
    base_task() = default;

    base_task(const base_task& t) : m_type(t.m_type), m_parent(t.m_parent), m_child_counter(t.m_child_counter.load())
    {}

    virtual ~base_task() = default;

    void operator() () const {
        task_type type_snapshot = m_type;

        base_task* bypass = const_cast<base_task*>(this)->execute();

        if (m_parent && m_type != task_type::recycled) {
            if (m_parent->remove_child_reference() == 0) {
                m_parent->operator()();
            }
        }

        if (m_type == task_type::allocated) {
            delete this;
        }

        if (bypass != nullptr) {
            m_type = type_snapshot;

            // Bypass is not supported by task_emulation and next_task executed directly.
            // However, the old-TBB bypass behavior can be achieved with
            // `return task_group::defer()` (check Migration Guide).
            // Consider submit another task if recursion call is not acceptable
            // i.e. instead of Direct Body call
            // submit task_emulation::run_task();
            bypass->operator()();
        }
    }

    virtual base_task* execute() = 0;

    template <typename C, typename... Args>
    C* allocate_continuation(std::uint64_t ref, Args&&... args) {
        C* continuation = new C{std::forward<Args>(args)...};
        continuation->m_type = task_type::allocated;
        continuation->reset_parent(reset_parent());
        continuation->m_child_counter = ref;
        return continuation;
    }

    template <typename F, typename... Args>
    F create_child(Args&&... args) {
        return create_child_impl<F>(std::forward<Args>(args)...);
    }

    template <typename F, typename... Args>
    F create_child_and_increment(Args&&... args) {
        add_child_reference();
        return create_child_impl<F>(std::forward<Args>(args)...);
    }

    template <typename F, typename... Args>
    F* allocate_child(Args&&... args) {
        return allocate_child_impl<F>(std::forward<Args>(args)...);
    }

    template <typename F, typename... Args>
    F* allocate_child_and_increment(Args&&... args) {
        add_child_reference();
        return allocate_child_impl<F>(std::forward<Args>(args)...);
    }

    template <typename C>
    void recycle_as_child_of(C& c) {
        m_type = task_type::recycled;
        reset_parent(&c);
    }

    void recycle_as_continuation() {
        m_type = task_type::recycled;
    }

    void add_child_reference() {
        ++m_child_counter;
    }

    std::uint64_t remove_child_reference() {
        return --m_child_counter;
    }

protected:
    enum class task_type {
        stack_based,
        allocated,
        recycled
    };

    mutable task_type m_type;

private:
    template <typename F, typename... Args>
    friend F create_root_task(tbb::task_group& tg, Args&&... args);

    template <typename F, typename... Args>
    friend F* allocate_root_task(tbb::task_group& tg, Args&&... args);

    template <typename F, typename... Args>
    F create_child_impl(Args&&... args) {
        F obj{std::forward<Args>(args)...};
        obj.m_type = task_type::stack_based;
        obj.reset_parent(this);
        return obj;
    }

    template <typename F, typename... Args>
    F* allocate_child_impl(Args&&... args) {
        F* obj = new F{std::forward<Args>(args)...};
        obj->m_type = task_type::allocated;
        obj->reset_parent(this);
        return obj;
    }

    base_task* reset_parent(base_task* ptr = nullptr) {
        auto p = m_parent;
        m_parent = ptr;
        return p;
    }

    base_task* m_parent{nullptr};
    std::atomic<std::uint64_t> m_child_counter{0};
};

class root_task : public base_task {
public:
    root_task(tbb::task_group& tg) : m_tg(tg), m_callback(m_tg.defer([] { /* Create empty callback to preserve reference for wait. */})) {
        add_child_reference();
        m_type = base_task::task_type::allocated;
    }

private:
    base_task* execute() override {
        m_tg.run(std::move(m_callback));
        return nullptr;
    }

    tbb::task_group& m_tg;
    tbb::task_handle m_callback;
};

template <typename F, typename... Args>
F create_root_task(tbb::task_group& tg, Args&&... args) {
    F obj{std::forward<Args>(args)...};
    obj.m_type = base_task::task_type::stack_based;
    obj.reset_parent(new root_task{tg});
    return obj;
}

template <typename F, typename... Args>
F* allocate_root_task(tbb::task_group& tg, Args&&... args) {
    F* obj = new F{std::forward<Args>(args)...};
    obj->m_type = base_task::task_type::allocated;
    obj->reset_parent(new root_task{tg});
    return obj;
}

template <typename F>
void run_task(F&& f) {
    tg_pool[tbb::this_task_arena::current_thread_index()].run(std::forward<F>(f));
}

template <typename F>
void run_task(F* f) {
    tg_pool[tbb::this_task_arena::current_thread_index()].run(std::ref(*f));
}

template <typename F>
void run_and_wait(tbb::task_group& tg, F* f) {
   tg.run_and_wait(std::ref(*f));
}
} // namespace task_emulation

#endif // __TBB_task_emulation_layer_H
