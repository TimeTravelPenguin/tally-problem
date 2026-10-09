use std::hash::BuildHasher;

use rustc_hash::{FxBuildHasher, FxHashMap as HashMap};

use super::SearchError;

pub(super) type NodeId = u32;
pub(super) const EMPTY: NodeId = 0;
const TERMINAL: NodeId = 1;

/// A digit trie with shared suffixes. Digits are ordered least significant first
/// so adding one only changes the single branch that propagates a carry.
#[derive(Debug, Default)]
pub(super) struct Diagram {
    nodes: Vec<Node>,
    unique: HashMap<u64, NodeId>,
    unions: HashMap<(NodeId, NodeId), NodeId>,
    differences: HashMap<(NodeId, NodeId), NodeId>,
    reset_preimages: HashMap<(NodeId, u8), NodeId>,
    increments: HashMap<NodeId, NodeId>,
    work: u64,
}

#[derive(Debug)]
struct Node {
    children: [NodeId; 10],
    // The hash table stores a compact fingerprint, while this chain compares
    // the full children when fingerprints collide. Identity is always exact.
    collision: NodeId,
}

impl Diagram {
    pub(super) fn singleton(&mut self, digits: &[u8]) -> Result<NodeId, SearchError> {
        let mut root = TERMINAL;

        for &digit in digits {
            let mut children = [EMPTY; 10];
            children[digit as usize] = root;
            root = self.intern(children)?;
        }

        Ok(root)
    }

    pub(super) fn contains(&self, mut root: NodeId, digits: &[u8]) -> bool {
        for &digit in digits.iter().rev() {
            if root == EMPTY {
                return false;
            }

            root = self.children(root)[digit as usize];
        }

        root == TERMINAL
    }

    pub(super) fn union(&mut self, left: NodeId, right: NodeId) -> Result<NodeId, SearchError> {
        self.combine(left, right, false)
    }

    pub(super) fn difference(
        &mut self,
        left: NodeId,
        right: NodeId,
    ) -> Result<NodeId, SearchError> {
        self.combine(left, right, true)
    }

    fn combine(
        &mut self,
        left: NodeId,
        right: NodeId,
        difference: bool,
    ) -> Result<NodeId, SearchError> {
        self.record_work(1);

        if let Some(result) = self.combined(left, right, difference) {
            return Ok(result);
        }

        let mut pending = Vec::new();
        push(&mut pending, (left, right, false))?;

        while let Some((left, right, expanded)) = pending.pop() {
            self.record_work(1);

            if self.combined(left, right, difference).is_some() {
                continue;
            }

            let left_children = self.children(left);
            let right_children = self.children(right);

            if !expanded {
                push(&mut pending, (left, right, true))?;

                for (&left_child, &right_child) in left_children.iter().zip(&right_children) {
                    if self.combined(left_child, right_child, difference).is_none() {
                        push(&mut pending, (left_child, right_child, false))?;
                    }
                }

                continue;
            }

            let mut children = [EMPTY; 10];

            for (idx, child) in children.iter_mut().enumerate() {
                *child = self
                    .combined(left_children[idx], right_children[idx], difference)
                    .expect("Child pairs are processed before their parent");
            }

            let result = self.intern(children)?;
            let cache = if difference {
                &mut self.differences
            } else {
                &mut self.unions
            };

            insert(cache, (left, right), result)?;
        }

        Ok(self.combined(left, right, difference).unwrap())
    }

    fn combined(&self, left: NodeId, right: NodeId, difference: bool) -> Option<NodeId> {
        if right == EMPTY || (!difference && left == right) {
            return Some(left);
        }

        if left == EMPTY || (difference && left == right) {
            return Some(if difference { EMPTY } else { right });
        }

        let cache = if difference {
            &self.differences
        } else {
            &self.unions
        };

        cache.get(&(left, right)).copied()
    }

    /// Preimage of the decimal increment, including wraparound. The other nine
    /// branches do not carry and can reuse their entire existing subtrees.
    pub(super) fn increment_preimage(&mut self, root: NodeId) -> Result<NodeId, SearchError> {
        self.record_work(1);

        if root <= TERMINAL {
            return Ok(root);
        }

        let mut pending = Vec::new();
        let mut node = root;

        while node > TERMINAL && !self.increments.contains_key(&node) {
            self.record_work(1);
            push(&mut pending, node)?;
            node = self.children(node)[0];
        }

        let mut result = self.increments.get(&node).copied().unwrap_or(node);

        while let Some(node) = pending.pop() {
            self.record_work(1);
            let previous = self.children(node);
            let mut children = [EMPTY; 10];
            children[..9].copy_from_slice(&previous[1..]);
            children[9] = result;
            result = self.intern(children)?;
            insert(&mut self.increments, node, result)?;
        }

        Ok(result)
    }

