use crate::domain::{DomainError, MediaGroup, MediaRepository, MediaSummary};
use rayon::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use uuid::Uuid;

/// Maximum number of items that can be grouped at once.
const MAX_GROUPABLE_ITEMS: usize = 10_000;

/// Maximum number of edges (similar pairs) before aborting to prevent OOM.
const MAX_EDGES: usize = 5_000_000;

/// Disjoint-set (Union-Find) with path compression and union by rank.
struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }

    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return;
        }
        match self.rank[ra].cmp(&self.rank[rb]) {
            std::cmp::Ordering::Less => self.parent[ra] = rb,
            std::cmp::Ordering::Greater => self.parent[rb] = ra,
            std::cmp::Ordering::Equal => {
                self.parent[rb] = ra;
                self.rank[ra] += 1;
            }
        }
    }
}

/// Compute dot product of two slices.
#[inline(always)]
fn dot(a: &[f32], b: &[f32]) -> f32 {
    // Simple loop — LLVM auto-vectorizes this to SIMD on x86-64 and aarch64.
    let mut sum = 0.0f32;
    for i in 0..a.len() {
        sum += a[i] * b[i];
    }
    sum
}

pub struct GroupMediaUseCase {
    repo: Arc<dyn MediaRepository>,
}

impl GroupMediaUseCase {
    pub fn new(repo: Arc<dyn MediaRepository>) -> Self {
        Self { repo }
    }

    pub async fn execute(
        &self,
        folder_id: Option<Uuid>,
        threshold: f32,
    ) -> Result<Vec<MediaGroup>, DomainError> {
        // 1. Load all embeddings (pre-normalized by the repo).
        let items = self.repo.get_all_embeddings(folder_id)?;

        if items.is_empty() {
            return Ok(Vec::new());
        }

        let n = items.len();

        // Cap item count to prevent O(N^2) blowup
        if n > MAX_GROUPABLE_ITEMS {
            return Err(DomainError::Io(format!(
                "Too many items to group: {} (max {})",
                n, MAX_GROUPABLE_ITEMS
            )));
        }

        let dim = items[0].1.len();

        // 2. Pack all vectors into a contiguous flat buffer for cache-friendly access.
        //    Layout: [vec0_f0, vec0_f1, ..., vec0_f1279, vec1_f0, ...]
        let mut matrix = Vec::with_capacity(n * dim);
        let mut summaries: Vec<MediaSummary> = Vec::with_capacity(n);
        let mut valid = vec![true; n]; // track zero-norm vectors

        for (summary, vec) in items {
            if vec.iter().all(|&v| v == 0.0) {
                valid[summaries.len()] = false;
            }
            matrix.extend_from_slice(&vec);
            summaries.push(summary);
        }

        // 3. Parallel pairwise comparison with edge count cap.
        //    With pre-normalized vectors: cosine_distance = 1.0 - dot(a, b).
        //    We need edges where dot(a, b) >= min_dot.
        let min_dot = 1.0 - threshold;

        let edge_count = Arc::new(AtomicUsize::new(0));
        let exceeded = Arc::new(AtomicBool::new(false));

        let edges: Vec<(usize, usize)> = (0..n)
            .into_par_iter()
            .flat_map_iter(|i| {
                if !valid[i] || exceeded.load(Ordering::Relaxed) {
                    return Vec::new();
                }
                let vec_i = &matrix[i * dim..(i + 1) * dim];
                let mut local_edges = Vec::new();

                for j in (i + 1)..n {
                    if exceeded.load(Ordering::Relaxed) {
                        break;
                    }
                    if !valid[j] {
                        continue;
                    }
                    let vec_j = &matrix[j * dim..(j + 1) * dim];
                    if dot(vec_i, vec_j) >= min_dot {
                        local_edges.push((i, j));
                        if edge_count.fetch_add(1, Ordering::Relaxed) + 1 > MAX_EDGES {
                            exceeded.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
                local_edges
            })
            .collect();

        if exceeded.load(Ordering::Relaxed) {
            return Err(DomainError::Io(format!(
                "Too many similar pairs (>{}) — try a higher similarity threshold",
                MAX_EDGES
            )));
        }

        // 4. Union-Find merging (sequential — trivially fast on the edge list).
        let mut uf = UnionFind::new(n);
        for &(a, b) in &edges {
            uf.union(a, b);
        }

        // 5. Collect connected components.
        let mut component_map: std::collections::HashMap<usize, Vec<usize>> =
            std::collections::HashMap::new();
        for i in 0..n {
            let root = uf.find(i);
            component_map.entry(root).or_default().push(i);
        }

        // 6. Build groups, skipping singletons.
        let mut summary_slots: Vec<Option<MediaSummary>> =
            summaries.drain(..).map(Some).collect();

        let mut groups: Vec<MediaGroup> = Vec::new();
        for members in component_map.into_values() {
            if members.len() < 2 {
                continue;
            }
            let items: Vec<MediaSummary> = members
                .into_iter()
                .filter_map(|idx| summary_slots[idx].take())
                .collect();

            groups.push(MediaGroup { id: 0, items });
        }

        // 7. Sort groups by newest item first, assign sequential IDs.
        groups.sort_by(|a, b| b.items[0].original_date.cmp(&a.items[0].original_date));
        for (i, group) in groups.iter_mut().enumerate() {
            group.id = i;
        }

        Ok(groups)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_find_basic() {
        let mut uf = UnionFind::new(5);
        uf.union(0, 1);
        uf.union(2, 3);
        assert_eq!(uf.find(0), uf.find(1));
        assert_ne!(uf.find(0), uf.find(2));
        uf.union(1, 3);
        assert_eq!(uf.find(0), uf.find(3));
        // 4 is still isolated
        assert_ne!(uf.find(0), uf.find(4));
    }

    #[test]
    fn dot_product_correct() {
        let a = [1.0, 2.0, 3.0];
        let b = [4.0, 5.0, 6.0];
        let result = dot(&a, &b);
        assert!((result - 32.0).abs() < 1e-6);
    }

    #[test]
    fn dot_product_zero() {
        let a = [1.0, 0.0, 0.0];
        let b = [0.0, 1.0, 0.0];
        assert!((dot(&a, &b) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn constants_are_valid() {
        assert!(MAX_GROUPABLE_ITEMS > 0);
        assert!(MAX_GROUPABLE_ITEMS <= 100_000);
        assert!(MAX_EDGES > 0);
        assert!(MAX_EDGES <= 50_000_000);
    }
}
