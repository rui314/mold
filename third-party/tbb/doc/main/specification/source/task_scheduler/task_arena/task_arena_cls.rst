.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
.. SPDX-FileCopyrightText: 2025 UXL Foundation Contributors
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
task_arena
==========
**[scheduler.task_arena]**

A class that represents an explicit, user-managed task scheduler arena.

.. code:: cpp

    // Defined in header <oneapi/tbb/task_arena.h>

    namespace oneapi {
        namespace tbb {

            class task_arena {
            public:
                static const int automatic = /* unspecified */;
                static const int not_initialized = /* unspecified */;
                enum class priority : /* unspecified type */ {
                    low = /* unspecified */,
                    normal = /* unspecified */,
                    high = /* unspecified */
                };

                struct constraints {
                    constraints(numa_node_id numa_node_       = task_arena::automatic,
                                int          max_concurrency_ = task_arena::automatic);

                    constraints& set_numa_id(numa_node_id id);
                    constraints& set_max_concurrency(int maximal_concurrency);
                    constraints& set_core_type(core_type_id id);
                    constraints& set_max_threads_per_core(int threads_number);

                    numa_node_id numa_id = task_arena::automatic;
                    int max_concurrency = task_arena::automatic;
                    core_type_id core_type = task_arena::automatic;
                    int max_threads_per_core = task_arena::automatic;
                };

                task_arena(int max_concurrency = automatic, unsigned reserved_slots = 1,
                           priority a_priority = priority::normal);
                task_arena(constraints constraints_, unsigned reserved_slots = 1,
                           priority a_priority = priority::normal);
                task_arena(const task_arena &s);
                explicit task_arena(oneapi::tbb::attach);
                ~task_arena();

                void initialize();
                void initialize(int max_concurrency, unsigned reserved_slots = 1,
                                priority a_priority = priority::normal);
                void initialize(constraints constraints_, unsigned reserved_slots = 1,
                                priority a_priority = priority::normal);
                void initialize(oneapi::tbb::attach);

                void terminate();

                bool is_active() const;
                int max_concurrency() const;

                template<typename F> auto execute(F&& f) -> decltype(f());

                template<typename F> void enqueue(F&& f);
                template<typename F> void enqueue(F&& f, task_group& tg);
                void enqueue(task_handle&& h);

                task_group_status task_arena::wait_for(task_group& tg);
            };

            std::vector<task_arena> create_numa_task_arenas(task_arena::constraints constraints_ = {},
                                                            unsigned reserved_slots = 0);

        } // namespace tbb
    } // namespace oneapi

A ``task_arena`` class represents a place where threads may share and execute tasks.

The number of threads that may simultaneously execute tasks in a ``task_arena`` is limited by its concurrency level.

Each user thread that invokes any parallel construction outside an explicit ``task_arena`` uses an implicit
task arena representation object associated with the calling thread.

The tasks spawned or enqueued into one arena cannot be executed in another arena.

Each ``task_arena`` has a ``priority``. The tasks from ``task_arena`` with higher priority are given
a precedence in execution over the tasks from ``task_arena`` with lower priority.

.. note::

    The ``task_arena`` constructors do not create an internal task arena representation object.
    It may already exist in case of the "attaching" constructor; otherwise, it is created
    by an explicit call to ``task_arena::initialize`` or lazily on first use.

Member types and constants
--------------------------

.. cpp:member:: static const int automatic

    When passed as ``max_concurrency`` to the specific constructor, arena
    concurrency is automatically set based on the hardware configuration.

.. cpp:member:: static const int not_initialized

    When returned by a method or function, indicates that there is no active ``task_arena``
    or that the ``task_arena`` object has not yet been initialized.

.. cpp:enum:: priority::low

    When passed to a constructor or the ``initialize`` method, the initialized ``task_arena``
    has a lowered priority.

.. cpp:enum:: priority::normal

    When passed to a constructor or the ``initialize`` method, the initialized ``task_arena``
    has regular priority.

.. cpp:enum:: priority::high

    When passed to a constructor or the ``initialize`` method, the initialized ``task_arena``
    has a raised priority.

.. cpp:struct:: constraints

    Represents limitations applied to threads within ``task_arena``.

    Starting from C++20 this class should be an aggregate type to support the designated initialization.

.. cpp:member:: numa_node_id constraints::numa_id

    An integral logical index uniquely identifying a NUMA node.
    If set to non-automatic value, then this NUMA node will be considered as preferred for all the
    threads within the arena.

    .. note::

        NUMA node ID is considered valid if it was obtained through tbb::info::numa_nodes().

.. cpp:member:: int constraints::max_concurrency

    The maximum number of threads that can participate in work processing
    within the ``task_arena`` at the same time.

.. cpp:member:: core_type_id constraints::core_type

    An integral logical index uniquely identifying a core type.
    If set to non-automatic value, then this core type will be considered as preferred for all the
    threads within the arena.

    .. note::

        core type ID is considered valid if it was obtained through ``tbb::info::core_types()``.

