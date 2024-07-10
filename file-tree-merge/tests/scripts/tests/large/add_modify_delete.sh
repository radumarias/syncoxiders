#!/bin/bash

#[test(ignore)]

export setup_base_path=~/tmp/syncoxiders_test
export setup_num_paths=2
export setup_num_initial_files=0

BIN="$(dirname $0)/../../../../../target/release/syncoxiders"

source ../../common.sh
source ../../setup.sh

set -e

echo "Setting up files ..."

# Add large files in path1
for i in {21..25}; do
    size=$(generate_random_size 1000 10000)
    head -c $size </dev/urandom > $setup_base_path/path1/file$i
    touch $setup_base_path/path1/file$i
done

# Change existing files in path1 with large content
for i in {1..5}; do
    size=$(generate_random_size 1000 10000)
    head -c $size </dev/urandom > $setup_base_path/path1/file$i
    touch $setup_base_path/path1/file$i
done

# Delete some files in path1
for i in {11..15}; do
  rm /tmp/syncoxiders_test/path1/file$i
done

# Run sync
$BIN --repo $setup_base_path/repo setup_base_path/path1 $setup_base_path/path2

# Verify new large files in path2
for i in {21..25}; do
  if ! cmp -s $setup_base_path/path1/file$i $setup_base_path/path2/file$i; then
    echo -e "${Red}file$i addition sync failed"
    exit 1
  fi
done

# Verify modified large files in path2
for i in {1..5}; do
  if ! cmp -s $setup_base_path/path1/file$i $setup_base_path/path2/file$i; then
    echo -e "${Red}file$i modification sync failed"
    exit 1
  fi
done

# Verify deleted files in path2
for i in {11..15}; do
  if [ -f $setup_base_path/path2/file$i ]; then
    echo -e "${Red}file$i deletion sync failed"
    exit 1
  fi
done

source ../../cleanup.sh