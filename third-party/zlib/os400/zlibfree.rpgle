**free
//  ZLIB.INC - Interface to the general purpose compression library

//  ILE RPG400 version by Patrick Monnerat, DATASPHERE.
//  Version 1.3.2


//  WARNING:
//     Procedures inflateInit(), inflateInit2(), deflateInit(),
//         deflateInit2() and inflateBackInit() need to be called with
//         two additional arguments:
//         the package version string and the stream control structure.
//         size. This is needed because RPG lacks some macro feature.
//         Call these procedures as:
//             inflateInit(...: ZLIB_VERSION: %size(z_stream))

/if not defined(ZLIB_H_)
/define ZLIB_H_

//*************************************************************************
//                               Constants
//*************************************************************************

//  Versioning information.

Dcl-C ZLIB_VERSION '1.3.2';
Dcl-C ZLIB_VERNUM X'1320';
Dcl-C ZLIB_VER_MAJOR 1;
Dcl-C ZLIB_VER_MINOR 3;
Dcl-C ZLIB_VER_REVISION 2;
Dcl-C ZLIB_VER_SUBREVISION 0;

//  Other equates.

Dcl-C Z_NO_FLUSH 0;
Dcl-C Z_PARTIAL_FLUSH 1;
Dcl-C Z_SYNC_FLUSH 2;
Dcl-C Z_FULL_FLUSH 3;
Dcl-C Z_FINISH 4;
Dcl-C Z_BLOCK 5;
Dcl-C Z_TREES 6;

Dcl-C Z_OK 0;
Dcl-C Z_STREAM_END 1;
Dcl-C Z_NEED_DICT 2;
Dcl-C Z_ERRNO -1;
Dcl-C Z_STREAM_ERROR -2;
Dcl-C Z_DATA_ERROR -3;
Dcl-C Z_MEM_ERROR -4;
Dcl-C Z_BUF_ERROR -5;
Dcl-C Z_VERSION_ERROR -6;

Dcl-C Z_NO_COMPRESSION 0;
Dcl-C Z_BEST_SPEED 1;
Dcl-C Z_BEST_COMPRESSION 9;
Dcl-C Z_DEFAULT_COMPRESSION -1;

Dcl-C Z_FILTERED 1;
Dcl-C Z_HUFFMAN_ONLY 2;
Dcl-C Z_RLE 3;
Dcl-C Z_DEFAULT_STRATEGY 0;

Dcl-C Z_BINARY 0;
Dcl-C Z_ASCII 1;
Dcl-C Z_UNKNOWN 2;

Dcl-C Z_DEFLATED 8;

Dcl-C Z_NULL 0;

//*************************************************************************
//                                 Types
//*************************************************************************

Dcl-S z_streamp Pointer; // Stream struct ptr
Dcl-S gzFile Pointer; // File pointer
Dcl-S gz_headerp Pointer;
Dcl-S z_off_t Int(10); // Stream offsets
Dcl-S z_off64_t Int(20); // Stream offsets

//*************************************************************************
//                               Structures
//*************************************************************************

//  The GZIP encode/decode stream support structure.

Dcl-Ds z_stream Align Based(z_streamp);
    zs_next_in Pointer; // Next input byte
    zs_avail_in Uns(10); // Byte cnt at next_in
    zs_total_in Uns(10); // Total bytes read
    zs_next_out Pointer; // Output buffer ptr
    zs_avail_out Uns(10); // Room left @ next_out
    zs_total_out Uns(10); // Total bytes written
    zs_msg Pointer; // Last errmsg or null
    zs_state Pointer; // Internal state
    zs_zalloc Pointer(*PROC); // Int. state allocator
    zs_free Pointer(*PROC); // Int. state dealloc.
    zs_opaque Pointer; // Private alloc. data
    zs_data_type Int(10); // ASC/BIN best guess
    zs_adler Uns(10); // Uncompr. adler32 val
    *N Uns(10); // Reserved
    *N Uns(10); // Ptr. alignment
End-Ds;

//*************************************************************************
//                     Utility function prototypes
//*************************************************************************

Dcl-Pr compress Int(10) Extproc('compress');
    dest Char(65535) Options(*VARSIZE); // Destination buffer
    destLen Uns(10); // Destination length
    source Char(65535) Const Options(*VARSIZE); // Source buffer
    sourceLen Uns(10) Value; // Source length