    /// A forward reset sends every occurrence of `reset` to its successor.
    /// Its inverse may describe 2^N predecessors, all represented by one DAG.
    pub(super) fn reset_preimage(
        &mut self,
        root: NodeId,
        reset: u8,
    ) -> Result<NodeId, SearchError> {
        self.record_work(1);

        if root <= TERMINAL {
            return Ok(root);
        }

        let mut pending = Vec::new();
        push(&mut pending, (root, false))?;

        while let Some((node, expanded)) = pending.pop() {
            self.record_work(1);

            if node <= TERMINAL || self.reset_preimages.contains_key(&(node, reset)) {
                continue;
            }

            let previous = self.children(node);

            if !expanded {
                push(&mut pending, (node, true))?;

                for (digit, &child) in previous.iter().enumerate() {
                    // A reset never outputs its old index, so this branch has
                    // no predecessor and need not be traversed.
                    if digit != reset as usize && child > TERMINAL {
                        push(&mut pending, (child, false))?;
                    }
                }

                continue;
            }

            let mut children = [EMPTY; 10];

            for (digit, child) in children.iter_mut().enumerate() {
                let output = if digit == reset as usize {
                    (digit + 1) % 10
                } else {
                    digit
                };

                let previous_child = previous[output];
                *child = if previous_child <= TERMINAL {
                    previous_child
                } else {
                    self.reset_preimages[&(previous_child, reset)]
                };
            }

            let result = self.intern(children)?;
            insert(&mut self.reset_preimages, (node, reset), result)?;
        }

        Ok(self.reset_preimages[&(root, reset)])
    }

    pub(super) fn clear_caches(&mut self) {
        self.record_work(self.cached_entries());
        self.unions.clear();
        self.differences.clear();
        self.reset_preimages.clear();
        self.increments.clear();
    }

    fn cached_entries(&self) -> usize {
        self.unions
            .len()
            .saturating_add(self.differences.len())
            .saturating_add(self.reset_preimages.len())
            .saturating_add(self.increments.len())
    }

    /// Compact only between search groups. All retained frontier, settled and
    /// reconstruction roots must be supplied and then remapped by the caller.
    pub(super) fn collect(
        &mut self,
        roots: impl Iterator<Item = NodeId>,
    ) -> Result<Vec<NodeId>, SearchError> {
        // Account for the reachability, live-node count and rebuilding scans.
        // This cumulative counter survives both compaction and cache resets.
        self.record_work(self.nodes.len().saturating_mul(3).saturating_add(2));
        self.record_work(self.cached_entries());
        self.unions = HashMap::default();
        self.differences = HashMap::default();
        self.reset_preimages = HashMap::default();
        self.increments = HashMap::default();
        let mut mapping = Vec::new();
        mapping
            .try_reserve_exact(self.nodes.len() + 2)
            .map_err(|_| SearchError::AllocationFailed)?;
        mapping.resize(self.nodes.len() + 2, EMPTY);
        mapping[1] = TERMINAL;

        for root in roots {
            self.record_work(1);

            if root > TERMINAL {
                mapping[root as usize] = NodeId::MAX;
            }
        }

        // Children always have smaller IDs, so both reachability and rebuilding
        // can use linear passes without recursion or a second traversal stack.
        for idx in (0..self.nodes.len()).rev() {
            if mapping[idx + 2] == NodeId::MAX {
                for child in self.nodes[idx].children {
                    if child > TERMINAL {
                        mapping[child as usize] = NodeId::MAX;
                    }
                }
            }
        }

        let live_nodes = mapping.iter().filter(|&&node| node == NodeId::MAX).count();
        let mut nodes = Vec::new();
        let mut unique = HashMap::default();
        nodes
            .try_reserve_exact(live_nodes)
            .map_err(|_| SearchError::AllocationFailed)?;
        unique
            .try_reserve(live_nodes)
            .map_err(|_| SearchError::AllocationFailed)?;

        for (idx, previous) in self.nodes.iter().enumerate() {
            if mapping[idx + 2] == NodeId::MAX {
                let children = previous.children.map(|child| mapping[child as usize]);
                let node = (nodes.len() + 2) as NodeId;
                mapping[idx + 2] = node;
                let hash = FxBuildHasher.hash_one(children);
                let collision = unique.get(&hash).copied().unwrap_or(EMPTY);
                nodes.push(Node {
                    children,
                    collision,
                });
                unique.insert(hash, node);
            }
        }

        self.nodes = nodes;
        self.unique = unique;

        Ok(mapping)
    }

    pub(super) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub(super) fn work(&self) -> u64 {
        self.work
    }

    fn record_work(&mut self, amount: usize) {
        self.work = self.work.saturating_add(amount as u64);
    }

