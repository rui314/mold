.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========================
opencl_node Template Class
==========================


Summary
-------

``opencl_node`` enables execution of OpenCL* kernels on devices that have the OpenCL support,
such as integrated and discrete graphics processing units or CPUs. ``opencl_node`` simplifies
the integration of OpenCL kernels into programs powered by the TBB Flow Graph.
``opencl_node`` also handles memory buffer management in such programs.

Syntax
------

.. code:: cpp

   template < typename JP, typename Factory, typename... Ports >
   class opencl_node < tuple < Ports... >, JP, Factory >
   
   // As a Factory - default_opencl_factory is used
   template < typename JP, typename... Ports >
   class opencl_node < tuple < Ports... >, JP >
   
   // As a Factory - default_opencl_factory is used
   // As a JP - queueing policy is used
   template < typename... Ports >
   class opencl_node < tuple < Ports... > >


Header
------

.. code:: cpp

   #define TBB_PREVIEW_FLOW_GRAPH_NODES 1
   #define TBB_PREVIEW_FLOW_GRAPH_FEATURES 1
   #include "oneapi/tbb/flow_graph_opencl_node.h"

The ``"flow_graph_opencl_node.h"`` header is not included in ``"oneapi/tbb/tbb.h"``
and ``"oneapi/tbb/flow_graph.h"``.
Due to this, you should include ``"flow_graph_opencl_node.h"`` directly.

Description
-----------

``opencl_node`` provides a simple interface for working with devices that have an OpenCL support.
``opencl_node`` is implemented as a
:doc:`streaming_node <streaming_node_cls>`
that uses ``opencl_factory``. ``opencl_factory`` hides all complexity associated with kernel execution,
synchronization of memory with the device, and results retrieval. ``opencl_factory`` implements the Factory
concept and follows the ``streaming_node`` execution workflow.

``opencl_factory`` uses two separate entities (Device Filter and Device Selector) for device initialization and execution
management:

= ========================================================================================
\ opencl_factory device selection: Entity, Usage model
==========================================================================================
\ Device Filter
  \
  Used to initialize the ``opencl_factory`` with a set of devices that will be available
  for kernel execution. All filtered devices must belong to the same OpenCL Platform.
  Default device filter includes all available devices from the first OpenCL Platform found.
------------------------------------------------------------------------------------------
\ Device Selector
  \
  Used to select the device for a particular ``opencl_node`` instance to execute its kernels.
  Default device selector selects the first device from the list of available devices that
  was constructed by device filter. The selection is done for every kernel execution.
------------------------------------------------------------------------------------------
= ========================================================================================

Among other things, ``opencl_node`` provides a set of helper classes, which
simplifies work with OpenCL entities (devices, memory buffers, kernels, ND-ranges):

= ========================================================================================
\ opencl_node helper classes: Entity, Description
==========================================================================================
\ ``opencl_buffer``/``opencl_subbuffer``
  \
  A template class that is an abstraction over a strongly typed linear array.
  The OpenCL kernel works with special memory objects allocated on the host,
  ``opencl_buffer`` and ``opencl_subbuffer`` encapsulate
  the logic of the memory transactions between the host and the target.
------------------------------------------------------------------------------------------
\ ``opencl_device``
  \
  A class that is an abstraction over OpenCL device. Used mainly inside Device Selector and Device Filter
  and has a public API to query information about devices.
------------------------------------------------------------------------------------------
\ ``opencl_device_list``
  \
  A container of ``opencl_device`` instances.
------------------------------------------------------------------------------------------
\ ``opencl_program``
  \
  A class that you can use to specify the type of the kernel, read the kernel from a file, or build the kernel.
------------------------------------------------------------------------------------------
\ ``opencl_program_type``
  \
  The enumeration ``opencl_program_type`` provides the set of kernel types supported by ``opencl_node``:
  
  * ``SOURCE`` stands for OpenCL* C source code.
    An ``opencl_program`` instance initialized with ``SOURCE`` reads the source file and builds the
    kernel at run time. ``SOURCE`` is the default value for ``opencl_program``.
  * ``PRECOMPILED`` stands for machine-readable format that can be executed on the target.
    A kernel in this format does not require run-time compilation.
  * ``SPIR`` stands for Standard Portable Intermediate Representation (SPIR)*.
  
