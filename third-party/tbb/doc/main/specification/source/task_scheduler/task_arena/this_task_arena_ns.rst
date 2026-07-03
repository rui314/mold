.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
.. SPDX-FileCopyrightText: 2025 UXL Foundation Contributors
..
.. SPDX-License-Identifier: CC-BY-4.0

===============
this_task_arena
===============
**[scheduler.this_task_arena]**

The namespace for functions applicable to the current ``task_arena``.

The namespace ``this_task_arena`` contains global functions for interaction
with the ``task_arena`` currently used by the calling thread.

.. code:: cpp

    // Defined in header <oneapi/tbb/task_arena.h>

    namespace oneapi {
    namespace tbb {
        namespace this_task_arena {
            int current_thread_index();
            int max_concurrency();

            template<typename F> auto isolate(F&& f) -> decltype(f());
            
            template<typename F> void enqueue(F&& f);
            template<typename F> void enqueue(F&& f, task_group& tg);
            void enqueue(task_handle&& h);
        }
    } // namespace tbb
    } // namespace oneapi 

.. namespace:: tbb::this_task_arena

.. cpp:function:: int current_thread_index()

    Returns the thread index in a ``task_arena`` currently used by the calling thread,
    or ``task_arena::not_initialized`` if the thread has not yet initialized the task scheduler.

    A thread index is an integer number between 0 and the ``task_arena`` concurrency level.
    Thread indexes are assigned to both application threads and worker threads on joining an arena and are kept until exiting the arena.
    Indexes of threads that share an arena are unique, that is, no two threads within the arena can have the same
    index at the same time - but not necessarily consecutive.

    .. note::

        Since a thread may exit the arena at any time if it does not execute a task, the
        index of a thread may change between any two tasks, even those belonging to the
        same task group or algorithm.

    .. note::

        Threads that use different arenas may have the same current index value.

    .. note::

        Joining a nested arena in ``execute()`` may change current index
        value while preserving the index in the outer arena which will be restored on return.

.. cpp:function:: int max_concurrency()

    Returns the concurrency level of the ``task_arena`` currently used by the calling thread.
    If the thread has not yet initialized the task scheduler, returns the concurrency level
    determined automatically for the hardware configuration.

.. cpp:function:: template<typename F> auto isolate(F&& f) -> decltype(f())

    Runs the specified functor in isolation by restricting the calling thread to process only tasks
    scheduled in the scope of the functor (also called the isolation region). The function returns the value returned by the functor.
    The ``F`` type must meet the `Function Objects` requirements described in the [function.objects] section of the ISO C++ standard.

    .. caution::

        The object returned by the functor cannot be a reference. ``std::reference_wrapper`` can be used instead.

.. cpp:function:: template<typename F> void enqueue(F&& f)
  
    Enqueues a task into the ``task_arena`` currently used by the calling thread to process the specified functor, then returns immediately.
    The ``F`` type must meet the `Function Objects` requirements described in the [function.objects] section of the ISO C++ standard.

    Behavior of this function is equivalent to ``template<typename F> void task_arena::enqueue(F&& f)`` applied to the ``task_arena`` 
    object constructed with ``attach`` parameter.

.. cpp:function:: template<typename F> void enqueue(F&& f, task_group& tg)
  
    Adds a task to process the specified functor into ``tg`` and enqueues it into the ``task_arena`` currently used by the calling thread.

    The behavior of this function is equivalent to ``this_task_arena::enqueue( tg.defer(std::forward<F>(f)) )``.

.. cpp:function:: void enqueue(task_handle&& h)   
     
    Enqueues a task owned by ``h`` into the ``task_arena`` that is currently used by the calling thread.
    
    The behavior of this function is equivalent to the generic version (``template<typename F> void enqueue(F&& f)``), except the parameter type. 

    .. note:: 
        ``h`` should not be empty to avoid an undefined behavior.