End-Pr;

Dcl-Pr compress_z Int(10) Extproc('compress_z');
    dest Char(65535) Options(*VARSIZE); // Destination buffer
    destLen Uns(20); // Destination length
    source Char(65535) Const Options(*VARSIZE); // Source buffer
    sourceLen Uns(20) Value; // Source length
End-Pr;

Dcl-Pr compress2 Int(10) Extproc('compress2');
    dest Char(65535) Options(*VARSIZE); // Destination buffer
    destLen Uns(10); // Destination length
    source Char(65535) Const Options(*VARSIZE); // Source buffer
    sourceLen Uns(10) Value; // Source length
    level Int(10) Value; // Compression level
End-Pr;

Dcl-Pr compress2_z Int(10) Extproc('compress2_z');
    dest Char(65535) Options(*VARSIZE); // Destination buffer
    destLen Uns(20); // Destination length
    source Char(65535) Const Options(*VARSIZE); // Source buffer
    sourceLen Uns(20) Value; // Source length
    level Int(10) Value; // Compression level
End-Pr;

Dcl-Pr compressBound Uns(10) Extproc('compressBound');
    sourceLen Uns(10) Value;
End-Pr;

Dcl-Pr compressBound_z Uns(10) Extproc('compressBound_z');
    sourceLen Uns(20) Value;
End-Pr;

Dcl-Pr uncompress Int(10) Extproc('uncompress');
    dest Char(65535) Options(*VARSIZE); // Destination buffer
    destLen Uns(10); // Destination length
    source Char(65535) Const Options(*VARSIZE); // Source buffer
    sourceLen Uns(10) Value; // Source length
End-Pr;

Dcl-Pr uncompress_z Int(10) Extproc('uncompress_z');
    dest Char(65535) Options(*VARSIZE); // Destination buffer
    destLen Uns(20); // Destination length
    source Char(65535) Const Options(*VARSIZE); // Source buffer
    sourceLen Uns(20) Value; // Source length
End-Pr;

Dcl-Pr uncompress2 Int(10) Extproc('uncompress2');
    dest Char(65535) Options(*VARSIZE); // Destination buffer
    destLen Uns(10); // Destination length
    source Char(65535) Const Options(*VARSIZE); // Source buffer
    sourceLen Uns(10); // Source length
End-Pr;

Dcl-Pr uncompress2_z Int(10) Extproc('uncompress2_z');
    dest Char(65535) Options(*VARSIZE); // Destination buffer
    destLen Uns(20); // Destination length
    source Char(65535) Const Options(*VARSIZE); // Source buffer
    sourceLen Uns(20); // Source length
End-Pr;

/if not defined(LARGE_FILES)
    Dcl-Pr gzopen Extproc('gzopen') Like(gzFile);
            path Pointer Value Options(*STRING); // File pathname
            mode Pointer Value Options(*STRING); // Open mode
    End-Pr;
/else
    Dcl-Pr gzopen Extproc('gzopen64') Like(gzFile);
            path Pointer Value Options(*STRING); // File pathname
            mode Pointer Value Options(*STRING); // Open mode
        End-Pr;

    Dcl-Pr gzopen64 Extproc('gzopen64') Like(gzFile);
            path Pointer Value Options(*STRING); // File pathname
            mode Pointer Value Options(*STRING); // Open mode
    End-Pr;
/endif

Dcl-Pr gzdopen Extproc('gzdopen') Like(gzFile);
    fd Int(10) Value; // File descriptor
    mode Pointer Value Options(*STRING); // Open mode
End-Pr;

Dcl-Pr gzbuffer Int(10) Extproc('gzbuffer');
    file Value Like(gzFile); // File pointer
    size Uns(10) Value;
End-Pr;

Dcl-Pr gzsetparams Int(10) Extproc('gzsetparams');
    file Value Like(gzFile); // File pointer
    level Int(10) Value;
    strategy Int(10) Value;
End-Pr;

Dcl-Pr gzread Int(10) Extproc('gzread');
    file Value Like(gzFile); // File pointer
    buf Char(65535) Options(*VARSIZE); // Buffer
    len Uns(10) Value; // Buffer length
End-Pr;

