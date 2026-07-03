%pythonbegin %{
#
# Copyright (c) 2016-2025 Intel Corporation
# Copyright (c) 2026 UXL Foundation Contributors
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
/* Defines Python wrappers for oneAPI Threading Building Blocks (oneTBB)
 *
 * Free-threading (NOGIL) Python 3.13+ Support:
 * This module declares Py_MOD_GIL_NOT_USED to indicate it can run safely
 * without the Global Interpreter Lock. All callbacks to Python code properly
 * acquire the GIL using SWIG_PYTHON_THREAD_BEGIN_BLOCK/END_BLOCK macros.
 */

%}
%module api

#if SWIG_VERSION < 0x030001
#error SWIG version 3.0.6 or newer is required for correct functioning
#endif

%{
#include "tbb/task_arena.h"
#include "tbb/task_group.h"
#include "tbb/global_control.h"
#include "tbb/version.h"

#include <condition_variable>
#include <mutex>
#include <memory>

using namespace tbb;

/*
 * PyCaller - Wrapper for Python callable objects
 * 
 * Thread-safety for free-threading Python:
 * - Uses SWIG_PYTHON_THREAD_BEGIN_BLOCK to acquire GIL before Python API calls
 * - Uses SWIG_PYTHON_THREAD_END_BLOCK to release GIL after Python API calls
 * - Reference counting (Py_INCREF/DECREF) is protected by GIL acquisition
 * 
 * This ensures safe operation when called from TBB worker threads.
 */
class PyCaller : public swig::SwigPtr_PyObject {
private:
    // Release the held Python object reference (GIL must NOT be held on entry)
    void release_ref() {
        if (_obj) {
            SWIG_PYTHON_THREAD_BEGIN_BLOCK;
            Py_XDECREF(_obj);
            SWIG_PYTHON_THREAD_END_BLOCK;
            _obj = nullptr;
        }
    }

public:
    // Copy constructor - must acquire GIL for Py_XINCREF
    PyCaller(const PyCaller& s) : SwigPtr_PyObject() {
        _obj = s._obj;
        SWIG_PYTHON_THREAD_BEGIN_BLOCK;
        Py_XINCREF(_obj);
        SWIG_PYTHON_THREAD_END_BLOCK;
    }
    
    PyCaller(PyObject *p, bool initial = true) : SwigPtr_PyObject(p, initial) {}
    
    // Destructor - release Python object reference
    ~PyCaller() {
        release_ref();
    }
    
    // Assignment operator
    PyCaller& operator=(const PyCaller& s) {
        if (this != &s) {
            release_ref();
            _obj = s._obj;
            SWIG_PYTHON_THREAD_BEGIN_BLOCK;
            Py_XINCREF(_obj);
            SWIG_PYTHON_THREAD_END_BLOCK;
        }
        return *this;
    }

    void operator()() const {
        /* Acquire GIL before calling Python code - required for free-threading */
        SWIG_PYTHON_THREAD_BEGIN_BLOCK;
        PyObject* r = PyObject_CallFunctionObjArgs((PyObject*)*this, nullptr);
        if(r) {
            Py_DECREF(r);
        } else {
            /* Log exception - cannot propagate from TBB worker thread */
            PyErr_WriteUnraisable((PyObject*)*this);
        }
        SWIG_PYTHON_THREAD_END_BLOCK;
    }
};

/*
 * ArenaPyCaller - Wrapper for Python callable with task_arena binding
 * 
 * Thread-safety: GIL is acquired for Py_XINCREF in constructor and
 * the actual Python call is delegated to PyCaller which handles GIL.
 */
struct ArenaPyCaller {
    task_arena *my_arena;
    PyObject *my_callable;
    
private:
    // Release the held Python callable reference (GIL must NOT be held on entry)
    void release_callable() {
        if (my_callable) {
            SWIG_PYTHON_THREAD_BEGIN_BLOCK;
            Py_XDECREF(my_callable);
            SWIG_PYTHON_THREAD_END_BLOCK;
            my_callable = nullptr;
        }
    }

public:
    ArenaPyCaller(task_arena *a, PyObject *c) : my_arena(a), my_callable(c) {
        SWIG_PYTHON_THREAD_BEGIN_BLOCK;
        Py_XINCREF(c);
        SWIG_PYTHON_THREAD_END_BLOCK;
    }
    
    // Copy constructor - needed because TBB may copy task functors
    ArenaPyCaller(const ArenaPyCaller& other) : my_arena(other.my_arena), my_callable(other.my_callable) {
        SWIG_PYTHON_THREAD_BEGIN_BLOCK;
        Py_XINCREF(my_callable);
        SWIG_PYTHON_THREAD_END_BLOCK;
    }
    
    // Destructor - release Python object reference
    ~ArenaPyCaller() {
        release_callable();
    }
    
    // Assignment operator
    ArenaPyCaller& operator=(const ArenaPyCaller& other) {
        if (this != &other) {
            release_callable();
            my_arena = other.my_arena;
            my_callable = other.my_callable;
            SWIG_PYTHON_THREAD_BEGIN_BLOCK;
            Py_XINCREF(my_callable);
            SWIG_PYTHON_THREAD_END_BLOCK;
        }
        return *this;
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

/*
 * _concurrency_barrier - Wait for all TBB worker threads to be ready
 * 
 * This function is thread-safe and does not require GIL as it only
 * uses C++ synchronization primitives (mutex, condition_variable).
 */
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
    b.event.wait(lock, [&b]{ return b.worker_threads >= b.full_threads; });
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
