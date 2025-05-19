.. _Working_on_the_Assembly_Line_pipeline:

Working on the Assembly Line: parallel_pipeline
===============================================


*Pipelining* is a common parallel pattern that mimics a traditional
manufacturing assembly line. Data flows through a series of pipeline
filters and each filter processes the data in some way. Given an
incoming stream of data, some of these filters can operate in parallel,
and others cannot. For example, in video processing, some operations on
frames do not depend on other frames, and so can be done on multiple
frames at the same time. On the other hand, some operations on frames
require processing prior frames first.


The |full_name| classes
``parallel_pipeline`` and filter implement the pipeline pattern. A
simple text processing example will be used to demonstrate the usage of
``parallel_pipeline`` and filter to perform parallel formatting. The
example reads a text file, squares each decimal numeral in the text, and
writes the modified text to a new file. Below is a picture of the
pipeline.


.. CAUTION:: 
   Since the body object provided to the filters of the
   ``parallel_pipeline`` might be copied, its ``operator()`` should not
   modify the body. Otherwise the modification might or might not become
   visible to the thread that invoked ``parallel_pipeline``, depending
   upon whether ``operator()`` is acting on the original or a copy. As a
   reminder of this nuance, ``parallel_pipeline`` requires that the body
   object's ``operator()`` be declared ``const``.


.. container:: tablenoborder


   .. list-table:: 
      :header-rows: 0

      * -     Read chunk from input file    
        -     |image0|
        -     Square numerals in chunk    
        -     |image1|
        -     Write chunk to output file    




Assume that the raw file I/O is sequential. The squaring filter can be
done in parallel. That is, if you can serially read ``n`` chunks very
quickly, you can transform each of the ``n`` chunks in parallel, as long
as they are written in the proper order to the output file. Though the
raw I/O is sequential, the formatting of input and output can be moved
to the middle filter, and thus be parallel.


To amortize parallel scheduling overheads, the filters operate on chunks
of text. Each input chunk is approximately 4000 characters. Each chunk
is represented by an instance of class ``TextSlice``:


::


   // Holds a slice of text.
   /** Instances *must* be allocated/freed using methods herein, because the C++ declaration
      represents only the header of a much larger object in memory. */
   class TextSlice {
       // Pointer to one past last character in sequence
       char* logical_end;
       // Pointer to one past last available byte in sequence.
       char* physical_end;
   public:
       // Allocate a TextSlice object that can hold up to max_size characters.
       static TextSlice* allocate( size_t max_size ) {
           // +1 leaves room for a terminating null character.
           TextSlice* t = (TextSlice*)oneapi::tbb::tbb_allocator<char>().allocate( sizeof(TextSlice)+max_size+1 );
           t->logical_end = t->begin();
           t->physical_end = t->begin()+max_size;
           return t;
       }
       // Free this TextSlice object
       void free() {
           oneapi::tbb::tbb_allocator<char>().deallocate((char*)this, sizeof(TextSlice)+(physical_end-begin())+1);
       }
       // Pointer to beginning of sequence
       char* begin() {return (char*)(this+1);}
       // Pointer to one past last character in sequence
       char* end() {return logical_end;}
       // Length of sequence
       size_t size() const {return logical_end-(char*)(this+1);}
       // Maximum number of characters that can be appended to sequence
       size_t avail() const {return physical_end-logical_end;}
       // Append sequence [first,last) to this sequence.
       void append( char* first, char* last ) {
           memcpy( logical_end, first, last-first );
           logical_end += last-first;
       }
       // Set end() to given value.
       void set_end( char* p ) {logical_end=p;}
   };


Below is the top-level code for building and running the pipeline.
``TextSlice`` objects are passed between filters using pointers to avoid
the overhead of copying a ``TextSlice``.