Dcl-Pr gzfread Int(20) Extproc('gzfread');
    buf Char(65535) Options(*VARSIZE); // Buffer
    size Uns(20) Value; // Buffer length
    nitems Uns(20) Value; // Buffer length
    file Value Like(gzFile); // File pointer
End-Pr;

Dcl-Pr gzwrite Int(10) Extproc('gzwrite');
    file Value Like(gzFile); // File pointer
    buf Char(65535) Const Options(*VARSIZE); // Buffer
    len Uns(10) Value; // Buffer length
End-Pr;

Dcl-Pr gzfwrite Int(20) Extproc('gzfwrite');
    buf Char(65535) Options(*VARSIZE); // Buffer
    size Uns(20) Value; // Buffer length
    nitems Uns(20) Value; // Buffer length
    file Value Like(gzFile); // File pointer
End-Pr;

Dcl-Pr gzputs Int(10) Extproc('gzputs');
    file Value Like(gzFile); // File pointer
    s Pointer Value Options(*STRING); // String to output
End-Pr;

Dcl-Pr gzgets Pointer Extproc('gzgets');
    file Value Like(gzFile); // File pointer
    buf Char(65535) Options(*VARSIZE); // Read buffer
    len Int(10) Value; // Buffer length
End-Pr;

Dcl-Pr gzputc Int(10) Extproc('gzputc');
    file Value Like(gzFile); // File pointer
    c Int(10) Value; // Character to write
End-Pr;

Dcl-Pr gzgetc Int(10) Extproc('gzgetc');
    file Value Like(gzFile); // File pointer
End-Pr;

Dcl-Pr gzgetc_ Int(10) Extproc('gzgetc_');
    file Value Like(gzFile); // File pointer
End-Pr;

Dcl-Pr gzungetc Int(10) Extproc('gzungetc');
    c Int(10) Value; // Character to push
    file Value Like(gzFile); // File pointer
End-Pr;

Dcl-Pr gzflush Int(10) Extproc('gzflush');
    file Value Like(gzFile); // File pointer
    flush Int(10) Value; // Type of flush
End-Pr;

/if not defined(LARGE_FILES)
    Dcl-Pr gzseek Extproc('gzseek') Like(z_off_t);
        file Value Like(gzFile); // File pointer
        offset Value Like(z_off_t); // Offset
        whence Int(10) Value; // Origin
    End-Pr;
/else
    Dcl-Pr gzseek Extproc('gzseek64') Like(z_off_t);
        file Value Like(gzFile); // File pointer
        offset Value Like(z_off_t); // Offset
        whence Int(10) Value; // Origin
    End-Pr;

    Dcl-Pr gzseek64 Extproc('gzseek64') Like(z_off64_t);
        file Value Like(gzFile); // File pointer
        offset Value Like(z_off64_t); // Offset
        whence Int(10) Value; // Origin
    End-Pr;
/endif

Dcl-Pr gzrewind Int(10) Extproc('gzrewind');
    file Value Like(gzFile); // File pointer
End-Pr;

/if not defined(LARGE_FILES)
    Dcl-Pr gztell Extproc('gztell') Like(z_off_t);
        file Value Like(gzFile); // File pointer
    End-Pr;
/else
    Dcl-Pr gztell Extproc('gztell64') Like(z_off_t);
        file Value Like(gzFile); // File pointer
    End-Pr;

    Dcl-Pr gztell64 Extproc('gztell64') Like(z_off64_t);
        file Value Like(gzFile); // File pointer
    End-Pr;
/endif

/if not defined(LARGE_FILES)
    Dcl-Pr gzoffset Extproc('gzoffset') Like(z_off_t);
        file Value Like(gzFile); // File pointer
    End-Pr;
/else
    Dcl-Pr gzoffset Extproc('gzoffset64') Like(z_off_t);
        file Value Like(gzFile); // File pointer
    End-Pr;

    Dcl-Pr gzoffset64 Extproc('gzoffset64') Like(z_off64_t);
        file Value Like(gzFile); // File pointer
    End-Pr;
/endif

Dcl-Pr gzeof Int(10) Extproc('gzeof');
    file Value Like(gzFile); // File pointer
End-Pr;

Dcl-Pr gzdirect Int(10) Extproc('gzdirect');
    file Value Like(gzFile); // File pointer
