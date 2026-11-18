//! Stable dependency graph ordering for application requirement aliases.
use crate::{AppSchema, SchemaError};
use std::collections::BTreeMap;

impl AppSchema {
    pub(crate) fn order_indices(&self) -> Result<Vec<usize>, SchemaError> {
        let positions: BTreeMap<&str, usize> = self
            .requires
            .iter()
            .enumerate()
            .map(|(index, requirement)| (requirement.name.as_str(), index))
            .collect();
        let mut started = vec![false; self.requires.len()];
        let mut order = Vec::with_capacity(self.requires.len());
        while order.len() < self.requires.len() {
            let next = self
                .requires
                .iter()
                .enumerate()
                .find(|(index, requirement)| {
                    !started[*index]
                        && requirement.depends_on.iter().all(|dependency| {
                            positions
                                .get(dependency.as_str())
                                .is_some_and(|position| started[*position])
                        })
                });
            let Some((index, _)) = next else {
                return Err(SchemaError::DependencyCycle {
                    requirements: self
                        .requires
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| !started[*index])
                        .map(|(_, requirement)| requirement.name.clone())
                        .collect(),
                });
            };
            started[index] = true;
            order.push(index);
        }
        Ok(order)
    }
}
