.. SPDX-FileCopyrightText: 2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===========
task_handle
===========
**[scheduler.task_handle]**

An instance of ``task_handle`` type owns a deferred task object.

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

.. namespace:: tbb::task_handle

.. cpp:function:: task_handle()

    Creates an empty ``task_handle`` object.

.. cpp:function:: task_handle(task_handle&& src)

    Constructs ``task_handle`` object with the content of ``src`` using move semantics. ``src`` becomes empty after the construction. 

.. cpp:function:: ~task_handle()

    Destroys the ``task_handle`` object and associated task if it exists. 

.. cpp:function:: task_handle& operator=(task_handle&& src)

    Replaces the content of ``task_handle`` object with the content of ``src`` using move semantics. ``src`` becomes empty after the assignment.
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