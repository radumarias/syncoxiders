#!/bin/bash

set -e

# Trap ERR signal and call error_handler function
trap 'error_handler $LINENO' ERR

rm -rf "$setup_base_path"

# repo
mkdir -p "$setup_base_path/repo"

# paths
if [ "$setup_num_paths" -gt 0 ]; then
  for (( i=1; i<=setup_num_paths; i++ )); do
    rm -rf "$setup_base_path/path$i"
    mkdir -p "$setup_base_path/path$i"
  done
fi

# Create a function to generate files with random content
generate_files() {
  if [ "$setup_num_initial_files" -gt 0 ]; then
    for i in $(seq 1 $setup_num_initial_files); do
      size=$(generate_random_size 1 100)
      head -c $size </dev/urandom > $setup_base_path/path1/file
    done
  fi
}

# Generate initial files in path1
generate_files
