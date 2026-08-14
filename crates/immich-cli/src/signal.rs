use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use immich_rs_core::CancellationToken;

use crate::failure::CliFailure;

const CANCELLED_EXIT: u8 = 130;

pub fn install(cancellation: CancellationToken) -> Result<(), CliFailure> {
    let interrupts = Arc::new(AtomicU8::new(0));
    let handler_interrupts = Arc::clone(&interrupts);
    ctrlc::set_handler(move || {
        let previous = handler_interrupts.fetch_add(1, Ordering::AcqRel);
        if previous == 0 {
            cancellation.cancel();
        } else {
            std::process::exit(i32::from(CANCELLED_EXIT));
        }
    })
    .map_err(|_| CliFailure::invariant("cannot install cancellation handler"))
}
