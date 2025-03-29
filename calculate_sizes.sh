#!/bin/bash

output=$(du -b files 2>/dev/null)

json="["
while read -r size file; do
    json+="{\"size\":$size,\"file\":\"$file\"},"
done <<< "$output"

json=${json%,}
json+="]"

output_file="sizes.json"
echo "$json" > "$output_file"
