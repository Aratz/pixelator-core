//! What one graph-collapse run counted: the nodes it looked at, the matches it
//! kept, and how far each kept match was from its node.
//!
//! Like the edgelist stats, values combine with [`AddAssign`], so per-component
//! tallies fold into a run total.

use std::ops::AddAssign;

/// Tallies accumulated over graph-collapse searches.
///
/// The distance distributions are dense vectors indexed by distance, so the
/// count of matches at distance `d` is at index `d`. A vector is only as long
/// as the largest distance seen, and the accessors skip distances with no match.
#[derive(Clone, Default)]
pub struct CollapseStats {
    /// Components searched. [`graph_collapse`](super::graph_collapse) leaves
    /// this at 0, and the caller adds components with [`add_components`].
    ///
    /// [`add_components`]: Self::add_components
    pub total_components: usize,
    /// Kept matches. A match dropped for closing a loop is not counted.
    pub targets_found: usize,
    /// Nodes whose degree was at most `max_degree`, so were searched.
    pub nodes_considered: usize,
    /// Kept matches by graph hops from the node to its match.
    pub bfs_distance_distribution: Vec<usize>,
    /// Kept matches by Hamming distance between the node's UMI and its match's.
    pub hamming_distance_distribution: Vec<usize>,
}

impl AddAssign for CollapseStats {
    fn add_assign(&mut self, other: Self) {
        self.total_components += other.total_components;
        self.targets_found += other.targets_found;
        self.nodes_considered += other.nodes_considered;
        add_counts(
            &mut self.bfs_distance_distribution,
            other.bfs_distance_distribution,
        );
        add_counts(
            &mut self.hamming_distance_distribution,
            other.hamming_distance_distribution,
        );
    }
}

/// Adds `other` into `counts` index by index, growing `counts` if `other` is longer.
fn add_counts(counts: &mut Vec<usize>, other: Vec<usize>) {
    if counts.len() < other.len() {
        counts.resize(other.len(), 0)
    }
    for (i, n) in other.into_iter().enumerate() {
        counts[i] += n;
    }
}

impl CollapseStats {
    /// Records one kept match, `bfs_dist` hops away and `hamming_dist` symbols apart.
    pub fn add_target_stats(&mut self, bfs_dist: usize, hamming_dist: usize) {
        self.targets_found += 1;

        if self.bfs_distance_distribution.len() < bfs_dist + 1 {
            self.bfs_distance_distribution.resize(bfs_dist + 1, 0);
        }
        self.bfs_distance_distribution[bfs_dist] += 1;

        if self.hamming_distance_distribution.len() < hamming_dist + 1 {
            self.hamming_distance_distribution
                .resize(hamming_dist + 1, 0);
        }
        self.hamming_distance_distribution[hamming_dist] += 1;
    }

    /// Adds `n_considered_nodes` to the count of nodes that were searched.
    pub fn add_considered_nodes(&mut self, n_considered_nodes: usize) {
        self.nodes_considered += n_considered_nodes;
    }

    /// Adds `n_components` to the count of components searched.
    pub fn add_components(&mut self, n_components: usize) {
        self.total_components += n_components;
    }

    /// `(hops, matches)` pairs in ascending hop order, for hop counts that
    /// occurred at least once.
    pub fn bfs_distance_counts(&self) -> Vec<(usize, usize)> {
        self.bfs_distance_distribution
            .iter()
            .copied()
            .enumerate()
            .filter(|&(_, c)| c > 0)
            .collect()
    }

