// SPDX-License-Identifier: GPL-2.0

//! Infiniband mlx4 devices.
//!

use alloc::boxed::Box;
use core::pin::Pin;
use core::{marker, ptr};
use macros::vtable;

use crate::bindings;
use crate::error::{code::*, Result};
use crate::str::CStr;
use crate::workqueue::{BoxedQueue, Queue};

/// Infiband mlx4 device registration.
///
pub struct Registration<T: Mlx4Operation> {
    registered: bool,
    #[allow(dead_code)]
    name: &'static CStr,
    wq: Mlx4WorkQueue,
    cm_wq: CmWorkQueue,
    qp_wq: QpWorkQueue,
    mcg_wq: McgWorkQueue,
    phantom: marker::PhantomData<T>,
}

impl<T: Mlx4Operation> Registration<T> {
    /// Creates a new [`Registration`] but does not register it yet.
    ///
    /// It is allowed to move.
    pub fn new(name: &'static CStr) -> Self {
        // INVARIANT: `registered` is `false`
        Self {
            registered: false,
            name,
            wq: Mlx4WorkQueue::new(),
            cm_wq: CmWorkQueue::new(),
            qp_wq: QpWorkQueue::new(),
            mcg_wq: McgWorkQueue::new(),
            phantom: marker::PhantomData,
        }
    }

    /// Registers a infiband mlx4 device.
    ///
    /// Returns a pinned heap-allocated representation of the registration.
    pub fn new_pinned(name: &'static CStr) -> Result<Pin<Box<Self>>> {
        let mut r = Pin::from(Box::try_new(Self::new(name))?);
        r.as_mut().register()?;
        Ok(r)
    }

    // Registers a infiband mlx4 device with the rest of the kernel.
    ///
    /// It must be pinned because the memory block that represents the registration is
    /// self-referential.
    pub fn register(self: Pin<&mut Self>) -> Result {
        // SAFETY: We must ensure that we never move out of `this`.
        let this = unsafe { self.get_unchecked_mut() };
        if this.registered {
            // Already registered.
            return Err(EINVAL);
        }

        match this.wq.init() {
            Ok(()) => {}
            Err(e) => return Err(e),
        }

        match this.qp_wq.init() {
            Ok(()) => {}
            Err(e) => {
                this.wq.clean();
                return Err(e);
            }
        }

        match this.cm_wq.init() {
            Ok(()) => {}
            Err(e) => {
                this.wq.clean();
                this.qp_wq.clean();
                return Err(e);
            }
        }

        match this.mcg_wq.init() {
            Ok(()) => {}
            Err(e) => {
                this.wq.clean();
                this.cm_wq.clean();
                this.qp_wq.clean();
                return Err(e);
            }
        }

        // SAFETY: The adapter is compatible with the mlx4 register
        unsafe {
            bindings::mlx4_register_interface(Mlx4OperationTable::<T>::build());
        }

        this.registered = true;
        Ok(())
    }
}

impl<T: Mlx4Operation> Drop for Registration<T> {
    /// Removes the registration from the kernel if it has completed successfully before.
    fn drop(&mut self) {
        if self.registered {
            self.mcg_wq.clean();
            self.cm_wq.clean();
            self.qp_wq.clean();
            self.wq.clean();
        }
    }
}

/// Build kernel's `struct mlx4_interface` type with mlx4 device operation.
pub struct Mlx4OperationTable<T>(marker::PhantomData<T>);

impl<T: Mlx4Operation> Mlx4OperationTable<T> {
    /// Builds an instance of [`struct mlx4_interface`].
    ///
    /// # Safety
    ///
    /// The caller must ensure that the adapter is compatible with the way the device is registered.
    pub fn build() -> *mut bindings::mlx4_interface {
        return &mut bindings::mlx4_interface {
            add: Some(Self::add_callback),
            remove: Some(Self::remove_callback),
            event: Some(Self::event_callback),
            get_dev: None,
            activate: None,
            list: bindings::list_head {
                next: ptr::null_mut(),
                prev: ptr::null_mut(),
            },
            // MLX4_PROT_IB_IPV6
            protocol: 0,
            // MLX4_INTFF_BONDING
            flags: 1,
        };
    }