::


   void RunPipeline( int ntoken, FILE* input_file, FILE* output_file ) {
       oneapi::tbb::parallel_pipeline(
           ntoken,
           oneapi::tbb::make_filter<void,TextSlice*>(
               oneapi::tbb::filter_mode::serial_in_order, MyInputFunc(input_file) )
       &
           oneapi::tbb::make_filter<TextSlice*,TextSlice*>(
               oneapi::tbb::filter_mode::parallel, MyTransformFunc() )
       &
           oneapi::tbb::make_filter<TextSlice*,void>(
               oneapi::tbb::filter_mode::serial_in_order, MyOutputFunc(output_file) ) );
   } 


The parameter ``ntoken`` to method ``parallel_pipeline`` controls the
level of parallelism. Conceptually, tokens flow through the pipeline. In
a serial in-order filter, each token must be processed serially in
order. In a parallel filter, multiple tokens can by processed in
parallel by the filter. If the number of tokens were unlimited, there
might be a problem where the unordered filter in the middle keeps
gaining tokens because the output filter cannot keep up. This situation
typically leads to undesirable resource consumption by the middle
filter. The parameter to method ``parallel_pipeline`` specifies the
maximum number of tokens that can be in flight. Once this limit is
reached, the pipeline never creates a new token at the input filter
until another token is destroyed at the output filter.


The second parameter specifies the sequence of filters. Each filter is
constructed by function ``make_filter<inputType, outputType>(mode,functor)``.


-  The *inputType* specifies the type of values input by a filter. For
   the input filter, the type is ``void``.


-  The *outputType* specifies the type of values output by a filter. For
   the output filter, the type is ``void``.


-  The *mode* specifies whether the filter processes items in parallel,
   serial in-order, or serial out-of-order.


-  The *functor* specifies how to produce an output value from an input
   value.


The filters are concatenated with ``operator&``. When concatenating two
filters, the *outputType* of the first filter must match the *inputType*
of the second filter.


The filters can be constructed and concatenated ahead of time. An
equivalent version of the previous example that does this follows:


::


   void RunPipeline( int ntoken, FILE* input_file, FILE* output_file ) {
       oneapi::tbb::filter<void,TextSlice*> f1( oneapi::tbb::filter_mode::serial_in_order, 
                                          MyInputFunc(input_file) );
       oneapi::tbb::filter<TextSlice*,TextSlice*> f2(oneapi::tbb::filter_mode::parallel, 
                                               MyTransformFunc() );
       oneapi::tbb::filter<TextSlice*,void> f3(oneapi::tbb::filter_mode::serial_in_order, 
                                         MyOutputFunc(output_file) );
       oneapi::tbb::filter<void,void> f = f1 & f2 & f3;
       oneapi::tbb::parallel_pipeline(ntoken,f);
   }


The input filter must be ``serial_in_order`` in this example because the
filter reads chunks from a sequential file and the output filter must
write the chunks in the same order. All ``serial_in_order`` filters
process items in the same order. Thus if an item arrives at
``MyOutputFunc`` out of the order established by ``MyInputFunc``, the
pipeline automatically delays invoking ``MyOutputFunc::operator()`` on
the item until its predecessors are processed. There is another kind of
serial filter, ``serial_out_of_order``, that does not preserve order.


The middle filter operates on purely local data. Thus any number of
invocations of its functor can run concurrently. Hence it is specified
as a parallel filter.


The functors for each filter are explained in detail now. The output
functor is the simplest. All it has to do is write a ``TextSlice`` to a
file and free the ``TextSlice``.


::


   // Functor that writes a TextSlice to a file.
   class MyOutputFunc {
       FILE* my_output_file;
   public:
       MyOutputFunc( FILE* output_file );
       void operator()( TextSlice* item ) const;
   };
    

   MyOutputFunc::MyOutputFunc( FILE* output_file ) :
       my_output_file(output_file)
   {
   }
    

   void MyOutputFunc::operator()( TextSlice* out ) const {
       size_t n = fwrite( out->begin(), 1, out->size(), my_output_file );
       if( n!=out->size() ) {
           fprintf(stderr,"Can't write into file '%s'\n", OutputFileName);
           exit(1);
       }
       out->free();
   } 