    fn children(&self, node: NodeId) -> [NodeId; 10] {
        self.nodes[node as usize - 2].children
    }

    fn intern(&mut self, children: [NodeId; 10]) -> Result<NodeId, SearchError> {
        self.record_work(1);

        if children == [EMPTY; 10] {
            return Ok(EMPTY);
        }

        let hash = FxBuildHasher.hash_one(children);
        let collision = self.unique.get(&hash).copied().unwrap_or(EMPTY);
        let mut candidate = collision;

        while candidate > TERMINAL {
            self.record_work(1);
            let node = &self.nodes[candidate as usize - 2];

            if node.children == children {
                return Ok(candidate);
            }

            candidate = node.collision;
        }

        let node = self
            .nodes
            .len()
            .checked_add(2)
            .and_then(|idx| idx.try_into().ok())
            .ok_or(SearchError::AllocationFailed)?;
        self.nodes
            .try_reserve(1)
            .map_err(|_| SearchError::AllocationFailed)?;
        self.unique
            .try_reserve(1)
            .map_err(|_| SearchError::AllocationFailed)?;
        self.nodes.push(Node {
            children,
            collision,
        });
        self.unique.insert(hash, node);

        // Nonempty nodes carry their depth through their nonempty children;
        // terminals occur only at the end of a word. Identical child arrays
        // therefore always have identical depth, without a separate level key.
        Ok(node)
    }
}

pub(super) fn insert<Key: std::hash::Hash + Eq, Value>(
    map: &mut HashMap<Key, Value>,
    key: Key,
    value: Value,
) -> Result<(), SearchError> {
    map.try_reserve(1)
        .map_err(|_| SearchError::AllocationFailed)?;
    map.insert(key, value);

    Ok(())
}

