#!/bin/bash

set -e

export setup_base_path=/tmp/syncoxiders_test
export setup_num_paths=2
export setup_num_initial_files=0

BIN="$(dirname $0)/../../../../../target/release/syncoxiders"

source ../../common.sh
source ../../setup.sh

# Add new files in path1
for i in {1..5}; do
  size=$(generate_random_size 1 100)
  head -c $size </dev/urandom > $setup_base_path/path1/file$i
  touch $setup_base_path/path1/file$i
done

# Run sync
$BIN --dry-run --repo $setup_base_path/repo $setup_base_path/path1 $setup_base_path/path2

# Verify no files in path2
for i in {1..5}; do
  if [ -f $setup_base_path/path2/file$i ]; then
    echo "Dry-run for file$i failed"
    exit 1
  fi
done

source ../../cleanup.sh