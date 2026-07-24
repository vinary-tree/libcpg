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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sorting_algorithm_variants() {
        let all = [
            SortingAlgorithm::Quicksort,
            SortingAlgorithm::Mergesort,
            SortingAlgorithm::Heapsort,
            SortingAlgorithm::InsertionSort,
            SortingAlgorithm::SelectionSort,
            SortingAlgorithm::BubbleSort,
            SortingAlgorithm::RadixSort,
            SortingAlgorithm::CountingSort,
            SortingAlgorithm::BucketSort,
            SortingAlgorithm::Timsort,
            SortingAlgorithm::Unknown,
        ];
        // Copy + equality + Debug hold; each variant is distinct from the rest.
        for (i, a) in all.iter().enumerate() {
            let copied = *a;
            assert_eq!(copied, *a);
            assert!(!format!("{a:?}").is_empty());
            for (j, b) in all.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }
}
