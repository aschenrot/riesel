//! Research-only candidate for *optional* S3 typed-port metadata.
//!
//! This deliberately does NOT extend Riesel's SDK, execute value edges, or
//! own data. It checks one possible app-side signature preflight and then
//! maps eligible typed edges into the existing production **S2 control DAG**.
//! Only the application/domain can supply actual values and prove handler
//! signatures, affinity and lifetime. This is NOT a complete S3 solution.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use riesel::{ExecutionPlanDraft, OperationKey, OperationNode, ValidatedExecutionPlan};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TypeId {
    pub name: &'static str,
    pub version: u32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Port {
    pub name: &'static str,
    pub ty: TypeId,
    pub max_bytes: usize,
    pub required: bool,
}

#[derive(Clone, Debug)]
pub(super) struct Operation {
    pub node: &'static str,
    pub binding: &'static str,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    /// Explicit S2 prerequisite node IDs: `before` these operations.
    pub after: Vec<&'static str>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ValueEdge {
    pub source: &'static str,
    pub output: &'static str,
    pub target: &'static str,
    pub input: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PortError {
    TooManyNodes,
    DuplicateNode,
    DuplicatePort,
    MissingNode,
    MissingPort,
    TypeMismatch,
    VersionMismatch,
    ReceiverCapacityExceeded,
    GraphBudgetExceeded,
    MissingRequiredInput,
    MultipleProducers,
    Cycle,
    RieselPlan(String),
}

const MAX_NODES: usize = 16;
const MAX_RETAINED_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

fn unique_ports(ports: &[Port]) -> bool {
    let mut names = BTreeSet::new();
    ports.iter().all(|p| !p.name.is_empty() && names.insert(p.name))
}

/// A bounded preflight for an *explicitly declared* candidate graph; no
/// reflection of arbitrary Rust handler signatures, no dynamic type import.
pub(super) fn compile_candidate(
    operations: &[Operation],
    value_edges: &[ValueEdge],
) -> Result<ValidatedExecutionPlan, PortError> {
    if operations.is_empty() || operations.len() > MAX_NODES || value_edges.len() > MAX_NODES * MAX_NODES {
        return Err(PortError::TooManyNodes);
    }

    let mut by_id = BTreeMap::new();
    let mut retained = 0usize;
    for (position, op) in operations.iter().enumerate() {
        if by_id.insert(op.node, position).is_some() {
            return Err(PortError::DuplicateNode);
        }
        if !unique_ports(&op.inputs) || !unique_ports(&op.outputs) {
            return Err(PortError::DuplicatePort);
        }
        for output in &op.outputs {
            retained = retained.checked_add(output.max_bytes)
                .ok_or(PortError::GraphBudgetExceeded)?;
            if retained > MAX_RETAINED_OUTPUT_BYTES {
                return Err(PortError::GraphBudgetExceeded);
            }
        }
    }

    let mut dependencies = BTreeSet::new();
    for op in operations {
        for prerequisite in &op.after {
            if !by_id.contains_key(prerequisite) {
                return Err(PortError::MissingNode);
            }
            dependencies.insert((*prerequisite, op.node));
        }
    }

    let mut connected_inputs = BTreeSet::new();
    for e in value_edges {
        let Some(&source_i) = by_id.get(e.source) else { return Err(PortError::MissingNode) };
        let Some(&target_i) = by_id.get(e.target) else { return Err(PortError::MissingNode) };
        let source = &operations[source_i];
        let target = &operations[target_i];
        let Some(out) = source.outputs.iter().find(|p| p.name == e.output) else {
            return Err(PortError::MissingPort);
        };
        let Some(input) = target.inputs.iter().find(|p| p.name == e.input) else {
            return Err(PortError::MissingPort);
        };

        if out.ty.name != input.ty.name { return Err(PortError::TypeMismatch); }
        if out.ty.version != input.ty.version { return Err(PortError::VersionMismatch); }
        if out.max_bytes > input.max_bytes {
            return Err(PortError::ReceiverCapacityExceeded);
        }
        if !connected_inputs.insert((e.target, e.input)) {
            return Err(PortError::MultipleProducers);
        }
        dependencies.insert((e.source, e.target));
    }

    for op in operations {
        for input in &op.inputs {
            if input.required && !connected_inputs.contains(&(op.node, input.name)) {
                return Err(PortError::MissingRequiredInput);
            }
        }
    }

    // Check cycles in the UNION of control prerequisites and data edges,
    // including a cycle that neither edge subset contains by itself.
    let mut indegree = operations.iter().map(|op| (op.node, 0usize))
        .collect::<BTreeMap<_, _>>();
    let mut children = BTreeMap::<&str, Vec<&str>>::new();
    for (source, target) in &dependencies {
        *indegree.get_mut(target).expect("node validated") += 1;
        children.entry(*source).or_default().push(*target);
    }
    let mut ready = indegree.iter()
        .filter_map(|(&id, &count)| (count == 0).then_some(id))
        .collect::<BTreeSet<_>>();
    let mut visited = 0usize;
    while let Some(id) = ready.pop_first() {
        visited += 1;
        if let Some(next) = children.get(id) {
            for &child in next {
                let entry = indegree.get_mut(child).expect("node validated");
                *entry -= 1;
                if *entry == 0 { ready.insert(child); }
            }
        }
    }
    if visited != operations.len() { return Err(PortError::Cycle); }

    // Use Riesel's REAL validator for the resulting S2 control graph.
    // Nothing here creates executable typed inputs or outputs.
    let mut draft = ExecutionPlanDraft::new("research.187.typed-adapter", 1);
    for op in operations {
        let key = OperationKey::new(op.binding, 1)
            .map_err(|e| PortError::RieselPlan(e.to_string()))?;
        let mut node = OperationNode::new(op.node, key);
        for (source, target) in &dependencies {
            if *target == op.node { node = node.after(*source); }
        }
        draft = draft.operation(node);
    }
    draft.validate().map_err(|e| PortError::RieselPlan(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIELD: TypeId = TypeId { name: "grid.scalar.2d", version: 1 };
    const OCCUPANCY: TypeId = TypeId { name: "grid.occupancy.2d", version: 1 };
    const FIELD_V2: TypeId = TypeId { name: "grid.scalar.2d", version: 2 };

    fn port(name: &'static str, ty: TypeId, max_bytes: usize) -> Port {
        Port { name, ty, max_bytes, required: true }
    }
    fn source() -> Operation {
        Operation {
            node: "noise",
            binding: "research.noise",
            inputs: vec![],
            outputs: vec![port("scalar", FIELD, 64 * 64 * 8)],
            after: vec![],
        }
    }
    fn threshold() -> Operation {
        Operation {
            node: "threshold",
            binding: "research.threshold",
            inputs: vec![port("scalar", FIELD, 64 * 64 * 8)],
            outputs: vec![port("occupied", OCCUPANCY, 64 * 64)],
            after: vec![],
        }
    }
    fn summary() -> Operation {
        Operation {
            node: "summary",
            binding: "research.summary",
            inputs: vec![port("field", FIELD, 64 * 64 * 8)],
            outputs: vec![],
            after: vec![],
        }
    }
    fn first_edge() -> ValueEdge {
        ValueEdge { source: "noise", output: "scalar", target: "threshold", input: "scalar" }
    }

    #[test]
    fn actual_riesel_plan_accepts_typed_fanout_only_as_control_projection() {
        let ops = [source(), threshold(), summary()];
        let edges = [first_edge(), ValueEdge {
            source: "noise", output: "scalar", target: "summary", input: "field",
        }];
        let validated = compile_candidate(&ops, &edges).unwrap();
        let descriptions = validated.description().operations();
        assert_eq!(descriptions.len(), 3);
        assert!(descriptions[0].dependencies().is_empty());
        assert_eq!(descriptions[1].dependencies(), &["noise"]);
        assert_eq!(descriptions[2].dependencies(), &["noise"]);
        assert!(validated.dependency_order().count() == 3);
        // The production plan only records .after("noise"), never port IDs
        // nor runtime references to values.
    }

    #[test]
    fn catches_type_and_version_mismatch_before_real_plan_validation() {
        let mut wrong = threshold();
        wrong.inputs[0].ty = OCCUPANCY;
        assert_eq!(compile_candidate(&[source(), wrong], &[first_edge()]).err(),
            Some(PortError::TypeMismatch));
        let mut other = threshold();
        other.inputs[0].ty = FIELD_V2;
        assert_eq!(compile_candidate(&[source(), other], &[first_edge()]).err(),
            Some(PortError::VersionMismatch));
    }

    #[test]
    fn required_port_and_competing_producer_rejected() {
        assert_eq!(compile_candidate(&[source(), threshold()], &[]).err(),
            Some(PortError::MissingRequiredInput));
        assert_eq!(compile_candidate(&[source(), threshold()], &[first_edge(), first_edge()]).err(),
            Some(PortError::MultipleProducers));
    }

    #[test]
    fn rejects_missing_endpoint_and_capacity_overrun() {
        let mut e = first_edge();
        e.source = "missing";
        assert_eq!(compile_candidate(&[source(), threshold()], &[e]).err(),
            Some(PortError::MissingNode));
        let mut constrained = threshold();
        constrained.inputs[0].max_bytes = 16;
        assert_eq!(compile_candidate(&[source(), constrained], &[first_edge()]).err(),
            Some(PortError::ReceiverCapacityExceeded));
    }

    #[test]
    fn rejects_combined_control_and_data_cycles_even_when_each_is_acyclic() {
        // Value edge noise -> threshold, CONTROL prerequisite threshold -> noise.
        let mut backwards = source();
        backwards.after = vec!["threshold"];
        assert_eq!(compile_candidate(&[backwards, threshold()], &[first_edge()]).err(),
            Some(PortError::Cycle));
    }

    #[test]
    fn rejects_duplicate_identifiers_and_port_names() {
        assert_eq!(compile_candidate(&[source(), source()], &[]).err(),
            Some(PortError::DuplicateNode));
        let mut duplicate = source();
        duplicate.outputs.push(duplicate.outputs[0]);
        assert_eq!(compile_candidate(&[duplicate, threshold()], &[first_edge()]).err(),
            Some(PortError::DuplicatePort));
    }

    #[test]
    fn budgets_are_for_distinct_outputs_not_fanout_edges() {
        // One producer's output is shared by two consumers. A planner that
        // multiplied by fanout count would mistake references for copies.
        assert!(compile_candidate(&[source(), threshold(), summary()], &[
            first_edge(),
            ValueEdge { source: "noise", output: "scalar", target: "summary", input: "field" },
        ]).is_ok());
        let mut huge = source();
        huge.outputs[0].max_bytes = MAX_RETAINED_OUTPUT_BYTES + 1;
        assert_eq!(compile_candidate(&[huge], &[]).err(),
            Some(PortError::GraphBudgetExceeded));
        // These are static declared upper bounds, NOT actual metering.
    }
}
