# -*- Python -*-

import os
import platform
import re
import subprocess
import locale

from lit.llvm import llvm_config
import lit.llvm
import lit.util

config.name = 'mold'
config.suffixes = ['.s', '.test']
config.test_format = lit.formats.ShTest(False)
config.test_source_root = os.path.dirname(__file__)

config.environment['PATH'] = os.path.pathsep.join((
    os.path.dirname(__file__) + '/..',
    os.path.dirname(__file__) + '/../llvm-project/build/bin',
    config.environment['PATH']))
