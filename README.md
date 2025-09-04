# Architecture of fdu

1. Walker object that uses rayon + io_uring to parallelize filesystem walking and spits out
in a channel the metadata of each file with flags for entering/exiting a directory.
2. Processor object handles different output handlers to use the received data from walker
while it reconstructs directories using dashmap