    /// `(distance, matches)` pairs in ascending distance order, for Hamming
    /// distances that occurred at least once.
    pub fn hamming_distance_counts(&self) -> Vec<(usize, usize)> {
        self.hamming_distance_distribution
            .iter()
            .copied()
            .enumerate()
            .filter(|&(_, c)| c > 0)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_all_zero() {
        let stats = CollapseStats::default();

        assert_eq!(stats.total_components, 0);
        assert_eq!(stats.targets_found, 0);
        assert_eq!(stats.nodes_considered, 0);
        assert!(stats.bfs_distance_counts().is_empty());
        assert!(stats.hamming_distance_counts().is_empty());
    }

    #[test]
    fn add_target_stats_tallies_each_distance_separately() {
        let mut stats = CollapseStats::default();
        stats.add_target_stats(2, 1);
        stats.add_target_stats(2, 2);
        stats.add_target_stats(4, 1);

        assert_eq!(stats.targets_found, 3);
        assert_eq!(stats.bfs_distance_counts(), [(2, 2), (4, 1)]);
        assert_eq!(stats.hamming_distance_counts(), [(1, 2), (2, 1)]);
    }

    /// The vectors are indexed by distance, so the unused slots below the
    /// first match must not show up as matches.
    #[test]
    fn counts_skip_distances_that_never_occurred() {
        let mut stats = CollapseStats::default();
        stats.add_target_stats(6, 3);

        assert_eq!(stats.bfs_distance_counts(), [(6, 1)]);
        assert_eq!(stats.hamming_distance_counts(), [(3, 1)]);
    }

    #[test]
    fn counts_come_out_in_ascending_order_whatever_order_they_arrived_in() {
        let mut stats = CollapseStats::default();
        stats.add_target_stats(6, 3);
        stats.add_target_stats(2, 1);

        assert_eq!(stats.bfs_distance_counts(), [(2, 1), (6, 1)]);
        assert_eq!(stats.hamming_distance_counts(), [(1, 1), (3, 1)]);
    }

    #[test]
    fn node_and_component_counts_accumulate() {
        let mut stats = CollapseStats::default();
        stats.add_considered_nodes(3);
        stats.add_considered_nodes(4);
        stats.add_components(1);
        stats.add_components(2);

        assert_eq!(stats.nodes_considered, 7);
        assert_eq!(stats.total_components, 3);
        assert_eq!(stats.targets_found, 0);
    }

    #[test]
    fn add_assign_merges_each_field_independently() {
        let mut total = CollapseStats::default();
        total.add_components(1);
        total.add_considered_nodes(5);
        total.add_target_stats(2, 1);

        // Longer on both distributions than `total`, so merging has to grow them.
        let mut other = CollapseStats::default();
        other.add_components(2);
        other.add_considered_nodes(2);
        other.add_target_stats(2, 2);
        other.add_target_stats(4, 1);
        total += other;

        assert_eq!(total.total_components, 3);
        assert_eq!(total.nodes_considered, 7);
        assert_eq!(total.targets_found, 3);
        assert_eq!(total.bfs_distance_counts(), [(2, 2), (4, 1)]);
        assert_eq!(total.hamming_distance_counts(), [(1, 2), (2, 1)]);
    }

    /// The shorter side must not truncate the longer one.
    #[test]
    fn add_assign_keeps_a_longer_left_side() {
        let mut total = CollapseStats::default();
        total.add_target_stats(4, 3);

        let mut other = CollapseStats::default();
        other.add_target_stats(2, 1);
        total += other;

        assert_eq!(total.bfs_distance_counts(), [(2, 1), (4, 1)]);
        assert_eq!(total.hamming_distance_counts(), [(1, 1), (3, 1)]);
    }

    #[test]
    fn adding_an_empty_default_changes_nothing() {
        let mut total = CollapseStats::default();
        total.add_components(1);
        total.add_considered_nodes(4);
        total.add_target_stats(2, 1);
        total += CollapseStats::default();

        assert_eq!(total.total_components, 1);
        assert_eq!(total.nodes_considered, 4);
        assert_eq!(total.targets_found, 1);
        assert_eq!(total.bfs_distance_counts(), [(2, 1)]);
        assert_eq!(total.hamming_distance_counts(), [(1, 1)]);
    }
}
