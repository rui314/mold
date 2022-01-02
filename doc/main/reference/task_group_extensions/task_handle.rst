.. _task_handle:

task_handle
===========

.. note::    To enable these extensions, set the ``TBB_PREVIEW_TASK_GROUP_EXTENSIONS`` macro to 1.


.. contents::
    :local:
    :depth: 1

Description
***********

This class owns a deferred task object. 

API
***

Header
------

.. code:: cpp

    #include <oneapi/tbb/task_group.h>

Synopsis
--------

.. code:: cpp

    namespace oneapi {
        namespace tbb {
      
            class task_handle {
            public:
                task_handle();
                task_handle(task_handle&& src);
                
                ~task_handle();
                
                task_handle& operator=(task_handle&& src);
            
                explicit operator bool() const noexcept;
            }; 
            
            bool operator==(task_handle const& h, std::nullptr_t) noexcept;
            bool operator==(std::nullptr_t, task_handle const& h) noexcept;
            
            bool operator!=(task_handle const& h, std::nullptr_t) noexcept;
            bool operator!=(std::nullptr_t, task_handle const& h) noexcept;
                      
        } // namespace tbb
    } // namespace oneapi

Member Functions
----------------

.. cpp:function:: task_handle()

Creates an empty ``task_handle`` object.

.. cpp:function:: task_handle(task_handle&& src)

Constructs ``task_handle`` object with the content of ``src`` using move semantics. ``src`` is left in an empty state.

.. cpp:function:: ~task_handle()

Destroys the ``task_handle`` object and associated task if it exists. 

.. cpp:function:: task_handle& operator=(task_handle&& src)

Replaces the content of ``task_handle`` object with the content of ``src`` using move semantics. ``src`` is left in an empty state.
The previously associated task object, if any, is destroyed before the assignment. 

**Returns:** Reference to ``*this``.

.. cpp:function:: explicit operator bool() const noexcept

Checks if ``*this`` has an associated task object.

**Returns:** ``true`` if ``*this`` is not empty, ``false`` otherwise.

Non-Member Functions
--------------------

.. code:: cpp

    bool operator==(task_handle const& h, std::nullptr_t) noexcept
    bool operator==(std::nullptr_t, task_handle const& h) noexcept

**Returns**: ``true`` if ``h`` is empty, ``false`` otherwise.

.. code:: cpp

    bool operator!=(task_handle const& h, std::nullptr_t) noexcept
    bool operator!=(std::nullptr_t, task_handle const& h) noexcept

**Returns**: ``true`` if ``h`` is not empty, ``false`` otherwise.
   
