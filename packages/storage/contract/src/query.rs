//! Independently versioned read-designated record Query facet (issue #161).
//! Logical operators are deliberately bounded and do not prescribe a
//! universal executor, database index, snapshot, or remote access grant.

use riesel_macros::{RieselType, capability};
use riesel_schema::{ModelDefinition, TypeRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::StoredRecord;

/// Maximum records returned by one linked Query call.
pub const MAX_QUERY_PAGE_SIZE: u64 = 32;
/// Maximum candidate records a provider may inspect for one linked call.
/// A sparse predicate can return an empty *partial* page.
pub const MAX_QUERY_SCAN_BUDGET: u64 = 256;
/// Long names or unbounded string operands would also make stored cursor
/// signatures unbounded. These caps apply even to local in-process callers.
pub const MAX_QUERY_IDENTIFIER_BYTES: usize = 256;
pub const MAX_QUERY_STRING_OPERAND_BYTES: usize = 64 * 1024;

/// The first portable record Query predicate profile. Operators not listed
/// here are unsupported rather than silently emulated by unbounded scanning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, RieselType)]
pub enum RecordCompare {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    IsMissing,
    IsNull,
    IsPresent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, RieselType)]
pub struct RecordPredicate {
    pub field: String,
    pub operator: RecordCompare,
    /// Required for Eq/Ne/Gt/Ge/Lt/Le; absent for presence operators.
    /// Equality with JSON null is invalid: use IsNull or IsMissing.
    pub value: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, RieselType)]
pub struct RecordQueryRequest {
    pub model: String,
    pub predicate: Option<RecordPredicate>,
    /// Returned row bound; not a bound on work.
    pub limit: u64,
    /// Provider inspection bound. May be smaller than limit.
    pub scan_budget: u64,
    /// Opaque provider-issued, owner/session/query-bound continuation token.
    pub cursor: Option<String>,
}

/// A *live* page, not a stable snapshot across successive queries.
/// `exhausted=false` is possible even when no row matched this page.
#[derive(Debug, Clone, Serialize, Deserialize, RieselType)]
pub struct RecordQueryPage {
    pub records: Vec<StoredRecord>,
    pub next_cursor: Option<String>,
    pub exhausted: bool,
    /// Number of candidate records inspected, not number of matches.
    pub inspected: u64,
}

#[derive(Debug, Clone, Error, Serialize, Deserialize, RieselType)]
pub enum RecordQueryError {
    #[error("model is not registered or not readable")]
    ModelDenied,
    #[error("query field is not declared in the registered model")]
    UnknownField,
    #[error("predicate operator is unsupported for this field type")]
    UnsupportedPredicate,
    #[error("predicate operand is invalid for this field or operator")]
    InvalidOperand,
    #[error("page limit must be between 1 and 32 records")]
    InvalidLimit,
    #[error("scan budget must be between 1 and 256 records")]
    InvalidScanBudget,
    #[error("query identifier or string operand exceeds the bounded request profile")]
    RequestTooLarge,
    #[error("continuation token is invalid, expired, or belongs to another query or owner")]
    InvalidCursor,
    #[error("the provider has reached its bounded continuation-token capacity")]
    CursorCapacity,
    #[error("query provider failed: {0}")]
    Failed(String),
    #[error("query transport failed: {0}")]
    Transport(String),
}

impl From<riesel_protocol::RpcError> for RecordQueryError {
    fn from(error: riesel_protocol::RpcError) -> Self {
        Self::Transport(error.to_string())
    }
}

/// Separate read-only RPC surface; the existing Storage v2 interface retains
/// the create/update/delete operations and their independent version.
#[capability(id = "riesel.query.records", version = 1, client_trait)]
pub trait RecordQuery {
    /// Read one bounded live page under the selected registered-model owner.
    #[query]
    fn query_page(&self, request: RecordQueryRequest) -> Result<RecordQueryPage, RecordQueryError>;
}

