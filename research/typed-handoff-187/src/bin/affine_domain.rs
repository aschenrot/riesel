//! #187 — real Riesel outer operation delegates to a local !Send owner.
//!
//! Test-only proof: Rc<RefCell<_>> remains on a Tokio LocalSet owner;
//! Riesel's Send-compatible async operation exchanges only a bounded,
//! Send-safe mailbox request + acknowledgement. Not proof of native GPU,
//! ECS or OS-main-thread admission, external authority, or S3 data ports.
#![forbid(unsafe_code)]

use riesel::{
    Application, AsyncOperation, ExecutionPlanDraft, OperationKey, OperationNode,
    OperationOutcome, OperationRegistry, async_operation,
};
use std::{
    cell::RefCell,
    io,
    num::NonZeroUsize,
    rc::Rc,
};
use tokio::sync::{mpsc, oneshot};

fn run_owner_mailbox(attach_owner: bool) -> riesel::Result<bool> {
    Application::new("Research 187 Native-Affine Owner").run(|context| {
        // Bounded mailbox: a request carries only a Send-safe one-shot
        // acknowledgement. No Rc, RefCell or GPU/native pointer crosses.
        let (tx, mut requests) = mpsc::channel::<oneshot::Sender<usize>>(1);
        let key = OperationKey::new("research.affine_domain_tick", 1)?;
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry.register(
            key.clone(),
            async_operation({
                let tx = tx.clone();
                move || {
                    let tx = tx.clone();
                    async move {
                        let (reply_tx, reply_rx) = oneshot::channel();
                        if tx.send(reply_tx).await.is_err() {
                            return OperationOutcome::Failed(
                                "domain owner mailbox unavailable".into()
                            );
                        }
                        match reply_rx.await {
                            Ok(1) => OperationOutcome::Completed,
                            Ok(_) => OperationOutcome::Failed(
                                "owner acknowledged unexpected state".into()
                            ),
                            Err(_) => OperationOutcome::Failed(
                                "domain owner left without acknowledgement".into()
                            ),
                        }
                    }
                }
            }),
        )?;
        drop(tx); // Only the operation handler retains a mailbox sender.
        let plan = ExecutionPlanDraft::new("research.187.local-affine-owner", 1)
            .operation(OperationNode::new("outer-tick", key)
                .delegated_frame("research-affine-owner"))
            .validate()?
            .bind(&registry)?;
        let runner = context.execution_graph(plan, NonZeroUsize::new(1).unwrap())?;
        let tokio = tokio::runtime::Builder::new_current_thread()
            .enable_all().build()?;
        let local_set = tokio::task::LocalSet::new();
        let owner_data = Rc::new(RefCell::new(0usize)); // !Send on purpose
        let captured = Rc::clone(&owner_data);
        let report = tokio.block_on(local_set.run_until(async move {
            if attach_owner {
                let owner = tokio::task::spawn_local(async move {
                    if let Some(response) = requests.recv().await {
                        *captured.borrow_mut() += 1;
                        let _ = response.send(*captured.borrow());
                    }
                });
                let report = runner.execute().await;
                owner.await.expect("bounded owner task must terminate");
                report
            } else {
                // Simulate owner shutdown before operation dispatch.
                drop(requests);
                runner.execute().await
            }
        }));
        if report.succeeded() != attach_owner {
            return Err(io::Error::other(format!(
                "owner availability not reflected in Riesel result: {:?}",
                report.operations
            )).into());
        }
        if *owner_data.borrow() != usize::from(attach_owner) {
            return Err(io::Error::other(
                "native-affine owner value not preserved locally"
            ).into());
        }
        println!(
            "AFFINE_DOMAIN owner_attached={} succeeded={} local_mutations={}",
            attach_owner, report.succeeded(), *owner_data.borrow(),
        );
        Ok(report.succeeded())
    })
}

fn main() -> riesel::Result<()> {
    if !run_owner_mailbox(true)? || run_owner_mailbox(false)? {
        return Err(io::Error::other("domain delegation proof failed").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_graph_admits_outer_op_without_moving_non_send_owner_state() {
        assert!(run_owner_mailbox(true).unwrap());
    }

    #[test]
    fn missing_affine_owner_fails_in_actual_graph_without_side_effect() {
        assert!(!run_owner_mailbox(false).unwrap());
    }
}
