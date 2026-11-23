#! /usr/bin/env bash

SRC="$1"

while read -r i; do
   echo -n "$i $(date -uIns) "
   timeout 60 ./target/release/graphy-u256 cull 19 "$SRC" <(echo "$i") 2> /dev/null | grep -c Some
done

date -uIns

