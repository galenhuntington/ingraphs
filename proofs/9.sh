#!/usr/bin/env bash
graphy ingraph-scan 9 <(graphy canon 9 <(geng -q 9 8:9)) | grep None
# ~1s -> 4 graphs
geng -q 9 > output/all9.txt && graphy misses 9 101752
# -> only one counterexample for 9X
graphy is-subgraph 101752 / 4135458444 "$(graphy complement 9 4135458444)"
# = false false