End-Pr;

Dcl-Pr gzclose_r Int(10) Extproc('gzclose_r');
    file Value Like(gzFile); // File pointer
End-Pr;

Dcl-Pr gzclose_w Int(10) Extproc('gzclose_w');
    file Value Like(gzFile); // File pointer
End-Pr;

Dcl-Pr gzclose Int(10) Extproc('gzclose');
    file Value Like(gzFile); // File pointer
End-Pr;

Dcl-Pr gzerror Pointer Extproc('gzerror'); // Error string
    file Value Like(gzFile); // File pointer
    errnum Int(10); // Error code
End-Pr;

Dcl-Pr gzclearerr Extproc('gzclearerr');
    file Value Like(gzFile); // File pointer
End-Pr;

//*************************************************************************
//                        Basic function prototypes
//*************************************************************************

Dcl-Pr zlibVersion Pointer Extproc('zlibVersion'); // Version string
End-Pr;

Dcl-Pr deflateInit Int(10) Extproc('deflateInit_'); // Init. compression
    strm Like(z_stream); // Compression stream
    level Int(10) Value; // Compression level
    version Pointer Value Options(*STRING); // Version string
    stream_size Int(10) Value; // Stream struct. size
End-Pr;

Dcl-Pr deflate Int(10) Extproc('deflate'); // Compress data
    strm Like(z_stream); // Compression stream
    flush Int(10) Value; // Flush type required
End-Pr;

Dcl-Pr deflateEnd Int(10) Extproc('deflateEnd'); // Termin. compression
    strm Like(z_stream); // Compression stream
End-Pr;

Dcl-Pr inflateInit Int(10) Extproc('inflateInit_'); // Init. expansion
    strm Like(z_stream); // Expansion stream
    version Pointer Value Options(*STRING); // Version string
    stream_size Int(10) Value; // Stream struct. size
End-Pr;

Dcl-Pr inflate Int(10) Extproc('inflate'); // Expand data
    strm Like(z_stream); // Expansion stream
    flush Int(10) Value; // Flush type required
End-Pr;

Dcl-Pr inflateEnd Int(10) Extproc('inflateEnd'); // Termin. expansion
    strm Like(z_stream); // Expansion stream
End-Pr;

//*************************************************************************
//                        Advanced function prototypes
//*************************************************************************

Dcl-Pr deflateInit2 Int(10) Extproc('deflateInit2_'); // Init. compression
    strm Like(z_stream); // Compression stream
    level Int(10) Value; // Compression level
    method Int(10) Value; // Compression method
    windowBits Int(10) Value; // log2(window size)
    memLevel Int(10) Value; // Mem/cmpress tradeoff
    strategy Int(10) Value; // Compression strategy
    version Pointer Value Options(*STRING); // Version string
    stream_size Int(10) Value; // Stream struct. size
End-Pr;

Dcl-Pr deflateSetDictionary Int(10) Extproc('deflateSetDictionary'); // Init. dictionary
    strm Like(z_stream); // Compression stream
    dictionary Char(65535) Const Options(*VARSIZE); // Dictionary bytes
    dictLength Uns(10) Value; // Dictionary length
End-Pr;

Dcl-Pr deflateCopy Int(10) Extproc('deflateCopy'); // Compress strm 2 strm
    dest Like(z_stream); // Destination stream
    source Like(z_stream); // Source stream
End-Pr;

Dcl-Pr deflateReset Int(10) Extproc('deflateReset'); // End and init. stream
    strm Like(z_stream); // Compression stream
End-Pr;

Dcl-Pr deflateParams Int(10) Extproc('deflateParams'); // Change level & strat
    strm Like(z_stream); // Compression stream
    level Int(10) Value; // Compression level
    strategy Int(10) Value; // Compression strategy
End-Pr;

Dcl-Pr deflateTune Int(10) Extproc('deflateTune');
    strm Like(z_stream); // Compression stream
    good Int(10) Value;
    lazy Int(10) Value;
    nice Int(10) Value;
    chain_ Int(10) Value;
End-Pr;

Dcl-Pr deflateBound Uns(10) Extproc('deflateBound'); // Change level & strat
    strm Like(z_stream); // Compression stream
    sourcelen Uns(10) Value; // Source length