.. cpp:member:: int constraints::max_threads_per_core

    The maximum number of threads that can be scheduled to one core simultaneously.

.. cpp:function:: constraints::constraints(numa_node_id numa_node_ = task_arena::automatic, int max_concurrency_ = task_arena::automatic)

    Constructs the constraints object with the provided `numa_id` and `max_concurrency` settings.

    .. note::

        To support designated initialization this constructor is omitted starting from C++20. Aggregate initialization is supposed to be used instead.

.. cpp:function:: constraints& constraints::set_numa_id(numa_node_id id)

    Sets the `numa_id` to the provided ``id``. Returns the reference to the updated constraints object.

.. cpp:function:: constraints& constraints::set_max_concurrency(int maximal_concurrency)

    Sets the `max_concurrency` to the provided ``maximal_concurrency``. Returns the reference to the updated constraints object.

.. cpp:function:: constraints& constraints::set_core_type(core_type_id id)

    Sets the `core_type` to the provided ``id``. Returns the reference to the updated constraints object.

.. cpp:function:: constraints& constraints::set_max_threads_per_core(int threads_number)

    Sets the `max_threads_per_core` to the provided ``threads_number``. Returns the reference to the updated constraints object.

Member functions
----------------

.. cpp:function:: task_arena(int max_concurrency = automatic, unsigned reserved_slots = 1, priority a_priority = priority::normal)

    Creates a ``task_arena`` with a certain concurrency limit (``max_concurrency``) and priority
    (``a_priority``).  Some portion of the limit can be reserved for application threads with
    ``reserved_slots``.  The amount for reservation cannot exceed the limit.

    .. caution::

        If ``max_concurrency`` and ``reserved_slots`` are
        explicitly set to be equal and greater than 1, oneTBB worker threads will never
        join the arena. As a result, the execution guarantee for enqueued tasks is not valid
        in such arena. Do not use ``task_arena::enqueue()`` with an arena set to have no worker threads.

.. cpp:function:: task_arena(constraints constraints_, unsigned reserved_slots = 1, priority a_priority = priority::normal)

    Creates a ``task_arena`` with a certain constraints(``constraints_``) and priority
    (``a_priority``).  Some portion of the limit can be reserved for application threads with
    ``reserved_slots``.  The amount for reservation cannot exceed the concurrency limit specified in ``constraints``.

    .. caution::

        If ``constraints::max_concurrency`` and ``reserved_slots`` are
        explicitly set to be equal and greater than 1, oneTBB worker threads will never
        join the arena. As a result, the execution guarantee for enqueued tasks is not valid
        in such arena. Do not use ``task_arena::enqueue()`` with an arena set to have no worker threads.

    If ``constraints::numa_node`` is specified, then all threads that enter the arena are automatically
    pinned to corresponding NUMA node.

.. cpp:function:: task_arena(const task_arena&)

    Copies settings from another ``task_arena`` instance.

.. cpp:function:: explicit task_arena(oneapi::tbb::attach)

    Creates an instance of ``task_arena`` that is connected to the internal task arena representation currently used by the calling thread.
    If no such arena exists yet, creates a ``task_arena`` with default parameters.

    .. note::

        Unlike other constructors, this one automatically initializes
        the new ``task_arena`` when connecting to an already existing arena.

.. cpp:function:: ~task_arena()

    Destroys the ``task_arena`` instance, but the destruction may not be synchronized with any task execution inside this ``task_arena``.
    It means that an internal task arena representation associated with this ``task_arena`` instance can be destroyed later.
    Not thread-safe for concurrent invocations of other methods.

.. cpp:function:: void initialize()

    Performs actual initialization of internal task arena representation.

    .. note::

        After the call to ``initialize``, the arena parameters are fixed and cannot be changed.

.. cpp:function:: void initialize(int max_concurrency, unsigned reserved_slots = 1, priority a_priority = priority::normal)

    Same as above, but overrides previous arena parameters.

.. cpp:function:: void initialize(constraints constraints_, unsigned reserved_slots = 1, priority a_priority = priority::normal)

    Same as above.

.. cpp:function:: void initialize(oneapi::tbb::attach)

    If an internal task arena representation currently used by the calling thread, the method ignores arena
    parameters and connects ``task_arena`` to that internal task arena representation.
    The method has no effect when called for an already initialized ``task_arena``.

.. cpp:function:: void terminate()

    Removes the reference to the internal task arena representation without destroying the
    task_arena object, which can then be re-used. Not thread safe for concurrent invocations of other methods.

.. cpp:function:: bool is_active() const

    Returns ``true`` if the ``task_arena`` has been initialized; ``false``, otherwise.

.. cpp:function:: int max_concurrency() const

    Returns the concurrency level of the ``task_arena``.
    Does not require the ``task_arena`` to be initialized and does not perform initialization.