/// Validate the first profile before a provider begins inspecting records.
/// Provider-specific data visibility and cursor ownership must be checked
/// separately *inside* the provider's data boundary.
pub fn validate_record_query_envelope(
    request: &RecordQueryRequest,
) -> Result<(), RecordQueryError> {
    let oversized = request.model.len() > MAX_QUERY_IDENTIFIER_BYTES
        || request.predicate.as_ref().is_some_and(|predicate| {
            predicate.field.len() > MAX_QUERY_IDENTIFIER_BYTES
                || predicate
                    .value
                    .as_ref()
                    .and_then(Value::as_str)
                    .is_some_and(|value| value.len() > MAX_QUERY_STRING_OPERAND_BYTES)
        });
    if oversized {
        return Err(RecordQueryError::RequestTooLarge);
    }
    if request
        .cursor
        .as_ref()
        .is_some_and(|cursor| cursor.is_empty() || cursor.len() > 64)
    {
        return Err(RecordQueryError::InvalidCursor);
    }
    Ok(())
}

pub fn validate_record_query(
    definition: &ModelDefinition,
    request: &RecordQueryRequest,
) -> Result<(), RecordQueryError> {
    validate_record_query_envelope(request)?;
    if !(1..=MAX_QUERY_PAGE_SIZE).contains(&request.limit) {
        return Err(RecordQueryError::InvalidLimit);
    }
    if !(1..=MAX_QUERY_SCAN_BUDGET).contains(&request.scan_budget) {
        return Err(RecordQueryError::InvalidScanBudget);
    }
    if let Some(predicate) = &request.predicate {
        let ty = definition
            .fields
            .get(&predicate.field)
            .ok_or(RecordQueryError::UnknownField)?;
        validate_record_predicate(ty, predicate)?;
    }
    Ok(())
}

fn field_value_kind(kind: &TypeRef) -> &TypeRef {
    if let TypeRef::Optional { item } = kind {
        item
    } else {
        kind
    }
}

fn validate_record_predicate(
    kind: &TypeRef,
    predicate: &RecordPredicate,
) -> Result<(), RecordQueryError> {
    use RecordCompare as Op;
    match predicate.operator {
        Op::IsMissing | Op::IsNull => {
            if !matches!(kind, TypeRef::Optional { .. }) {
                return Err(RecordQueryError::UnsupportedPredicate);
            }
            if predicate.value.is_some() {
                return Err(RecordQueryError::InvalidOperand);
            }
        }
        Op::IsPresent => {
            if predicate.value.is_some() {
                return Err(RecordQueryError::InvalidOperand);
            }
        }
        op => {
            let value = predicate
                .value
                .as_ref()
                .ok_or(RecordQueryError::InvalidOperand)?;
            if value.is_null() {
                return Err(RecordQueryError::InvalidOperand);
            }
            let valid = match field_value_kind(kind) {
                TypeRef::Bool | TypeRef::String | TypeRef::Uuid
                    if !matches!(op, Op::Eq | Op::Ne) =>
                {
                    return Err(RecordQueryError::UnsupportedPredicate);
                }
                TypeRef::Bool => value.is_boolean(),
                TypeRef::String => value.is_string(),
                TypeRef::Uuid => value.as_str().is_some_and(|s| Uuid::parse_str(s).is_ok()),
                TypeRef::U64 => value.as_u64().is_some(),
                TypeRef::I64 => value.as_i64().is_some(),
                // Floating and untyped JSON comparisons need their own
                // precision/collation profile; reject rather than coerce.
                _ => return Err(RecordQueryError::UnsupportedPredicate),
            };
            if !valid {
                return Err(RecordQueryError::InvalidOperand);
            }
        }
    }
    Ok(())
}

