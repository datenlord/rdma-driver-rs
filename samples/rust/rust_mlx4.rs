// SPDX-License-Identifier: GPL-2.0

//! Rust infiniband mls4 device sample.

use kernel::mlx4;
use kernel::prelude::*;

module! {
    type: RustMlx4,
    name: "rust_mlx4",
    author: "Rust for Linux Contributors",
    description: "Rust infiniband mlx4 device sample",
    license: "GPL",
}

struct RustMlx4Ops;

#[vtable]
impl mlx4::Mlx4Operation for RustMlx4Ops {
    fn add() -> Result {
        Ok(())
    }
    fn remove() -> Result {
        Ok(())
    }
    fn event() -> Result {
        Ok(())
    }
}

struct RustMlx4 {
    _dev: Pin<Box<mlx4::Registration<RustMlx4Ops>>>,
}

impl kernel::Module for RustMlx4 {
    fn init(name: &'static CStr, _module: &'static ThisModule) -> Result<Self> {
        pr_info!("Rust infiniband mlx4 driver sample (init)\n");

        Ok(RustMlx4 {
            _dev: mlx4::Registration::new_pinned(name)?,
        })
    }
}

impl Drop for RustMlx4 {
    fn drop(&mut self) {
        pr_info!("Rust infiniband mlx4 driver sample (exit)\n");
    }
}