End-Pr;

Dcl-Pr deflateBound_z Uns(10) Extproc('deflateBound_z'); // Change level & strat
    strm Like(z_stream); // Compression stream
    sourcelen Uns(20) Value; // Source length
End-Pr;

Dcl-Pr deflatePending Int(10) Extproc('deflatePending'); // Change level & strat
    strm Like(z_stream); // Compression stream
    pending Uns(10); // Pending bytes
    bits Int(10); // Pending bits
End-Pr;

Dcl-Pr deflateUsed Int(10) Extproc('deflateUsed'); // Get used bits
    strm Like(z_stream); // Compression stream
    bits Int(10); // Used bits
End-Pr;

Dcl-Pr deflatePrime Int(10) Extproc('deflatePrime'); // Change level & strat
    strm Like(z_stream); // Compression stream
    bits Int(10) Value; // # of bits to insert
    value Int(10) Value; // Bits to insert
End-Pr;

Dcl-Pr inflateInit2 Int(10) Extproc('inflateInit2_'); // Init. expansion
    strm Like(z_stream); // Expansion stream
    windowBits Int(10) Value; // log2(window size)
    version Pointer Value Options(*STRING); // Version string
    stream_size Int(10) Value; // Stream struct. size
End-Pr;

Dcl-Pr inflateSetDictionary Int(10) Extproc('inflateSetDictionary'); // Init. dictionary
    strm Like(z_stream); // Expansion stream
    dictionary Char(65535) Const Options(*VARSIZE); // Dictionary bytes
    dictLength Uns(10) Value; // Dictionary length
End-Pr;

Dcl-Pr inflateGetDictionary Int(10) Extproc('inflateGetDictionary'); // Get dictionary
    strm Like(z_stream); // Expansion stream
    dictionary Char(65535) Options(*VARSIZE); // Dictionary bytes
    dictLength Uns(10); // Dictionary length
End-Pr;

Dcl-Pr deflateGetDictionary Int(10) Extproc('deflateGetDictionary'); // Get dictionary
    strm Like(z_stream); // Expansion stream
    dictionary Char(65535) Options(*VARSIZE); // Dictionary bytes
    dictLength Uns(10); // Dictionary length
End-Pr;

Dcl-Pr inflateSync Int(10) Extproc('inflateSync'); // Sync. expansion
    strm Like(z_stream); // Expansion stream
End-Pr;

Dcl-Pr inflateCopy Int(10) Extproc('inflateCopy');
    dest Like(z_stream); // Destination stream
    source Like(z_stream); // Source stream
End-Pr;

Dcl-Pr inflateReset Int(10) Extproc('inflateReset'); // End and init. stream
    strm Like(z_stream); // Expansion stream
End-Pr;

Dcl-Pr inflateReset2 Int(10) Extproc('inflateReset2'); // End and init. stream
    strm Like(z_stream); // Expansion stream
    windowBits Int(10) Value; // Log2(buffer size)
End-Pr;

Dcl-Pr inflatePrime Int(10) Extproc('inflatePrime'); // Insert bits
    strm Like(z_stream); // Expansion stream
    bits Int(10) Value; // Bit count
    value Int(10) Value; // Bits to insert
End-Pr;

Dcl-Pr inflateMark Int(10) Extproc('inflateMark'); // Get inflate info
    strm Like(z_stream); // Expansion stream
End-Pr;

Dcl-Pr inflateCodesUsed Uns(20) Extproc('inflateCodesUsed');
    strm Like(z_stream); // Expansion stream
End-Pr;

Dcl-Pr inflateValidate Uns(20) Extproc('inflateValidate');
    strm Like(z_stream); // Expansion stream
    check Int(10) Value;
End-Pr;

Dcl-Pr inflateGetHeader Uns(10) Extproc('inflateGetHeader');
    strm Like(z_stream); // Expansion stream
    head Like(GZ_HEADERP);
End-Pr;

Dcl-Pr deflateSetHeader Uns(10) Extproc('deflateSetHeader');
    strm Like(z_stream); // Expansion stream
    head Like(GZ_HEADERP);
End-Pr;