/// The shared *typed comparison semantics* of the first record profile.
/// Memory/SQLite owners remain responsible for scanning, budgets, model
/// authorization, cursor state, cancellation and any native optimization.
pub fn record_matches_predicate(
    record: &Value,
    definition: &ModelDefinition,
    predicate: Option<&RecordPredicate>,
) -> Result<bool, RecordQueryError> {
    let Some(predicate) = predicate else {
        return Ok(true);
    };
    let kind = definition
        .fields
        .get(&predicate.field)
        .ok_or(RecordQueryError::UnknownField)?;
    validate_record_predicate(kind, predicate)?;
    let object = record
        .as_object()
        .ok_or_else(|| RecordQueryError::Failed("stored record is not an object".into()))?;
    let field = object.get(&predicate.field);
    match predicate.operator {
        RecordCompare::IsMissing => return Ok(field.is_none()),
        RecordCompare::IsNull => return Ok(field.is_some_and(Value::is_null)),
        RecordCompare::IsPresent => return Ok(field.is_some()),
        _ => {}
    }
    let Some(actual) = field.filter(|v| !v.is_null()) else {
        // Missing/explicit null are distinct and neither satisfies a typed
        // scalar comparison, including NotEqual.
        return Ok(false);
    };
    let wanted = predicate
        .value
        .as_ref()
        .ok_or(RecordQueryError::InvalidOperand)?;
    let ord = match field_value_kind(kind) {
        TypeRef::Bool => actual
            .as_bool()
            .zip(wanted.as_bool())
            .map(|(a, b)| a.cmp(&b)),
        TypeRef::String => actual.as_str().zip(wanted.as_str()).map(|(a, b)| a.cmp(b)),
        TypeRef::Uuid => actual
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .zip(wanted.as_str().and_then(|s| Uuid::parse_str(s).ok()))
            .map(|(a, b)| a.cmp(&b)),
        TypeRef::U64 => actual.as_u64().zip(wanted.as_u64()).map(|(a, b)| a.cmp(&b)),
        TypeRef::I64 => actual.as_i64().zip(wanted.as_i64()).map(|(a, b)| a.cmp(&b)),
        _ => None,
    }
    .ok_or_else(|| RecordQueryError::Failed("stored field violates registered type".into()))?;
    let match_result = match predicate.operator {
        RecordCompare::Eq => ord.is_eq(),
        RecordCompare::Ne => !ord.is_eq(),
        RecordCompare::Gt => ord.is_gt(),
        RecordCompare::Ge => ord.is_ge(),
        RecordCompare::Lt => ord.is_lt(),
        RecordCompare::Le => ord.is_le(),
        _ => unreachable!("presence operators were handled above"),
    };
    Ok(match_result)
}

/// Provider-instance-local opaque continuation registry. A token is bound
/// to one exact logical request and is consumed on successful resume. Storing
/// the token *inside* the linked provider also fences the provider lifetime;
/// the application session independently invalidates escaped RPC handles.
///
/// This is a reusable mechanism for record providers, not a query executor or
/// an authorization credential for independent client processes.
#[derive(Default)]
pub struct RecordQueryCursors {
    entries: std::collections::BTreeMap<Uuid, RecordContinuation>,
}

struct RecordContinuation {
    signature: RecordQueryRequest,
    after: Uuid,
    created: std::time::Instant,
}

const MAX_ACTIVE_QUERY_CURSORS: usize = 256;
const QUERY_CURSOR_TTL: std::time::Duration = std::time::Duration::from_secs(300);

impl RecordQueryCursors {
    fn expire(&mut self) {
        self.entries
            .retain(|_, continuation| continuation.created.elapsed() <= QUERY_CURSOR_TTL);
    }

    /// Return the last examined key for an existing matching token, consuming
    /// the token only when the complete query identity matches.
    pub fn resume(
        &mut self,
        request: &RecordQueryRequest,
    ) -> Result<Option<Uuid>, RecordQueryError> {
        let Some(token) = &request.cursor else {
            return Ok(None);
        };
        let token = Uuid::parse_str(token).map_err(|_| RecordQueryError::InvalidCursor)?;
        self.expire();
        let previous = self
            .entries
            .get(&token)
            .ok_or(RecordQueryError::InvalidCursor)?;
        let mut expected = request.clone();
        expected.cursor = None;
        if previous.signature != expected {
            return Err(RecordQueryError::InvalidCursor);
        }
        let last = previous.after;
        self.entries.remove(&token);
        Ok(Some(last))
    }

