//! Research-only second comparator: export operations from an existing
//! application-resolved, session-scoped capability. The manual dispatcher
//! illustrates missing public-export plumbing; it is not an approved API.
#[path = "facade_host_common.rs"]
mod facade_host_common;

use riesel::schema::CapabilityContract;
use riesel::{Application, AsyncCapability};
use riesel_counter::{AsyncCounterContract, CounterError};
use riesel_counter_component::AsyncMemoryCounter;
use riesel_protocol::{AsyncRpcFuture, AsyncRpcHandler, CallError, RpcError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddArgs {
    amount: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LabelArgs {
    prefix: String,
}

fn decode<T: serde::de::DeserializeOwned>(payload: Value) -> Result<T, RpcError> {
    serde_json::from_value(payload).map_err(|error| RpcError::InvalidArguments(error.to_string()))
}

/// Preserve transport/session failures as RPC errors, unlike naive conversion
/// to a domain error which would erase stale-session/unknown-outcome semantics.
fn encode<T: Serialize>(outcome: Result<T, CallError<CounterError>>) -> Result<Value, RpcError> {
    let logical: Result<T, CounterError> = match outcome {
        Ok(value) => Ok(value),
        Err(CallError::Domain(error)) => Err(error),
        Err(CallError::Rpc(error)) => return Err(error),
    };
    serde_json::to_value(logical).map_err(|error| RpcError::Codec(error.to_string()))
}

struct ResolvedFacade {
    counter: AsyncCapability<AsyncCounterContract>,
    suppress_first_add_reply: AtomicBool,
}

impl AsyncRpcHandler for ResolvedFacade {
    fn contract(&self) -> Result<CapabilityContract, RpcError> {
        AsyncCounterContract::definition().map_err(|error| RpcError::Contract(error.to_string()))
    }

    fn dispatch<'a>(&'a self, operation: &'a str, payload: Value) -> AsyncRpcFuture<'a> {
        Box::pin(async move {
            match operation {
                "value" => {
                    let _: NoArgs = decode(payload)?;
                    encode(self.counter.value().await)
                }
                "add" => {
                    let args: AddArgs = decode(payload)?;
                    let outcome = self.counter.add(args.amount).await;
                    if args.amount > 0
                        && self
                            .suppress_first_add_reply
                            .swap(false, Ordering::AcqRel)
                    {
                        // After a real Riesel-resolved mutation returns, withhold
                        // the RPC response. Client timeout MUST mean unknown
                        // outcome, not permission to replay this operation.
                        std::future::pending::<Result<Value, RpcError>>().await
                    } else {
                        encode(outcome)
                    }
                }
                "label" => {
                    let args: LabelArgs = decode(payload)?;
                    encode(self.counter.label(&args.prefix).await)
                }
                _ => Err(RpcError::UnknownOperation(operation.to_owned())),
            }
        })
    }
}

fn main() -> riesel::Result<()> {
    let suppress_first_add_reply = std::env::var_os("RIESEL_RESEARCH_SUPPRESS_ADD_REPLY").is_some();
    Application::new("ResolvedFacadeResearch")
        .async_handler(
            move |counter: AsyncCapability<AsyncCounterContract>| async move {
                // Unlike the first comparator, these operations resolve through
                // Riesel's actual provider registry and active application session.
                facade_host_common::serve(Arc::new(ResolvedFacade {
                    counter: counter.clone(),
                    suppress_first_add_reply: AtomicBool::new(suppress_first_add_reply),
                }))
                .await?;
                let observed = counter.value().await.map_err(io::Error::other)?;
                if observed != 5 {
                    return Err(io::Error::other(format!(
                        "resolved application counter changed unexpectedly: {observed}"
                    )));
                }
                Ok::<(), io::Error>(())
            },
        )?
        .component::<AsyncMemoryCounter>()?
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_keeps_session_failure_distinct_from_domain_error() {
        let expired = encode::<u64>(Err(CallError::Rpc(RpcError::SessionExpired)));
        assert!(matches!(expired, Err(RpcError::SessionExpired)));

        let domain = encode::<u64>(Err(CallError::Domain(CounterError::ZeroIncrement))).unwrap();
        let decoded: Result<u64, CounterError> = serde_json::from_value(domain).unwrap();
        assert!(matches!(decoded, Err(CounterError::ZeroIncrement)));
    }
}
