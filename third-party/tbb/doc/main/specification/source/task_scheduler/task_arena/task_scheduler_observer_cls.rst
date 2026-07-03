.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=======================
task_scheduler_observer
=======================
**[scheduler.task_scheduler_observer]**

Class that represents thread interest in task scheduling services.

.. code:: cpp

    // Defined in header <oneapi/tbb/task_scheduler_observer.h>

    namespace oneapi {
    namespace tbb {

       class task_scheduler_observer {
       public:
           task_scheduler_observer();
           explicit task_scheduler_observer( task_arena& a );
           virtual ~task_scheduler_observer();

           void observe( bool state=true );
           bool is_observing() const;

           virtual void on_scheduler_entry( bool is_worker ) {}
           virtual void on_scheduler_exit( bool is_worker } {}
       };

    } // namespace tbb
    } // namespace oneapi

A ``task_scheduler_observer`` permits clients to observe when a thread starts
and stops processing tasks in a certain task scheduler arena.
You typically derive your own observer class from ``task_scheduler_observer``, and override
virtual methods ``on_scheduler_entry`` or ``on_scheduler_exit``.
Observation can be enabled and disabled for an observer instance; it is disabled on creation.
Remember to call ``observe()`` to enable observation.

Exceptions thrown and not caught in the overridden methods of ``task_scheduler_observer`` result in undefined behavior.

Member functions
----------------

.. cpp:function:: task_scheduler_observer()

    Constructs a ``task_scheduler_observer`` object in the inactive state (observation is disabled).
    For a created observer, entry/exit notifications are invoked whenever a worker
    thread joins/leaves the arena of the observer's owner thread. If a thread is
    already in the arena when the observer is activated, the entry notification is
    called before it executes the first stolen task.

.. cpp:function:: explicit task_scheduler_observer( task_arena& )

    Constructs a ``task_scheduler_observer`` object for a given arena in inactive state (observation is disabled).
    For created observer, entry/exit notifications are invoked whenever a thread joins/leaves arena.
    If a thread is already in the arena when the observer is activated, the entry notification
    is called before it executes the first stolen task.

    Constructs a ``task_scheduler_observer`` object in the inactive state (observation is disabled),
    which receives notifications from threads entering and exiting the specified ``task_arena``.

.. cpp:function:: ~task_scheduler_observer()

    Disables observing and destroys the observer instance.
    Waits for extant invocations of ``on_scheduler_entry`` and ``on_scheduler_exit`` to complete.

.. cpp:function:: void observe( bool state=true )

    Enables observing if ``state`` is true; disables observing if ``state`` is false.

.. cpp:function:: bool is_observing() const

    **Returns**: True if observing is enabled; false, otherwise.

.. cpp:function:: virtual void on_scheduler_entry( bool is_worker )

    The task scheduler invokes this method for each thread that starts participating
    in oneTBB work or enters an arena after the observation is enabled.
    For threads that already execute tasks, the method is invoked
    before executing the first task stolen after enabling the observation.

    If a thread enables the observation and then spawns a task, it is guaranteed that the task,
    as well as all the tasks it creates, will be executed by threads which have invoked ``on_scheduler_entry``.

    The flag ``is_worker`` is true if the thread was created by oneTBB; false, otherwise.

    **Effects**: The default behavior does nothing.

.. cpp:function:: virtual void on_scheduler_exit( bool is_worker )

    The task scheduler invokes this method when
    a thread stops participating in task processing or leaves an arena.

    .. caution::

        A process does not wait for the worker threads to clean up,
        and can terminate before ``on_scheduler_exit`` is invoked.

    **Effects**: The default behavior does nothing.

Example
-------

The following example sketches the code of an observer that pins oneTBB worker threads to hardware threads.

.. code:: cpp

    class pinning_observer : public oneapi::tbb::task_scheduler_observer {
    public:
        affinity_mask_t m_mask; // HW affinity mask to be used for threads in an arena
        pinning_observer( oneapi::tbb::task_arena &a, affinity_mask_t mask )
            : oneapi::tbb::task_scheduler_observer(a), m_mask(mask) {
            observe(true); // activate the observer
        }
        void on_scheduler_entry( bool worker ) override {
            set_thread_affinity(oneapi::tbb::this_task_arena::current_thread_index(), m_mask);
        }
        void on_scheduler_exit( bool worker ) override {
            restore_thread_affinity();
        }
    };