pub(super) fn push<Value>(buffer: &mut Vec<Value>, value: Value) -> Result<(), SearchError> {
    buffer
        .try_reserve(1)
        .map_err(|_| SearchError::AllocationFailed)?;
    buffer.push(value);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TallyCounter;

    fn digits(mut value: usize, width: usize) -> Vec<u8> {
        let mut result = vec![0; width];

        for digit in result.iter_mut().rev() {
            *digit = (value % 10) as u8;
            value /= 10;
        }

        result
    }

    fn value(digits: &[u8]) -> usize {
        digits
            .iter()
            .fold(0, |acc, &digit| acc * 10 + usize::from(digit))
    }

    fn word_set(diagram: &mut Diagram, members: &[bool]) -> NodeId {
        let mut root = EMPTY;

        for (member, &included) in members.iter().enumerate() {
            if included {
                let singleton = diagram.singleton(&digits(member, 3)).unwrap();
                root = diagram.union(root, singleton).unwrap();
            }
        }

        root
    }

    #[test]
    fn fingerprint_collisions_do_not_merge_distinct_nodes() {
        let mut diagram = Diagram::default();
        let first = diagram.singleton(&[1]).unwrap();
        let second = diagram.singleton(&[2]).unwrap();
        let first_hash = FxBuildHasher.hash_one(diagram.children(first));

        // Force two different nodes into one fingerprint chain. Lookups must
        // compare their children instead of treating a hash as their identity.
        diagram.nodes[second as usize - 2].collision = first;
        diagram.unique.insert(first_hash, second);

        assert_eq!(diagram.singleton(&[1]).unwrap(), first);
        assert_ne!(first, second);
        assert!(diagram.contains(first, &[1]));
        assert!(!diagram.contains(first, &[2]));
    }

    #[test]
    fn boolean_operations_match_explicit_sets_with_shared_branches() {
        let left: Vec<_> = (0..1_000)
            .map(|member| member % 3 == 0 || member / 100 == member % 10)
            .collect();
        let right: Vec<_> = (0..1_000)
            .map(|member| member % 5 == 0 || member / 10 % 10 == 7)
            .collect();
        let mut diagram = Diagram::default();
        let left_root = word_set(&mut diagram, &left);
        let right_root = word_set(&mut diagram, &right);
        let union = diagram.union(left_root, right_root).unwrap();
        let difference = diagram.difference(left_root, right_root).unwrap();

        for member in 0..1_000 {
            let word = digits(member, 3);
            assert_eq!(
                diagram.contains(union, &word),
                left[member] || right[member]
            );
            assert_eq!(
                diagram.contains(difference, &word),
                left[member] && !right[member]
            );
        }

        diagram.clear_caches();
        assert_eq!(diagram.union(left_root, right_root).unwrap(), union);
        assert_eq!(diagram.union(right_root, left_root).unwrap(), union);
        assert_eq!(
            diagram.difference(left_root, right_root).unwrap(),
            difference
        );
    }

    #[test]
    fn preimages_match_counter_operations_including_wraparound() {
        let members: Vec<_> = (0..1_000)
            .map(|member| member % 11 == 0 || member / 10 % 10 == 0)
            .collect();
        let mut diagram = Diagram::default();
        let root = word_set(&mut diagram, &members);
        let increment = diagram.increment_preimage(root).unwrap();
        let mut resets = [EMPTY; 10];

        for (reset, preimage) in resets.iter_mut().enumerate() {
            *preimage = diagram.reset_preimage(root, reset as u8).unwrap();
        }

        for member in 0..1_000 {
            let word = digits(member, 3);
            let mut initial = TallyCounter::new(3).unwrap();
            initial.set_values(word.clone()).unwrap();
            let mut counter = initial.clone();
            counter.increment();
            assert_eq!(
                diagram.contains(increment, &word),
                members[value(counter.values())]
            );

            for (reset, &preimage) in resets.iter().enumerate() {
                let mut counter = initial.clone();
                counter.set_reset_index(reset as u8).unwrap();
                counter.tick_reset_forward();
                assert_eq!(
                    diagram.contains(preimage, &word),
                    members[value(counter.values())]
                );
            }
        }

        diagram.clear_caches();
        assert_eq!(diagram.increment_preimage(root).unwrap(), increment);

        for (reset, &preimage) in resets.iter().enumerate() {
            assert_eq!(diagram.reset_preimage(root, reset as u8).unwrap(), preimage);
        }
    }

    #[test]
    fn wide_shared_diagrams_use_iterative_traversal() {
        let width = 10_000;
        let zeros = vec![0; width];
        let nines = vec![9; width];
        let mixed: Vec<_> = (0..width)
            .map(|idx| if idx % 2 == 0 { 0 } else { 9 })
            .collect();
        let mut diagram = Diagram::default();
        let zero = diagram.singleton(&zeros).unwrap();
        let increment = diagram.increment_preimage(zero).unwrap();
        let reset = diagram.reset_preimage(zero, 9).unwrap();
        let union = diagram.union(zero, increment).unwrap();
        let difference = diagram.difference(reset, zero).unwrap();

        assert!(diagram.contains(increment, &nines));
        assert!(diagram.contains(reset, &mixed));
        assert!(diagram.contains(union, &zeros));
        assert!(diagram.contains(union, &nines));
        assert!(diagram.contains(difference, &mixed));
        assert!(!diagram.contains(difference, &zeros));
    }

    #[test]
    fn collection_remaps_retained_roots_and_preserves_canonical_operations() {
        let mut diagram = Diagram::default();
        let original = diagram.singleton(&[1, 2, 3]).unwrap();
        let predecessor = diagram.increment_preimage(original).unwrap();
        let union = diagram.union(original, predecessor).unwrap();
        let discarded: Vec<_> = (0..1_000).map(|member| member % 3 == 0).collect();
        let _ = word_set(&mut diagram, &discarded);
        let previous_count = diagram.node_count();
        let previous_work = diagram.work();
        let mapping = diagram
            .collect([original, predecessor, union].into_iter())
            .unwrap();
        let original = mapping[original as usize];
        let predecessor = mapping[predecessor as usize];
        let union = mapping[union as usize];

        assert!(diagram.node_count() < previous_count);
        assert!(diagram.work() > previous_work);
        assert!(diagram.contains(original, &[1, 2, 3]));
        assert!(diagram.contains(predecessor, &[1, 2, 2]));
        assert!(diagram.contains(union, &[1, 2, 2]));
        assert!(diagram.contains(union, &[1, 2, 3]));
        assert!(!diagram.contains(union, &[1, 2, 4]));
        assert_eq!(diagram.increment_preimage(original).unwrap(), predecessor);
        assert_eq!(diagram.union(original, predecessor).unwrap(), union);
    }

    #[test]
    fn work_snapshots_survive_cache_clears_and_compaction() {
        let mut diagram = Diagram::default();
        let root = diagram.singleton(&[0, 0, 0]).unwrap();
        let reset = diagram.reset_preimage(root, 9).unwrap();
        let previous_work = diagram.work();
        assert!(previous_work > 0);

        diagram.clear_caches();
        assert!(diagram.work() >= previous_work);
        let before_collection = diagram.work();
        let mapping = diagram.collect([root, reset].into_iter()).unwrap();
        let root = mapping[root as usize];
        let reset = mapping[reset as usize];
        assert!(diagram.work() > before_collection);
        let collected_work = diagram.work();

        assert_eq!(diagram.reset_preimage(root, 9).unwrap(), reset);
        assert!(diagram.work() > collected_work);
        assert!(diagram.contains(reset, &[9, 0, 9]));
    }
}