.. cpp:function:: template<typename F> auto execute(F&& f) -> decltype(f())

    Executes the specified functor in the ``task_arena`` and returns the value returned by the functor.
    The ``F`` type must meet the `Function Objects` requirements described in the [function.objects] section of the ISO C++ standard.

    The calling thread joins the ``task_arena`` if possible, and executes the functor.
    Upon return it restores the previous task scheduler state and floating-point settings.

    If joining the ``task_arena`` is not possible, the call wraps the functor into a task,
    enqueues it into the arena, waits using an OS kernel synchronization object
    for another opportunity to join, and finishes after the task completion.

    An exception thrown in the functor will be captured and re-thrown from ``execute``.

    .. note::

        Any number of threads outside of the arena can submit work to the arena and be blocked.
        However, only the maximal number of threads specified for the arena can participate in executing the work.

.. cpp:function:: template<typename F> void enqueue(F&& f)

    Enqueues a task into the ``task_arena`` to process the specified functor and immediately returns.
    The ``F`` type must meet the `Function Objects` requirements described in the [function.objects] section of the ISO C++ standard.
    The task is scheduled for eventual execution by a worker thread even if no thread ever explicitly waits for the task to complete.
    If the total number of worker threads is zero, a special additional worker thread is created to execute enqueued tasks.

    .. note::

        The method does not require the calling thread to join the arena; that is, any number
        of threads outside of the arena can submit work to it without blocking.

    .. caution::

        There is no guarantee that tasks enqueued into an arena execute concurrently with
        respect to any other tasks there.

    .. caution::

        An exception thrown and not caught in the functor results in undefined behavior.

.. cpp:function:: template<typename F> void enqueue(F&& f, task_group& tg)

    Adds a task to process the specified functor into ``tg`` and enqueues it into the ``task_arena``.

    The behavior of this function is equivalent to ``this->enqueue( tg.defer(std::forward<F>(f)) )``.

.. cpp:function:: void enqueue(task_handle&& h)   
     
    Enqueues a task owned by ``h`` into the ``task_arena`` for processing. 
 
    The behavior of this function is equivalent to the generic version (``template<typename F> void task_arena::enqueue(F&& f)``), except parameter type. 

    .. note:: 
       ``h`` should not be empty to avoid an undefined behavior.

.. cpp:function:: task_group_status task_arena::wait_for(task_group& tg)

    Waits for all tasks in ``tg`` to complete or be cancelled, while possibly executing tasks in the ``task_arena``.
    Returns the status of ``tg`` once waiting is complete.

    The behavior of this function is equivalent to ``this->execute([&tg]{ return tg.wait(); })``.

Non-member Functions
--------------------

.. cpp:function:: std::vector<task_arena> create_numa_task_arenas(task_arena::constraints constraints_ = {}, unsigned reserved_slots = 0)

    Returns a ``std::vector`` of non-initialized ``task_arena`` objects, each bound to a separate NUMA node.
    The number of created ``task_arena`` instances is equal to the number of NUMA nodes on the system,
    as determined by ``tbb::info::numa_nodes()``.
    
    If an error occurs during system information discovery,
    returns a ``std::vector`` containing a single ``task_arena`` object created as 
    ``task_arena(constraints_.set_numa_id(task_arena::automatic), reserved_slots)``.

    The ``constraints_`` argument can be specified to apply additional limitations to threads in
    the ``task_arena`` objects. For each created arena, the ``numa_id`` value in ``constraints_``
    is automatically set to the corresponding NUMA node ID from ``tbb::info::numa_nodes()``.
    
    The ``reserved_slots`` argument allows reserving a specified number of slots in
    each ``task_arena`` object for application threads. By default, no slots are reserved.

Example
-------

The example demonstrates ``task_arena`` NUMA support API. Each constructed ``task_arena`` is pinned
to the corresponding NUMA node.

.. code:: cpp

    #include "oneapi/tbb/task_group.h"
    #include "oneapi/tbb/task_arena.h"

    #include <vector>

    int main() {
        std::vector<oneapi::tbb::task_arena> arenas = oneapi::tbb::create_numa_task_arenas();
        std::vector<oneapi::tbb::task_group> task_groups(arenas.size()-1);

        for (int i = 1; i < arenas.size(); i++) {
            arenas[i].enqueue([]{
                /* executed by a thread pinned to the specified NUMA node */
            }, task_groups[i-1]);
        }

        arenas[0].execute([] {
            /* executed by the main thread pinned to the NUMA node for arenas[0] */
        });

        for (int i = 1; i < arenas.size(); i++) {
            arenas[i].wait_for(task_groups[i-1]);
        }

        return 0;
    }


See also:

* :doc:`attach <../attach_tag_type>`
* :doc:`task_group <../task_group/task_group_cls>`
* :doc:`task_scheduler_observer <task_scheduler_observer_cls>`
