%pythonbegin %{
#
# Copyright (c) 2016-2021 Intel Corporation
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.


__all__ = ["task_arena",
           "task_group",
           "global_control",
           "default_num_threads",
           "this_task_arena_max_concurrency",
           "this_task_arena_current_thread_index",
           "runtime_version",
           "runtime_interface_version"]
%}
%begin %{
/* Defines Python wrappers for Intel(R) oneAPI Threading Building Blocks (oneTBB) */
%}
%module api

#if SWIG_VERSION < 0x030001
#error SWIG version 3.0.6 or newer is required for correct functioning
#endif

%{
#define TBB_PREVIEW_WAITING_FOR_WORKERS 1
#include "tbb/task_arena.h"
#include "tbb/task_group.h"
#include "tbb/global_control.h"
#include "tbb/version.h"

#include <condition_variable>
#include <mutex>
#include <memory>

using namespace tbb;

class PyCaller : public swig::SwigPtr_PyObject {
public:
    // icpc 2013 does not support simple using SwigPtr_PyObject::SwigPtr_PyObject;
    PyCaller(const PyCaller& s) : SwigPtr_PyObject(s) {}
    PyCaller(PyObject *p, bool initial = true) : SwigPtr_PyObject(p, initial) {}

    void operator()() const {
        SWIG_PYTHON_THREAD_BEGIN_BLOCK;
        PyObject* r = PyObject_CallFunctionObjArgs((PyObject*)*this, nullptr);
        if(r) Py_DECREF(r);
        SWIG_PYTHON_THREAD_END_BLOCK;
    }
};

struct ArenaPyCaller {
    task_arena *my_arena;
    PyObject *my_callable;
    ArenaPyCaller(task_arena *a, PyObject *c) : my_arena(a), my_callable(c) {
        SWIG_PYTHON_THREAD_BEGIN_BLOCK;
        Py_XINCREF(c);
        SWIG_PYTHON_THREAD_END_BLOCK;
    }
    void operator()() const {
        my_arena->execute(PyCaller(my_callable, false));
    }
};

struct barrier_data {
    std::condition_variable event;
    std::mutex m;
    int worker_threads, full_threads;
};

void _concurrency_barrier(int threads = tbb::task_arena::automatic) {
    if(threads == tbb::task_arena::automatic)
        threads = tbb::this_task_arena::max_concurrency();
    if(threads < 2)
        return;
    std::unique_ptr<global_control> g(
        (global_control::active_value(global_control::max_allowed_parallelism) < unsigned(threads))?
            new global_control(global_control::max_allowed_parallelism, threads) : nullptr);

    tbb::task_group tg;
    barrier_data b;
    b.worker_threads = 0;
    b.full_threads = threads-1;
    for(int i = 0; i < b.full_threads; i++)
        tg.run([&b]{
            std::unique_lock<std::mutex> lock(b.m);
            if(++b.worker_threads >= b.full_threads)
                b.event.notify_all();
            else while(b.worker_threads < b.full_threads)
                b.event.wait(lock);
        });
    std::unique_lock<std::mutex> lock(b.m);
    b.event.wait(lock);
    tg.wait();
};

%}

void _concurrency_barrier(int threads = tbb::task_arena::automatic);

namespace tbb {

    class task_arena {
    public:
        static const int automatic = -1;
        task_arena(int max_concurrency = automatic, unsigned reserved_for_masters = 1);
        task_arena(const task_arena &s);
        ~task_arena();
        void initialize();
        void initialize(int max_concurrency, unsigned reserved_for_masters = 1);
        void terminate();
        bool is_active();
        %extend {
        void enqueue( PyObject *c ) { $self->enqueue(PyCaller(c)); }
        void execute( PyObject *c ) { $self->execute(PyCaller(c)); }
        };
    };

    class task_group {
    public:
        task_group();
        ~task_group();
        void wait();
        void cancel();
        %extend {
        void run( PyObject *c ) { $self->run(PyCaller(c)); }
        void run( PyObject *c, task_arena *a ) { $self->run(ArenaPyCaller(a, c)); }
        };
    };

    class global_control {
    public:
        enum parameter {
            max_allowed_parallelism,
            thread_stack_size,
            parameter_max // insert new parameters above this point
        };
        global_control(parameter param, size_t value);
        ~global_control();
        static size_t active_value(parameter param);
    };

} // tbb

%inline {
    inline const char* runtime_version() { return TBB_runtime_version();}
    inline int runtime_interface_version() { return TBB_runtime_interface_version();}
    inline int this_task_arena_max_concurrency() { return this_task_arena::max_concurrency();}
    inline int this_task_arena_current_thread_index() { return this_task_arena::current_thread_index();}
};

// Additional definitions for Python part of the module
%pythoncode %{
default_num_threads = this_task_arena_max_concurrency
%}