    /// Create one finite-lifetime opaque continuation for a partial scan.
    pub fn issue(
        &mut self,
        request: &RecordQueryRequest,
        after: Uuid,
    ) -> Result<String, RecordQueryError> {
        self.expire();
        if self.entries.len() >= MAX_ACTIVE_QUERY_CURSORS {
            return Err(RecordQueryError::CursorCapacity);
        }
        let mut signature = request.clone();
        signature.cursor = None;
        for _ in 0..8 {
            let token = Uuid::new_v4();
            if self.entries.contains_key(&token) {
                continue;
            }
            self.entries.insert(
                token,
                RecordContinuation {
                    signature,
                    after,
                    created: std::time::Instant::now(),
                },
            );
            return Ok(token.to_string());
        }
        Err(RecordQueryError::CursorCapacity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riesel_schema::OperationKind;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn definition() -> ModelDefinition {
        ModelDefinition {
            fields: BTreeMap::from([
                ("completed".into(), TypeRef::Bool),
                ("big".into(), TypeRef::U64),
                ("delta".into(), TypeRef::I64),
                ("external_id".into(), TypeRef::Uuid),
                ("name".into(), TypeRef::String),
                (
                    "note".into(),
                    TypeRef::Optional {
                        item: Box::new(TypeRef::String),
                    },
                ),
                ("dynamic".into(), TypeRef::Json),
            ]),
        }
    }

    fn pred(field: &str, op: RecordCompare, value: Option<Value>) -> RecordPredicate {
        RecordPredicate {
            field: field.into(),
            operator: op,
            value,
        }
    }

    fn request(predicate: Option<RecordPredicate>) -> RecordQueryRequest {
        RecordQueryRequest {
            model: "asset".into(),
            predicate,
            limit: 32,
            scan_budget: 32,
            cursor: None,
        }
    }

    #[test]
    fn generated_facets_are_read_only_and_versioned_separately() {
        let contract = RecordQueryContract::definition().unwrap();
        assert_eq!(contract.capability.id, "riesel.query.records");
        assert_eq!(contract.capability.version, 1);
        assert_eq!(contract.operations.len(), 1);
        assert_eq!(contract.operations["query_page"].kind, OperationKind::Query);
        assert!(
            contract
                .operations
                .values()
                .all(|op| op.kind == OperationKind::Query)
        );
    }

    #[test]
    fn exact_large_numbers_uuid_and_optional_presence_are_not_coerced() {
        let m = definition();
        let id = Uuid::new_v4();
        let rec = json!({"completed":false, "big":u64::MAX, "delta":i64::MIN,
                         "external_id":id.to_string().to_uppercase(), "name":"Asset", "note":null});
        for (predicate, matches) in [
            (pred("big", RecordCompare::Eq, Some(json!(u64::MAX))), true),
            (
                pred("big", RecordCompare::Eq, Some(json!(u64::MAX - 1))),
                false,
            ),
            (
                pred("big", RecordCompare::Gt, Some(json!(u64::MAX - 1))),
                true,
            ),
            (
                pred("delta", RecordCompare::Eq, Some(json!(i64::MIN))),
                true,
            ),
            (
                pred(
                    "external_id",
                    RecordCompare::Eq,
                    Some(json!(id.to_string())),
                ),
                true,
            ),
            (pred("note", RecordCompare::IsNull, None), true),
            (pred("note", RecordCompare::IsMissing, None), false),
            (
                pred("completed", RecordCompare::Eq, Some(json!(false))),
                true,
            ),
            (
                pred("completed", RecordCompare::Eq, Some(json!(true))),
                false,
            ),
        ] {
            validate_record_query(&m, &request(Some(predicate.clone()))).unwrap();
            assert_eq!(
                record_matches_predicate(&rec, &m, Some(&predicate)).unwrap(),
                matches
            );
        }
        let without_note = json!({"completed":false, "big":u64::MAX, "delta":i64::MIN,
            "external_id":id.to_string(), "name":"Asset"});
        assert!(
            record_matches_predicate(
                &without_note,
                &m,
                Some(&pred("note", RecordCompare::IsMissing, None))
            )
            .unwrap()
        );
        assert!(
            !record_matches_predicate(
                &without_note,
                &m,
                Some(&pred("note", RecordCompare::Ne, Some(json!("x"))))
            )
            .unwrap()
        );
    }

    #[test]
    fn type_mismatch_unsupported_operation_and_limits_fail_before_scan() {
        let m = definition();
        for p in [
            pred("completed", RecordCompare::Gt, Some(json!(false))),
            pred("big", RecordCompare::Eq, Some(json!(-1))),
            pred("note", RecordCompare::IsMissing, Some(json!(42))),
            pred("completed", RecordCompare::IsNull, None),
            pred("dynamic", RecordCompare::Eq, Some(json!(7))),
            pred("name", RecordCompare::Eq, Some(Value::Null)),
        ] {
            assert!(validate_record_query(&m, &request(Some(p))).is_err());
        }
        for invalid in [
            pred("big", RecordCompare::Eq, Some(json!(-1))),
            pred("completed", RecordCompare::Eq, Some(json!("false"))),
            pred("external_id", RecordCompare::Eq, Some(json!("not-a-uuid"))),
        ] {
            assert!(matches!(
                validate_record_query(&m, &request(Some(invalid))),
                Err(RecordQueryError::InvalidOperand)
            ));
        }
        assert!(matches!(
            validate_record_query(
                &m,
                &request(Some(pred(
                    "completed",
                    RecordCompare::Gt,
                    Some(json!(false))
                )))
            ),
            Err(RecordQueryError::UnsupportedPredicate)
        ));
        let mut q = request(None);
        q.limit = 33;
        assert!(matches!(
            validate_record_query(&m, &q),
            Err(RecordQueryError::InvalidLimit)
        ));
        q.limit = 1;
        q.scan_budget = 257;
        assert!(matches!(
            validate_record_query(&m, &q),
            Err(RecordQueryError::InvalidScanBudget)
        ));
    }
    #[test]
    fn cursor_input_signatures_have_explicit_finite_memory_bounds() {
        let definition = definition();
        let mut query = request(Some(pred(
            "name",
            RecordCompare::Eq,
            Some(json!("x".repeat(MAX_QUERY_STRING_OPERAND_BYTES + 1))),
        )));
        assert!(matches!(
            validate_record_query(&definition, &query),
            Err(RecordQueryError::RequestTooLarge)
        ));
        query.predicate.as_mut().unwrap().value =
            Some(json!("x".repeat(MAX_QUERY_STRING_OPERAND_BYTES)));
        validate_record_query(&definition, &query).unwrap();
        query.predicate.as_mut().unwrap().field = "x".repeat(MAX_QUERY_IDENTIFIER_BYTES + 1);
        assert!(matches!(
            validate_record_query(&definition, &query),
            Err(RecordQueryError::RequestTooLarge)
        ));
        query.predicate = None;
        query.model = "m".repeat(MAX_QUERY_IDENTIFIER_BYTES + 1);
        assert!(matches!(
            validate_record_query_envelope(&query),
            Err(RecordQueryError::RequestTooLarge)
        ));
    }

    #[test]
    fn continuations_bind_exact_query_and_owner_instance() {
        let mut cursors = RecordQueryCursors::default();
        let after = Uuid::new_v4();
        let request = request(Some(pred("big", RecordCompare::Eq, Some(json!(u64::MAX)))));
        let token = cursors.issue(&request, after).unwrap();
        let mut changed = request.clone();
        changed.cursor = Some(token.clone());
        changed.model = "different".into();
        assert!(matches!(
            cursors.resume(&changed),
            Err(RecordQueryError::InvalidCursor)
        ));

        let mut resumed = request.clone();
        resumed.cursor = Some(token.clone());
        assert_eq!(cursors.resume(&resumed).unwrap(), Some(after));
        assert!(matches!(
            cursors.resume(&resumed),
            Err(RecordQueryError::InvalidCursor)
        ));
        let mut different_owner = RecordQueryCursors::default();
        assert!(matches!(
            different_owner.resume(&resumed),
            Err(RecordQueryError::InvalidCursor)
        ));
    }

    #[test]
    fn abandoned_cursor_state_is_bounded_and_expired() {
        let mut cursors = RecordQueryCursors::default();
        let request = request(None);
        for _ in 0..MAX_ACTIVE_QUERY_CURSORS {
            cursors.issue(&request, Uuid::new_v4()).unwrap();
        }
        assert!(matches!(
            cursors.issue(&request, Uuid::new_v4()),
            Err(RecordQueryError::CursorCapacity)
        ));
        for entry in cursors.entries.values_mut() {
            entry.created =
                std::time::Instant::now() - QUERY_CURSOR_TTL - std::time::Duration::from_secs(1);
        }
        cursors.issue(&request, Uuid::new_v4()).unwrap();
        assert_eq!(cursors.entries.len(), 1);
    }
}