Dcl-Pr inflateBackInit Int(10) Extproc('inflateBackInit_');
    strm Like(z_stream); // Expansion stream
    windowBits Int(10) Value; // Log2(buffer size)
    window Char(65535) Options(*VARSIZE); // Buffer
    version Pointer Value Options(*STRING); // Version string
    stream_size Int(10) Value; // Stream struct. size
End-Pr;

Dcl-Pr inflateBack Int(10) Extproc('inflateBack');
    strm Like(z_stream); // Expansion stream
    in_ Pointer(*PROC) Value; // Input function
    in_desc Pointer Value; // Input descriptor
    out_ Pointer(*PROC) Value; // Output function
    out_desc Pointer Value; // Output descriptor
End-Pr;

Dcl-Pr inflateBackEnd Int(10) Extproc('inflateBackend');
    strm Like(z_stream); // Expansion stream
End-Pr;

Dcl-Pr zlibCompileFlags Uns(10) Extproc('zlibCompileFlags') End-Pr;

//*************************************************************************
//                        Checksum function prototypes
//*************************************************************************

Dcl-Pr adler32 Uns(10) Extproc('adler32'); // New checksum
    adler Uns(10) Value; // Old checksum
    buf Char(65535) Const Options(*VARSIZE); // Bytes to accumulate
    len Uns(10) Value; // Buffer length
End-Pr;

Dcl-Pr adler32_combine Uns(10) Extproc('adler32_combine'); // New checksum
    adler1 Uns(10) Value; // Old checksum
    adler2 Uns(10) Value; // Old checksum
    len2 Uns(20) Value; // Buffer length
End-Pr;

Dcl-Pr adler32_z Uns(10) Extproc('adler32_z'); // New checksum
    adler Uns(10) Value; // Old checksum
    buf Char(65535) Const Options(*VARSIZE); // Bytes to accumulate
    len Uns(20) Value; // Buffer length
End-Pr;

Dcl-Pr crc32 Uns(10) Extproc('crc32'); // New checksum
    crc Uns(10) Value; // Old checksum
    buf Char(65535) Const Options(*VARSIZE); // Bytes to accumulate
    len Uns(10) Value; // Buffer length
End-Pr;

Dcl-Pr crc32_combine Uns(10) Extproc('crc32_combine'); // New checksum
    crc1 Uns(10) Value; // Old checksum
    crc2 Uns(10) Value; // Old checksum
    len2 Uns(20) Value; // 2nd Buffer length
End-Pr;

Dcl-Pr crc32_z Uns(10) Extproc('crc32_z'); // New checksum
    crc Uns(10) Value; // Old checksum
    buf Char(65535) Const Options(*VARSIZE); // Bytes to accumulate
    len Uns(20) Value; // Buffer length
End-Pr;

Dcl-Pr crc32_combine_gen Uns(10) Extproc('crc32_combine_gen');
    len Uns(20) Value; // 2nd Buffer length
End-Pr;

Dcl-Pr crc32_combine_gen64 Uns(10) Extproc('crc32_combine_gen64');
    len Uns(20) Value; // 2nd Buffer length
End-Pr;

Dcl-Pr crc32_combine_op Uns(10) Extproc('crc32_combine_op'); // New checksum
    crc1 Uns(10) Value; // Old checksum
    crc2 Uns(10) Value; // Old checksum
    op Uns(10) Value; // Operator
End-Pr;

//*************************************************************************
//                     Miscellaneous function prototypes
//*************************************************************************

Dcl-Pr zError Pointer Extproc('zError'); // Error string
    err Int(10) Value; // Error code
End-Pr;

Dcl-Pr inflateSyncPoint Int(10) Extproc('inflateSyncPoint');
    strm Like(z_stream); // Expansion stream
End-Pr;

Dcl-Pr get_crc_table Pointer Extproc('get_crc_table'); // Ptr to ulongs
End-Pr;

Dcl-Pr inflateUndermine Int(10) Extproc('inflateUndermine');
    strm Like(z_stream); // Expansion stream
    arg Int(10) Value; // Error code
End-Pr;

Dcl-Pr inflateResetKeep Int(10) Extproc('inflateResetKeep'); // End and init. stream
    strm Like(z_stream); // Expansion stream
End-Pr;

Dcl-Pr deflateResetKeep Int(10) Extproc('deflateResetKeep'); // End and init. stream
    strm Like(z_stream); // Expansion stream
End-Pr;

/endif
