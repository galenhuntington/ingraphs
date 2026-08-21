#!/usr/bin/env bash

#  Generate Markdown table of properties of n=14 candidates.

PATH=./target/release:$PATH

echo '| Graph | Vertices | Planar | \|&zwj;Sym&zwj;\| | ⊃ 2nd 10-DUI | ⊃ 11-DUI | ⊃ 12-DUI | ⊃ 2nd 13-DUI |'
echo '| ---: | ---: | :-: | -: | :-: | :-: | :-: | :-: |'

cell() {
   FLAG=$1
   MARK=${2:-✅}
   [ -n "$FLAG" ] && [ "$FLAG" != ' false' ] && echo -n " $MARK"
   echo -n ' |'
}

subgcell() {
   SUB=$1
   SUP=$2
   MARK=$3
   cell "$(graphy is-subgraph "$SUB" / "$SUP")" "$MARK"
}

for g in $(< output/batches/all14); do
   SPEC=$(if [ "$g" = 105760086734720 ]; then echo -n '*'; fi)
   echo -n "| $SPEC$g | "
   if (( g < 35184372088832 )); then echo -n 10; else echo -n 11; fi
   echo -n ' |'
   cell "$(graphy matrix 11 <(echo "$g") | amtog -q | planarg -q)"
   echo -n " $(graphy info 11 "$g" | sed 's/.*syms://;s/ .*//') |"
   # subgcell 36216 "$g"
   # subgcell 2202040 "$g"
   subgcell 6395248 "$g"
   subgcell 6732736 "$g" 💎
   subgcell 816167872 "$g"
   subgcell 207515663232 "$g"
   echo -n ' |'
   echo
done

echo
echo '\*  Does not extend the 2nd 9-DUI or the 1st 10-DUI.'

