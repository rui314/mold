.. _Using_Circular_Buffers:

Using Circular Buffers
======================


Circular buffers can sometimes be used to minimize the overhead of
allocating and freeing the items passed between pipeline filters. If the
first filter to create an item and last filter to consume an item are
both ``serial_in_order``, the items can be allocated and freed via a
circular buffer of size at least ``ntoken``, where ``ntoken`` is the
first parameter to ``parallel_pipeline``. Under these conditions, no
checking of whether an item is still in use is necessary.


The reason this works is that at most ``ntoken`` items can be in flight,
and items will be freed in the order that they were allocated. Hence by
the time the circular buffer wraps around to reallocate an item, the
item must have been freed from its previous use in the pipeline. If the
first and last filter are *not* ``serial_in_order``, then you have to
keep track of which buffers are currently in use, because buffers might
not be retired in the same order they were allocated.

