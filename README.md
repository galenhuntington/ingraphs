This is code for my article [Dense Universal
Ingraphs](https://galen.xyz/ingraphs/).  It provides several tools
for working on this problem.  It is written in Rust, and is quite
rough, but usable.

## Problem

A universal ingraph for _n_ is a graph G such that for any graph H
on _n_ vertices, G is a subgraph of either H or the complement of H.

For example, a universal ingraph for 6 is the five-vertex graph
looking like ☐_.

The goal is to find universal ingraphs with the most number of edges.

## Code

This code has a smorgasbord of operations aimed towards finding and
analyzing ingraphs.  Some of it could be better documented.

Graphs are represented by decimal numbers (perhaps not the best system
but adequate).

## Some graphs

Here are numeric representations of some graphs mentioned in the
article:

| Graph | Number |
| :--- | ---: |
| 2 DUI | 1 |
| 3 DUI | 3 |
| 5 DUI | 13 |
| 6 DUI | 94 |
| 7 DUI | 1118 |
| 8 DUI | 3448 |
| 9X | 101752 |
| 9 DUI | 36280 |
| 9 DUI | 36216 |
| 9 DUI | 101736 |
| 9 DUI | 101744 |
| 10 DUI | 2202040 |
| 10 DUI | 6395248 |
| 11 DUI | 6732736 |
| 12 maybe | 816167872 |
| 13 guess | 208052598656 |

