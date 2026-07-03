.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==============
global_control
==============
**[scheduler.global_control]**

Use this class to control certain settings or behavior of the oneTBB dynamic library.

An object of class ``global_control``, or a "control variable", affects one of several behavioral aspects, or parameters, of TBB.
The ``global_control`` class is primarily intended for use at the application level, to control the whole application behavior.

The current set of parameters that you can modify is defined by the ``global_control::parameter`` enumeration.
The parameter and the value it should take are specified as arguments to the constructor of a control variable.
The impact of the control variable ends when its lifetime is complete.

Control variables can be created in different threads, and may have nested or overlapping scopes.
However, at any point in time each controlled parameter has a single active value that applies to the whole process.
This value is selected from all currently existing control variables by applying a parameter-specific selection rule.

.. code:: cpp

    // Defined in header <oneapi/tbb/global_control.h>

    namespace oneapi {
    namespace tbb {
        class global_control {
        public:
            enum parameter {
                max_allowed_parallelism,
                thread_stack_size,
                terminate_on_exception
            };

            global_control(parameter p, size_t value);
            ~global_control();

            static size_t active_value(parameter param);
        };
    } // namespace tbb
    } // namespace oneapi

Member types and constants
--------------------------

.. cpp:enum:: parameter::max_allowed_parallelism

    **Selection rule**: minimum

    Limits total number of worker threads that can be active in the task scheduler to ``parameter_value - 1``.

    .. note::

        With ``max_allowed_parallelism`` set to ``1``, ``global_control`` enforces serial execution
        of all tasks by the application thread(s), that is, the task scheduler does not allow worker threads to run.
        There is one exception: if some work is submitted for execution via ``task_arena::enqueue``,
        a single worker thread will still run ignoring the ``max_allowed_parallelism`` restriction.

.. cpp:enum:: parameter::thread_stack_size

    **Selection rule**: maximum

    Set stack size for working threads created by the library.

.. cpp:enum:: parameter::terminate_on_exception

    **Selection rule**: logical disjunction

    Setting the parameter to 1 causes termination in any condition that would throw or rethrow an exception.
    If set to 0 (default), the parameter does not affect the implementation behavior.

Member functions
----------------

.. cpp:function:: global_control(parameter param, size_t value)

    Constructs a ``global_control`` object with a specified control parameter and it's value.

.. cpp:function:: ~global_control()

    Destructs a control variable object and ends it's impact.

.. cpp:function:: static size_t active_value(parameter param)

    Returns the currently active value of the setting defined by ``param``.

See also:

* :doc:`task_arena <../task_arena/task_arena_cls>`
