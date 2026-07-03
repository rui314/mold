.. _task_completion_handle_cls:

``task_completion_handle`` Class
================================

.. note::
    To enable this extension, define the ``TBB_PREVIEW_TASK_GROUP_EXTENSIONS`` macro with a value of ``1``.

.. contents::
    :local:
    :depth: 2

Description
***********

An instance of the ``task_completion_handle`` class represents a task for the purpose of
setting execution dependencies and tracking task completion.
Unlike ``task_handle``, which becomes empty once the task
is submitted for execution, a ``task_completion_handle`` keeps referencing a task
regardless of whether it is submitted, executing, or completed.

.. code:: cpp

    tbb::task_group tg;

    tbb::task_handle th = tg.defer(task_body);
    // task is not submitted
    // th is non-empty and represents the task

    tbb::task_completion_handle tch = th;
    // task is not submitted
    // both th and tch are non-empty and represent the task

    tg.run(std::move(th));
    // task is submitted
    // th is empty
    // tch is non-empty and keeps representing the task

    tg.wait();
    // task is completed
    // tch is non-empty and represents the completed task

The ``task_completion_handle`` class is used for:

* Establishing :ref:`dynamic task dependencies <dynamic_dependencies>`: it can serve as
  a predecessor in ``task_group::set_task_order`` to add successors at any time, including
  after the corresponding task has been submitted and even completed.
* :ref:`Waiting for the completion of an individual task <wait_single_task>` using ``task_group::wait``,
  without waiting for all tasks in the group to finish.

A non-empty ``task_completion_handle`` can be obtained by constructing or assigning from a
non-empty ``task_handle`` or by copying another non-empty ``task_completion_handle``.
Multiple ``task_completion_handle`` objects may reference the same task simultaneously.
An empty ``task_completion_handle`` does not reference any task.

API
***

Header
------

.. code:: cpp

    #define TBB_PREVIEW_TASK_GROUP_EXTENSIONS 1
    #include <oneapi/tbb/task_group.h>

Synopsis
--------

.. code:: cpp

    namespace oneapi {
        namespace tbb {

            class task_completion_handle {
            public:
                task_completion_handle();

                task_completion_handle(const task_handle& handle);
                task_completion_handle(const task_completion_handle& other);
                task_completion_handle(task_completion_handle&& other);

                ~task_completion_handle();

                task_completion_handle& operator=(const task_handle& handle);
                task_completion_handle& operator=(const task_completion_handle& other);
                task_completion_handle& operator=(task_completion_handle&& other);
   
                explicit operator bool() const noexcept;

                friend bool operator==(const task_completion_handle& lhs,
                                       const task_completion_handle& rhs) noexcept;
                friend bool operator!=(const task_completion_handle& lhs,
                                       const task_completion_handle& rhs) noexcept;

                friend bool operator==(const task_completion_handle& t, std::nullptr_t) noexcept;
                friend bool operator!=(const task_completion_handle& t, std::nullptr_t) noexcept;

                friend bool operator==(std::nullptr_t, const task_completion_handle& t) noexcept;
                friend bool operator!=(std::nullptr_t, const task_completion_handle& t) noexcept;
            }; // class task_completion_handle

        } // namespace tbb
    } // namespace oneapi

Member Functions
----------------

Constructors
~~~~~~~~~~~~

.. code:: cpp

    task_completion_handle();

Constructs an empty ``task_completion_handle`` that does not refer to any task.

.. code:: cpp

    task_completion_handle(const task_handle& handle);

Constructs a ``task_completion_handle`` that refers to the task associated with ``handle``.
If ``handle`` is empty, the behavior is undefined.

.. code:: cpp

    task_completion_handle(const task_completion_handle& other);

Copies ``other`` into ``*this``. After the copy, both ``*this`` and ``other`` refer to the same task.

.. code:: cpp

    task_completion_handle(task_completion_handle&& other);

Moves ``other`` into ``*this``. After the move, ``*this`` refers to the task previously referenced by ``other``, which is left empty.

Destructor
~~~~~~~~~~

.. code:: cpp

    ~task_completion_handle();

Destroys the ``task_completion_handle``.

Assignment
~~~~~~~~~~

.. code:: cpp

    task_completion_handle& operator=(const task_handle& handle);

Replaces the task referenced by ``*this`` with the task associated with ``handle``.
If ``handle`` is empty, the behavior is undefined.

*Returns*: a reference to ``*this``.

.. code:: cpp

    task_completion_handle& operator=(const task_completion_handle& other);

Performs copy assignment from ``other`` to ``*this``. After the assignment, both refer to the same task.

*Returns*: a reference to ``*this``.

.. code:: cpp

    task_completion_handle& operator=(task_completion_handle&& other);

Performs move assignment from ``other`` to ``*this``. After the move, ``*this`` refers to the task previously referenced by ``other``, which is left empty.

*Returns*: a reference to ``*this``.

Observers
~~~~~~~~~

.. code:: cpp

    explicit operator bool() const noexcept;

*Returns*: ``true`` if ``*this`` references a task; otherwise, ``false``.

Comparison
~~~~~~~~~~

.. code:: cpp

    bool operator==(const task_completion_handle& lhs, const task_completion_handle& rhs) noexcept;

*Returns*: ``true`` if ``lhs`` and ``rhs`` reference the same task; otherwise, ``false``.

.. code:: cpp

    bool operator!=(const task_completion_handle& lhs, const task_completion_handle& rhs) noexcept;

Equivalent to ``!(lhs == rhs)``.

.. code:: cpp

    bool operator==(const task_completion_handle& t, std::nullptr_t) noexcept;
    bool operator==(std::nullptr_t, const task_completion_handle& t) noexcept;

*Returns*: ``true`` if ``t`` does not reference any task; otherwise, ``false``.

.. code:: cpp

    bool operator!=(const task_completion_handle& t, std::nullptr_t) noexcept;
    bool operator!=(std::nullptr_t, const task_completion_handle& t) noexcept;

Equivalent to ``!(t == nullptr)``.
