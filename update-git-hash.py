#! /usr/bin/env python3
#
# This script updates git-hash.cc so that the file contains the
# current git hash.

import os
import re
import sys

def get_git_hash(repo):
  try:
    with open(repo + '/.git/HEAD', 'r') as file:
      data = file.readline().strip()
      if data.startswith('ref: '):
        branch = data[5:]
        with open(repo + '/.git/' + branch, 'r') as file:
          return file.readline().strip()
      else:
        return data
  except FileNotFoundError:
    return ""

def read_prev_git_hash(path):
  try:
    with open(path, 'r') as file:
      m = re.search('mold_git_hash = "(.+?)"', file.read())
      if (m):
        return m.group(1)
      return ""
  except FileNotFoundError:
    return ""

if len(sys.argv) != 3:
  print(f'Usage: {sys.argv[0]} <repository-dir> <output-file>',
        file=sys.stderr)
  exit(1)

repo = sys.argv[1]
output_file = sys.argv[2]
cur = get_git_hash(repo)
prev = read_prev_git_hash(output_file)

if (not os.path.exists(output_file)) or cur != prev:
  with open(output_file, 'w') as file:
    file.write("#include <string>\n"
               "namespace mold {\n"
               f"std::string mold_git_hash = \"{cur}\";\n"
               "}\n")
