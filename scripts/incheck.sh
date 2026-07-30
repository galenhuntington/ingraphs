#!/bin/bash

# Ingraph checker.

# Example usage:
#  seq 0 999 | xargs -P6 -I{} ./scripts/incheck.sh 13 123456789 {}/1000

SIZE="$1"
CAND="$2"
RES=${3%/*}
MOD=${3#*/}
MID=$(( (SIZE*(SIZE-1)+3)/4 ))
DIR=output/runs$SIZE/$CAND-$MOD
# echo "$SIZE $MID $DIR"

mkdir -p "$DIR"
SEEDS="$DIR/seed-$RES.g6"
LOG="$DIR/log-$RES.log"
: >"$LOG"
{ time GRAPHY_SUB=$CAND ./nauty-prune/gengf -q "$SIZE" $MID:$MID "$RES/$MOD" >"$SEEDS"; } 2>>"$LOG"
./target/release/graphy free-close "$SIZE" "$CAND" "$SEEDS" >"$DIR/found-$RES.csv" 2>>"$LOG"

