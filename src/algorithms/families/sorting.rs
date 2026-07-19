//! Sorting algorithm patterns.

/// Known sorting algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortingAlgorithm {
    /// Quicksort.
    Quicksort,
    /// Merge sort.
    Mergesort,
    /// Heap sort.
    Heapsort,
    /// Insertion sort.
    InsertionSort,
    /// Selection sort.
    SelectionSort,
    /// Bubble sort.
    BubbleSort,
    /// Radix sort.
    RadixSort,
    /// Counting sort.
    CountingSort,
    /// Bucket sort.
    BucketSort,
    /// Tim sort.
    Timsort,
    /// Unknown sorting algorithm.
    Unknown,
}
