# SPDX-FileCopyrightText: 2019-2020 Intel Corporation
#
# SPDX-License-Identifier: MIT

# -*- coding: utf-8 -*-
#
# Configuration file for the Sphinx documentation builder.
#
# This file does only contain a selection of the most common options. For a
# full list see the documentation:
# http://www.sphinx-doc.org/en/master/config

# -- Path setup --------------------------------------------------------------

# If extensions (or modules to document with autodoc) are in another directory,
# add these directories to sys.path here. If the directory is relative to the
# documentation root, use os.path.abspath to make it absolute, like shown here.
#
import os
import sys
from os.path import join


project = 'oneTBB'

repo_root = join('..', '..', '..', '..')
exec(open(join(repo_root, 'source', 'conf', 'common_conf.py')).read())
exec(open(join(repo_root, 'source', 'conf', 'element_conf.py')).read())

exclude_patterns = ['nested-*.rst',
                    'uncategorized.rst',
                    'uncategorized/**',
                    'low_level_task_api.rst',
                    'low_level_tasking/**',
]

latex_documents = [
    ('index', 'oneTBB-spec.tex', u'oneAPI Threading Building Blocks Specification',
     u'Intel', 'manual'),
]
