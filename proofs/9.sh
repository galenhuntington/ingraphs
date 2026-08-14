#!/usr/bin/env bash
graphy ingraph-scan 9 <(graphy canon 9 <(geng -q 9 8:9)) | grep None
# ~1s -> 4 graphs
