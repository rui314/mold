.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===================
handle_object class
===================


Summary
-------

Class that provides read and write access to values
stored in ``concurrent_lru_cache``.

Syntax
------

.. code:: cpp

   template <typename key_type, typename value_type, typename value_functor_type>
   class concurrent_lru_cache<key_type, value_type, value_functor_type>::handle;

The implementation might declare ``concurrent_lru_cache::handle``
as a ``typedef`` alias to an implementation-defined class.

Header
------

.. code:: cpp

   #include "oneapi/tbb/concurrent_lru_cache.h"


Description
-----------

``handle`` is a proxy object
that refers to a value stored in a ``concurrent_lru_cache`` container.

An alive ``handle`` object prevents the container
from erasing the value which it holds reference to. The reference is released when
``handle`` is reassigned or destroyed. Once the last reference
to a value is released the container is allowed to erase the value.

A ``handle`` object cannot be copied.
Instead it allows transferring the reference to another ``handle`` instance
via implicit conversion to and from ``handle_move_t`` prior to C++11,
and via move semantics starting from C++11.

Members and free-standing functions
-----------------------------------

.. code:: cpp

   namespace oneapi {
   namespace tbb {
       template <typename key_type,
                 typename value_type,
                 typename value_functor_type>
       class concurrent_lru_cache<key_type, value_type, value_functor_type>::handle {
       public:
           handle();
           ~handle();
   
           // Supported since C++11
           handle(handle&& src);
           handle& operator=(handle&& src);
   
           // Supported until C++11
           handle(handle_move_t m);
           handle& operator=(handle_move_t m);
           operator handle_move_t();
           friend handle_move_t move(handle& h);
   
           operator bool() const;
           value_type& value();
   
       private:
           void operator=(handle&);
           handle(handle&);
       };
   
   } // namespace tbb
   } // namespace oneapi

The following table provides additional information on the
members of this class.

= ========================================================================================
\ Member, Description
==========================================================================================
\ ``handle()``
  \
  Constructs a ``handle`` object that
  does not refer to any value.
------------------------------------------------------------------------------------------
\ ``~handle()``
  \
  Releases the reference (if exists) to a value stored in
  ``concurrent_lru_cache``.
------------------------------------------------------------------------------------------
\ ``handle(handle&& src)``
  \
  **Supported since C++11.**
  Move constructor transfers the reference to a value stored in
  ``concurrent_lru_cache`` from ``src``
  to the newly constructed object. Upon completion, ``src``
  no longer refers to any value.
------------------------------------------------------------------------------------------
\ ``handle& operator=(handle&& src)``
  \
  **Supported since C++11.**
  Move assignment operator transfers the reference to a value stored in
  ``concurrent_lru_cache`` from ``src`` to
  ``*this``. If exists, the previous reference held by
  ``*this`` is released. Upon completion, ``src``
  no longer refers to any value.
  
  **Returns**: ``*this``.
------------------------------------------------------------------------------------------
\ Pre-C++11 construction and assignment
  \
  ``handle(handle_move_t m)``
  
  ``handle& operator=(handle_move_t m)``

  **Supported until C++11.**
  Enables a ``handle`` object to be constructed and assigned from 
  a ``handle_move_t`` object. Together with converters to
  ``handle_move_t`` described below, these methods allow
  transferring references to ``concurrent_lru_cache`` items
  between ``handle`` instances in absence of C++11 move semantics.
------------------------------------------------------------------------------------------
\ Pre-C++11 move operations
  \
  ``operator handle_move_t()``
  
  ``friend handle_move_t move(handle& h)``

  **Supported until C++11.** A conversion operator and
  a free-standing friend function to transfer the reference held by
  a ``handle`` object to a temporary ``handle_move_t`` object.
  The conversion operator should not be called directly, use
  the ``move`` function instead. Upon completion, the ``handle``
  object no longer refers to any value.
  
  **Return**: ``handle_move_t`` object
  referring to the value previously referred by the given ``handle``.
------------------------------------------------------------------------------------------
\ ``operator bool() const``
  \
  Checks if the ``handle`` object holds reference to any value.
  
  **Returns**: ``true`` if ``*this``
  holds reference to a value stored in ``concurrent_lru_cache``;
  ``false``, otherwise.
------------------------------------------------------------------------------------------
\ ``value_type& value()``
  \
  **Returns**: a reference to a ``value_type``
  object stored in ``concurrent_lru_cache``.
  
  Calling the method for a ``handle`` object
  that does not refer to any value results in undefined behavior.
------------------------------------------------------------------------------------------
= ========================================================================================
