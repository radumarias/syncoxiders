#!/bin/bash

set -e

Green="\e[0;32m"
Yellow="\e[0;33m"
Red="\033[0;31m"
NC="\e[0m" # No Color

# Function to handle errors
error_handler() {
    echo "Error on line $1"
}

# Function to generate a random size between given ranges in MB
generate_random_size() {
  local min_mb=$1
  local max_mb=$2
  local min_bytes=$((min_mb * 1048576))
  local max_bytes=$((max_mb * 1048576))
  shuf -i ${min_bytes}-${max_bytes} -n 1
}
