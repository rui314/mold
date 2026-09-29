Binary and source releases for mimalloc.

- __v3__: recommended: latest mimalloc design that tends to use less memory then v2.
- __v2__: stable legacy.
- __v1__: legacy.

Notes:

- Release versions follow the v3 version (and v1 and v2 versions are incremented independently).
- Generally it is recommended to download sources (or use `vcpkg` etc.) and build mimalloc as 
  part of your project.
- Source releases can also be downloaded directly from github by the tag.  
  For example <https://github.com/microsoft/mimalloc/archive/v3.5.1.tar.gz>.  
- Binary releases include a release-, debug-, and secure build.
- Linux binaries are built on Ubuntu 22.
