//! Research-only app-process facade over a new component instance.
//! The resolved-handle comparator is in counter_resolved_facade_host.rs.
#[path = "facade_host_common.rs"]
mod facade_host_common;

use riesel::Application;
use riesel_counter::AsyncCounterAsyncRpcDispatcher;
use riesel_counter_component::AsyncMemoryCounter;
use std::sync::Arc;

fn main() -> riesel::Result<()> {
    Application::new("PublicFacadeResearch")
        .async_handler(|| async move {
            // Explicit route selection: the component also implements
            // Observer, but only AsyncCounter is hosted on this endpoint.
            let handler = Arc::new(AsyncCounterAsyncRpcDispatcher::new(
                AsyncMemoryCounter::default(),
            ));
            facade_host_common::serve(handler).await
        })?
        .run()
}
