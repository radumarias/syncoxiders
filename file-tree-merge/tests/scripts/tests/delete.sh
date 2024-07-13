#!/bin/bash

set -e

export setup_base_path=/tmp/syncoxiders_test
export setup_num_paths=2
export setup_num_initial_files=0

BIN="$(dirname $0)/../../../../target/release/syncoxiders"

pwd
file ../common.sh
file ../setup.sh

source ../common.sh
source ../setup.sh

size=$(generate_random_size 1 100)
head -c $size </dev/urandom > $setup_base_path/path1/file1

# Run sync
$BIN --repo $setup_base_path/repo $setup_base_path/path1 $setup_base_path/path2

# Delete a file in path1
rm $setup_base_path/path1/file1

# Run sync
$BIN --repo $setup_base_path/repo $setup_base_path/path1 $setup_base_path/path2

# Verify deletion in path2
if [ -f $setup_base_path/path2/file1 ]; then
  echo "file1 deletion sync failed"
  exit 1
fi

source ../cleanup.sh