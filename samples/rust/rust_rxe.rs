// SPDX-License-Identifier: GPL-2.0

//! Rust infiniband Soft-RoCE driver sample.

use kernel::prelude::*;
use kernel::rxe;

module! {
    type: RustRxe,
    name: "rust_rxe",
    author: "Rust for Linux Contributors",
    description: "Rust infiniband soft-Roce driver sample",
    license: "GPL",
}

struct RustRxeOps;

#[vtable]
impl rxe::RxeOperation for RustRxeOps {
    fn notify() -> Result {
        Ok(())
    }
    fn newlink() -> Result {
        Ok(())
    }
    fn udp_recv() -> Result {
        Ok(())
    }
}

struct RustRxe {
    _dev: Pin<Box<rxe::Registration<RustRxeOps>>>,
}

impl kernel::Module for RustRxe {
    fn init(name: &'static CStr, _module: &'static ThisModule) -> Result<Self> {
        pr_info!("Rust Soft-RoCE driver sample (init)\n");

        Ok(RustRxe {
            _dev: rxe::Registration::<RustRxeOps>::new_pinned(name)?,
        })
    }
}

impl Drop for RustRxe {
    fn drop(&mut self) {
        pr_info!("Rust Soft-RoCE driver sample (exit)\n");
    }
}
