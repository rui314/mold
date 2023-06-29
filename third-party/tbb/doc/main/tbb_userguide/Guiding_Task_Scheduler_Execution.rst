.. _guiding_task_scheduler_execution:

Guiding Task Scheduler Execution
================================

By default, the task scheduler tries to use all available computing resources. In some cases,
you may want to configure the task scheduler to use only some of them.

.. caution::

    Guiding the execution of the task scheduler may cause composability issues.

|full_name| provides the ``task_arena`` interface to guide tasks execution within the arena by:
    - setting the preferred computation units;
    - restricting part of computation units.

Such customizations are encapsulated within the ``task_arena::constraints`` structure.
To set the limitation, you have to customize the ``task_arena::constraints`` and then pass
it to the ``task_arena`` instance during the construction or initialization.

The structure ``task_arena::constraints`` allows to specify the following restrictions:

- Preferred NUMA node
- Preferred core type
- The maximum number of logical threads scheduled per single core simultaneously
- The level of ``task_arena`` concurrency

You may use the interfaces from ``tbb::info`` namespace to construct the ``tbb::task_arena::constraints``
instance. Interfaces from ``tbb::info`` namespace respect the process affinity mask. For instance,
if the process affinity mask excludes execution on some of the NUMA nodes, then these NUMA nodes are
not returned by ``tbb::info::numa_nodes()`` interface.

The following examples show how to use these interfaces:

.. rubric:: Setting the preferred NUMA node

The execution on systems with non-uniform memory access (`NUMA <https://en.wikipedia.org/wiki/Non-uniform_memory_access>`_ systems)
may cause a performance penalty if threads from one NUMA node access the memory allocated on
a different NUMA node. To reduce this overhead, the work may be divided among several ``task_arena``
instances, whose execution preference is set to different NUMA nodes. To set execution preference,
assign a NUMA node identifier to the ``task_arena::constraints::numa_id`` field.

::

    std::vector<tbb::numa_node_id> numa_indexes = tbb::info::numa_nodes();
    std::vector<tbb::task_arena> arenas(numa_indexes.size());
    std::vector<tbb::task_group> task_groups(numa_indexes.size());

    for(unsigned j = 0; j < numa_indexes.size(); j++) {
        arenas[j].initialize(tbb::task_arena::constraints(numa_indexes[j]));
        arenas[j].execute([&task_groups, &j](){ 
            task_groups[j].run([](){/*some parallel stuff*/});
        });
    }

    for(unsigned j = 0; j < numa_indexes.size(); j++) {
        arenas[j].execute([&task_groups, &j](){ task_groups[j].wait(); });
    }

.. rubric:: Setting the preferred core type

The processors with `Intel® Hybrid Technology <https://www.intel.com/content/www/us/en/products/docs/processors/core/core-processors-with-hybrid-technology-brief.html>`_
contain several core types, each is suited for different purposes.
In most cases, systems with hybrid CPU architecture show reasonable performance without involving additional API calls.
However, in some exceptional scenarios, performance may be tuned by setting the preferred core type.
To set the preferred core type for the execution, assign a specific core type identifier to the ``task_arena::constraints::core_type`` field.

The example shows how to set the most performant core type as preferable for work execution:

::

    std::vector<tbb::core_type_id> core_types = tbb::info::core_types();
    tbb::task_arena arena(
        tbb::task_arena::constraints{}.set_core_type(core_types.back())
    );

    arena.execute( [] {
        /*the most performant core type is defined as preferred.*/
    });

.. rubric:: Limiting the maximum number of threads simultaneously scheduled to one core

The processors with `Intel® Hyper-Threading Technology <https://www.intel.com/content/www/us/en/architecture-and-technology/hyper-threading/hyper-threading-technology.html>`_
allow more than one thread to run on each core simultaneously. However, there might be situations
when there is need to lower the number of simultaneously running threads per core. In such cases,
assign the desired value to the ``task_arena::constraints::max_threads_per_core`` field.

The example shows how to allow only one thread to run on each core at a time:

::

    tbb::task_arena no_ht_arena( tbb::task_arena::constraints{}.set_max_threads_per_core(1) );
    no_ht_arena.execute( [] {
        /*parallel work*/
    });

A more composable way to limit the number of threads executing on cores is by setting the maximal
concurrency of the ``tbb::task_arena``: 

::

    int no_ht_concurrency = tbb::info::default_concurrency(
        tbb::task_arena::constraints{}.set_max_threads_per_core(1)
    );
    tbb::task_arena arena( no_ht_concurrency );
    arena.execute( [] {
        /*parallel work*/
    });

Similarly to the previous example, the number of threads inside the arena is equal to the
number of available cores. However, this one results in fewer overheads and better composability
by imposing a less constrained execution.
