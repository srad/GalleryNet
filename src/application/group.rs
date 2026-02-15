use crate::domain::{DomainError, MediaGroup, MediaRepository, MediaSummary};
use rayon::prelude::*;
use std::sync::Arc;
use uuid::Uuid;

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

        // 3. Parallel pairwise comparison.
        //    With pre-normalized vectors: cosine_distance = 1.0 - dot(a, b).
        //    We need edges where dot(a, b) >= min_dot.
        //
        //    Each row i is processed in parallel. For each i, we scan j > i and
        //    collect matching (i, j) pairs into a thread-local vec. Rayon merges
        //    all per-row edge lists at the end.
        let min_dot = 1.0 - threshold;

        let edges: Vec<(usize, usize)> = (0..n)
            .into_par_iter()
            .flat_map_iter(|i| {
                if !valid[i] {
                    return Vec::new();
                }
                let vec_i = &matrix[i * dim..(i + 1) * dim];
                let mut local_edges = Vec::new();

                for j in (i + 1)..n {
                    if !valid[j] {
                        continue;
                    }
                    let vec_j = &matrix[j * dim..(j + 1) * dim];
                    if dot(vec_i, vec_j) >= min_dot {
                        local_edges.push((i, j));
                    }
                }
                local_edges
            })
            .collect();

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
