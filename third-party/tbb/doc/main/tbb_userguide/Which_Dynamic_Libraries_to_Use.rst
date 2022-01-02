.. _Which_Dynamic_Libraries_to_Use:

Which Dynamic Libraries to Use
==============================


The template ``scalable_allocator<T>`` requires the |full_name| 
scalable memory allocator library as
described in **Scalable Memory Allocator**. It does not require the
oneTBB general library, and can be used independently of the rest of
oneTBB.


The templates ``tbb_allocator<T>`` and ``cache_aligned_allocator<T>``
use the scalable allocator library if it is present otherwise it reverts
to using ``malloc`` and ``free``. Thus, you can use these templates even
in applications that choose to omit the scalable memory allocator
library.


The rest of |full_name| can be used
with or without the oneTBB scalable memory allocator library.


.. container:: tablenoborder


   .. list-table:: 
      :header-rows: 1

      * -     Template     
        -     Requirements     
        -     Notes     
      * -     \ ``scalable_allocator<T>``     
        -     |full_name| scalable    memory allocator library. See **Scalable Memory Allocator**.    
        -           
      * -     \ ``tbb_allocator<T>``           \ ``cache_aligned_allocator<T>``    
        -           
        -     Uses the scalable allocator library if it is present,    otherwise it reverts to using ``malloc`` and ``free``.    