------------------------------------------------------------------------------------------
\ ``opencl_range``
  \
  Enables ``opencl_node`` to support ranges
  (see the OpenCL ``clEnqueueNDRangeKernel()`` method for details).
------------------------------------------------------------------------------------------
= ========================================================================================

**Argument binding**

By default, ``opencl_node`` binds the first input port to the first kernel argument,
the second input port to the second kernel argument and so on.
The ``set_args`` method of ``opencl_node`` allows binding a specific value to each kernel argument,
if they do not change from run to run. To specify that the value should be taken from the input port instead,
use the ``port_ref`` helper function. See its description and usage examples in the ``streaming_node``
section of the documentation.

Example
-------

The example below shows the
:doc:`cubator and squarer example <../../flow_graph/message_flow_graph_example>`
changed to offload part of computation to the GPU.
Kernels

.. include:: ../examples/vector_operations.cl
   :code: cpp


Example

.. include:: ../examples/opencl_node_example.cpp
   :code: cpp



Members
-------

.. code:: cpp

   namespace oneapi {
   namespace tbb {
   namespace flow {
   
   template < typename... Args >
   class opencl_node;
   
   template < typename JP, typename Factory, typename... Ports >
   class opencl_node < tuple < Ports... >, JP, Factory  > : public streaming_node < tuple < Ports... >, JP, Factory > {
   public:
       typedef implementation-dependent kernel_type;
   
       opencl_node( graph &g, const kernel_type& kernel );
   
       opencl_node( graph &g, const kernel_type& kernel, Factory &f );
   
       template < typename DeviceSelector >
       opencl_node( graph &g, const kernel_type& kernel, DeviceSelector d, Factory &f);
   };
   
   template < typename JP, typename... Ports >
   class opencl_node < tuple < Ports... >, JP  > : public opencl_node < tuple < Ports... >, JP, opencl_info::default_opencl_factory > {
   public:
       typedef implementation-dependent kernel_type;
   
       opencl_node( graph &g, const kernel_type& kernel );
   
       template < typename DeviceSelector >
       opencl_node( graph &g, const kernel_type& kernel, DeviceSelector d );
   };
   
   template < typename... Ports >
   class opencl_node < tuple < Ports... > > : public opencl_node < tuple < Ports... >, queueing, opencl_info::default_opencl_factory > {
   public:
       typedef implementation-dependent kernel_type;
   
       opencl_node( graph &g, const kernel_type& kernel );
   
       template < typename DeviceSelector >
       opencl_node( graph &g, const kernel_type& kernel, DeviceSelector d );
   };
   
   } // namespace flow
   } // namespace tbb
   } // namespace oneapi

The following table provides additional information on the members of this template class. All other
public members are described in the base ``streaming_node`` class.

= ========================================================================================
\ Member, Description
==========================================================================================
\ ``typename... Ports``
  \
  The node's incoming/outgoing data types.
------------------------------------------------------------------------------------------
\ ``typename JP``
  \
  Join Policy. See the description of the class ``join_node`` for details.
------------------------------------------------------------------------------------------
\ ``typename Factory``
  \
  The device-specific Factory type. ``opencl_factory`` is used for this node by default.
------------------------------------------------------------------------------------------
\ Main constructors
  \
  ``opencl_node( graph &g, const kernel_type& kernel )``
  
  ``opencl_node( graph &g, const kernel_type& kernel, Factory &f )``
  
  ``template < typename DeviceSelector > opencl_node( graph &g, const kernel_type& kernel, DeviceSelector d )``
  
  ``template < typename DeviceSelector > opencl_node( graph &g, const kernel_type& kernel, DeviceSelector d, Factory &f)``

  See ``streaming_node`` for details.
------------------------------------------------------------------------------------------
= ========================================================================================


See also:

* :doc:`streaming_node <streaming_node_cls>`
