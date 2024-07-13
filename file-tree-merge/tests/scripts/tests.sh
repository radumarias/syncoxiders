#!/bin/bash

set -e

Green="\e[0;32m"
Yellow="\e[0;33m"
Red="\033[0;31m"
NC="\e[0m" # No Color

ignore_marker='#[test(ignore)]'

find tests -type f -name "*.sh" | while read -r file; do
  absolute_path=$(realpath "$file")
  basename=$(basename "$file")

  parent_dir=$(dirname "$file")
  # Change to the parent directory
  echo "parent dir $parent_dir"
  cd "$parent_dir" || exit
  
  first_parent=$(basename "$parent_dir")
  
  if grep -Fq "$ignore_marker" "$absolute_path"; then
    echo -e "test $first_parent/$basename ... ${Yellow}ignored${NC}"
    # Change back to the original directory (optional)
    cd - >/dev/null || exit
    continue
  else
    echo -e "test $first_parent/$basename ..."
    sh "$absolute_path"
    # Capture the exit code
    exit_code=$?
    # Check the exit code and handle it
    if [ $exit_code -eq 0 ]; then
        echo -e "${Green}ok${NC}"
    else
        echo -e "${Red}failed${NC}"
        exit $exit_code
    fi
  fi

  # Change back to the original directory (optional)
  cd - >/dev/null || exit

  echo -e ""
done