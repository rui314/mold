# How to build oneTBB documentation

Our documentation is written in restructured text markup (.rst) and built using [Sphinx](http://www.sphinx-doc.org/en/master/). 

This document explains how to build oneTBB documentation locally. 

## Prerequisites
- Python 3.7.0 or higher
- Sphinx 

## Build documentation

Do the following to generate HTML output of the documentation: 

1. Clone oneTBB repository:

```
git clone https://github.com/oneapi-src/oneTBB.git
```

2. Go to the `doc` folder:

```
cd oneTBB/doc
```

3. Run in the command line:

```
make html
```


That's it! Your built documentation is located in the ``build/html`` folder. 
