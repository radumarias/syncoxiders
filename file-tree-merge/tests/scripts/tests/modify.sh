#!/bin/bash

set -e

export setup_base_path=/tmp/syncoxiders_test
export setup_num_paths=2
export setup_num_initial_files=0

BIN="$(dirname $0)/../../../../target/release/syncoxiders"

source ../common.sh
source ../setup.sh

size=$(generate_random_size 1 100)
head -c $size </dev/urandom > $setup_base_path/path1/file1
cp $setup_base_path/path1/file1 $setup_base_path/path2/file1

# Modify a file in path1
head -c $size </dev/urandom > $setup_base_path/path1/file1
touch $setup_base_path/path1/file1

# Run sync
$BIN --repo $setup_base_path/repo $setup_base_path/path1 $setup_base_path/path2

# Verify modified file in path2
if ! cmp -s $setup_base_path/path1/file1 $setup_base_path/path2/file1; then
  echo -e "${Red}file1 modify sync failed"
  exit 1
fi

source ../cleanup.sh