Method ``operator()`` processes a ``TextSlice``. The parameter ``out``
points to the ``TextSlice`` to be processed. Since it is used for the
last filter of the pipeline, it returns ``void``.


The functor for the middle filter is similar, but a bit more complex. It
returns a pointer to the ``TextSlice`` that it produces.


::


   // Functor that changes each decimal number to its square.
   class MyTransformFunc {
   public:
       TextSlice* operator()( TextSlice* input ) const;
   };


   TextSlice* MyTransformFunc::operator()( TextSlice* input ) const {
       // Add terminating null so that strtol works right even if number is at end of the input.
       *input->end() = '\0';
       char* p = input->begin();
       TextSlice* out = TextSlice::allocate( 2*MAX_CHAR_PER_INPUT_SLICE );
       char* q = out->begin();
       for(;;) {
           while( p<input->end() && !isdigit(*p) )
               *q++ = *p++;
           if( p==input->end() )
               break;
           long x = strtol( p, &p, 10 );
           // Note: no overflow checking is needed here, as we have twice the
           // input string length, but the square of a non-negative integer n
           // cannot have more than twice as many digits as n.
           long y = x*x;
           sprintf(q,"%ld",y);
           q = strchr(q,0);
       }
       out->set_end(q);
       input->free();
       return out;
   } 


The input functor is the most complicated, because it has to ensure that
no numeral crosses a boundary. When it finds what could be a numeral
crossing into the next slice, it copies the partial numeral to the next
slice. Furthermore, it has to indicate when the end of input is reached.
It does this by invoking method ``stop()`` on a special argument of type
``flow_control``. This idiom is required for any functor used for the
first filter of a pipeline.

::


   TextSlice* next_slice = NULL;


   class MyInputFunc {
   public:
       MyInputFunc( FILE* input_file_ );
       MyInputFunc( const MyInputFunc& f ) : input_file(f.input_file) { }
       ~MyInputFunc();
       TextSlice* operator()( oneapi::tbb::flow_control& fc ) const;
   private:
       FILE* input_file;
   };
    

   MyInputFunc::MyInputFunc( FILE* input_file_ ) :
       input_file(input_file_) { }
    

   MyInputFunc::~MyInputFunc() {
   }
    

   TextSlice* MyInputFunc::operator()( oneapi::tbb::flow_control& fc ) const {
       // Read characters into space that is available in the next slice.
       if( !next_slice )
           next_slice = TextSlice::allocate( MAX_CHAR_PER_INPUT_SLICE );
       size_t m = next_slice->avail();
       size_t n = fread( next_slice->end(), 1, m, input_file );
       if( !n && next_slice->size()==0 ) {
           // No more characters to process
           fc.stop();
           return NULL;
       } else {
           // Have more characters to process.
           TextSlice* t = next_slice;
           next_slice = TextSlice::allocate( MAX_CHAR_PER_INPUT_SLICE );
           char* p = t->end()+n;
           if( n==m ) {
               // Might have read partial number.  
               // If so, transfer characters of partial number to next slice.
               while( p>t->begin() && isdigit(p[-1]) )
                   --p;
               assert(p>t->begin(),"Number too large to fit in buffer.\n");
               next_slice->append( p, t->end()+n );
           }
           t->set_end(p);
           return t;
       }
   }


The copy constructor must be defined because the functor is copied when
the underlying ``oneapi::tbb::filter_t`` is built from the functor, and again when the pipeline runs.


.. |image0| image:: Images/image010.jpg
   :width: 31px
   :height: 26px
.. |image1| image:: Images/image010.jpg
   :width: 31px
   :height: 26px

.. toctree::
   :maxdepth: 4

   ../tbb_userguide/Using_Circular_Buffers
   ../tbb_userguide/Throughput_of_pipeline
   ../tbb_userguide/Non-Linear_Pipelines