    unsafe extern "C" fn add_callback(_dev: *mut bindings::mlx4_dev) -> *mut core::ffi::c_void {
        let _ = T::add();
        return ptr::null_mut();
    }

    unsafe extern "C" fn remove_callback(
        _dev: *mut bindings::mlx4_dev,
        _context: *mut core::ffi::c_void,
    ) {
        let _ = T::remove();
    }

    unsafe extern "C" fn event_callback(
        _dev: *mut bindings::mlx4_dev,
        _context: *mut core::ffi::c_void,
        _event: bindings::mlx4_dev_event,
        _param: core::ffi::c_ulong,
    ) {
        let _ = T::event();
    }
}

/// Corresponds to the kernel's `struct mlx4_interface`.
///
/// You implement this trait whenever you would create a `struct mlx4_interface`.
#[vtable]
pub trait Mlx4Operation {
    /// Add a new mlx4 ib device.
    fn add() -> Result;
    /// Remove mlx4 ib device.
    fn remove() -> Result;
    /// Respond to specific mlx4 ib device event
    fn event() -> Result;
}

pub(crate) struct Mlx4WorkQueue {
    wq: Option<BoxedQueue>,
}

impl Mlx4WorkQueue {
    pub(crate) fn new() -> Self {
        Self { wq: None }
    }

    pub(crate) fn init(&mut self) -> Result {
        let wq_tmp = Queue::try_new(format_args!("mlx4_ib"), 655369, 1);
        self.wq = match wq_tmp {
            Ok(wq) => Some(wq),
            Err(e) => return Err(e),
        };

        Ok(())
    }

    pub(crate) fn clean(&mut self) {
        if self.wq.is_some() {
            drop(self.wq.take().unwrap());
        }
    }
}

pub(crate) struct CmWorkQueue {
    cm_wq: Option<BoxedQueue>,
}

impl CmWorkQueue {
    pub(crate) fn new() -> Self {
        Self { cm_wq: None }
    }

    pub(crate) fn init(&mut self) -> Result {
        let cm_wq_tmp = Queue::try_new(format_args!("mlx4_ib_cm"), 0, 0);
        self.cm_wq = match cm_wq_tmp {
            Ok(cm_wq) => Some(cm_wq),
            Err(e) => return Err(e),
        };

        Ok(())
    }

    pub(crate) fn clean(&mut self) {
        if self.cm_wq.is_some() {
            drop(self.cm_wq.take().unwrap());
        }
    }
}

pub(crate) struct McgWorkQueue {
    clean_wq: Option<BoxedQueue>,
}

impl McgWorkQueue {
    pub(crate) fn new() -> Self {
        Self { clean_wq: None }
    }

    pub(crate) fn init(&mut self) -> Result {
        let clean_wq_tmp = Queue::try_new(format_args!("mlx4_ib_mcg"), 655369, 1);
        self.clean_wq = match clean_wq_tmp {
            Ok(clean_wq) => Some(clean_wq),
            Err(e) => return Err(e),
        };

        Ok(())
    }

    pub(crate) fn clean(&mut self) {
        if self.clean_wq.is_some() {
            drop(self.clean_wq.take().unwrap());
        }
    }
}

pub(crate) struct QpWorkQueue {
    mlx4_ib_qp_event_wq: Option<BoxedQueue>,
}

impl QpWorkQueue {
    pub(crate) fn new() -> Self {
        Self {
            mlx4_ib_qp_event_wq: None,
        }
    }

    pub(crate) fn init(&mut self) -> Result {
        let mlx4_ib_qp_event_wq_tmp =
            Queue::try_new(format_args!("mlx4_ib_qp_event_wq"), 655361, 1);
        self.mlx4_ib_qp_event_wq = match mlx4_ib_qp_event_wq_tmp {
            Ok(mlx4_ib_qp_event_wq) => Some(mlx4_ib_qp_event_wq),
            Err(e) => return Err(e),
        };

        Ok(())
    }

    pub(crate) fn clean(&mut self) {
        if self.mlx4_ib_qp_event_wq.is_some() {
            drop(self.mlx4_ib_qp_event_wq.take().unwrap());
        }
    }
}
