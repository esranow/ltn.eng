use super::smt::SolverError;
use std::collections::{HashMap, HashSet};

pub struct DependencyGraph<T> {
    nodes: HashSet<T>,
    edges: HashMap<T, Vec<T>>,
    in_degree: HashMap<T, usize>,
}

impl<T: Eq + std::hash::Hash + Clone> Default for DependencyGraph<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Eq + std::hash::Hash + Clone> DependencyGraph<T> {
    pub fn new() -> Self {
        Self {
            nodes: HashSet::new(),
            edges: HashMap::new(),
            in_degree: HashMap::new(),
        }
    }

    pub fn add_node(&mut self, node: T) {
        if self.nodes.insert(node.clone()) {
            self.edges.entry(node.clone()).or_default();
            self.in_degree.entry(node).or_insert(0);
        }
    }

    pub fn add_edge(&mut self, from: T, to: T) {
        self.add_node(from.clone());
        self.add_node(to.clone());
        self.edges.get_mut(&from).unwrap().push(to.clone());
        *self.in_degree.get_mut(&to).unwrap() += 1;
    }

    pub fn topological_sort(&self) -> Result<Vec<T>, SolverError> {
        let mut in_degree = self.in_degree.clone();
        let mut queue: Vec<T> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(node, _)| node.clone())
            .collect();

        let mut sorted = Vec::new();

        while let Some(node) = queue.pop() {
            sorted.push(node.clone());
            if let Some(neighbors) = self.edges.get(&node) {
                for neighbor in neighbors {
                    let deg = in_degree.get_mut(neighbor).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(neighbor.clone());
                    }
                }
            }
        }

        if sorted.len() != self.nodes.len() {
            return Err(SolverError::CycleDetected);
        }

        Ok(sorted)
    }
}
