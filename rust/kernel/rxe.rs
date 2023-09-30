// SPDX-License-Identifier: GPL-2.0

//! Infiniband soft-Roce devices.
use alloc::boxed::Box;
use core::pin::Pin;
use core::{marker, ptr};
use macros::vtable;

use crate::error::{code::*, Error, Result};
use crate::str::CStr;
use crate::{bindings, pr_err, pr_info};

/// Soft-Roce transport registration.
///
pub struct Registration<T: RxeOperation> {
    registered: bool,
    #[allow(dead_code)]
    name: &'static CStr,
    net_socket: RxeRecvSockets<T>,
    rxe_link_ops: bindings::rdma_link_ops,
    phantom: marker::PhantomData<T>,
}

impl<T: RxeOperation> Registration<T> {
    /// Creates a new [`Registration`] but does not register it yet.
    ///
    /// It is allowed to move.
    pub fn new(name: &'static CStr) -> Self {
        // INVARIANT: `registered` is `false`
        Self {
            registered: false,
            name,
            net_socket: RxeRecvSockets::new(),
            rxe_link_ops: bindings::rdma_link_ops::default(),
            phantom: marker::PhantomData,
        }
    }

    /// Registers a infiniband soft-Roce device
    /// Returns a pinned heap-allocated representation of the registration.
    pub fn new_pinned(name: &'static CStr) -> Result<Pin<Box<Self>>> {
        let mut r = Pin::from(Box::try_new(Self::new(name))?);
        r.as_mut().register()?;
        Ok(r)
    }

    /// Registers a infiband soft-Roce device with the rest of the kernel.
    ///
    /// It must be pinned because the memory block that represents the registration is
    /// self-referential.
    pub fn register(self: Pin<&mut Self>) -> Result {
        // SAFETY: We must ensure that we never move out of 'this'.
        let this = unsafe { self.get_unchecked_mut() };
        if this.registered {
            // Already registered
            return Err(EINVAL);
        }

        match this.net_socket.alloc() {
            Ok(()) => {}
            Err(e) => return Err(e),
        }

        this.rxe_link_ops = RxeRdmaLinkTable::<T>::build();

        // SAFETY: The adapter is compatible with the rdma_link_register
        unsafe {
            bindings::rdma_link_register(&mut this.rxe_link_ops);
        }

        this.registered = true;
        pr_info!("loaded");
        Ok(())
    }
}

impl<T: RxeOperation> Drop for Registration<T> {
    fn drop(&mut self) {
        if self.registered {
            // SAFETY: [`self.rxe_link_ops`] was previously created using RxeRdmaLinkTable::<T>::build()
            unsafe { bindings::rdma_link_unregister(&mut self.rxe_link_ops) };
            // SAFETY: unregister ib driver with driver_id bindings::rdma_driver_id_RDMA_DRIVER_RXE
            unsafe { bindings::ib_unregister_driver(bindings::rdma_driver_id_RDMA_DRIVER_RXE) };
        }
    }
}

// SAFETY: `Registration` does not expose any of its state across threads
// (it is fine for multiple threads to have a shared reference to it).
unsafe impl<T: RxeOperation> Sync for Registration<T> {}

/// soft-Roce register net sockets
pub struct RxeRecvSockets<T: RxeOperation> {
    sk4: Option<*mut bindings::socket>,
    sk6: Option<*mut bindings::socket>,
    rxe_net_notifier: Option<bindings::notifier_block>,
    phantom: marker::PhantomData<T>,
}

impl<T: RxeOperation> RxeRecvSockets<T> {
    /// Create net socket but not init it yet.
    pub fn new() -> Self {
        Self {
            sk4: None,
            sk6: None,
            rxe_net_notifier: None,
            phantom: marker::PhantomData,
        }
    }

    /// Init rxe net socket
    pub fn alloc(&mut self) -> Result<()> {
        match self.ipv4_init() {
            Ok(_tmp) => {}
            Err(e) => return Err(e),
        }

        match self.ipv6_init() {
            Ok(_tmp) => {}
            Err(e) => {
                self.rxe_net_release();
                return Err(e);
            }
        }

        match self.net_notifier_register() {
            Ok(_tmp) => {}
            Err(e) => {
                self.rxe_net_release();
                return Err(e);
            }
        }
        Ok(())
    }

    /// Init ipv4 socket
    fn ipv4_init(&mut self) -> Result<()> {
        let mut udp_cfg = bindings::udp_port_cfg::default();
        let mut tnl_cfg = bindings::udp_tunnel_sock_cfg::default();
        let mut sock: *mut bindings::socket = ptr::null_mut();

        udp_cfg.family = bindings::AF_INET as u8;
        udp_cfg.local_udp_port = 46866;
        // SAFETY: [`bindings::init_net`] and [`udp_cfg`] can be safely passed to [`bindings::udp_sock_create4`]
        // [`sock`] will be pass to [`self.sk4`] later, it will live at least as long as the module, which is an implicit requirement
        let err =
            unsafe { bindings::udp_sock_create4(&mut bindings::init_net, &mut udp_cfg, &mut sock) };

        if err < 0 {
            pr_err!("Failed to create IPv4 UDP tunnel\n");
            return Err(Error::from_kernel_errno(err));
        }

        tnl_cfg.encap_type = 1;
        tnl_cfg.encap_rcv = RxeUdpEncapRecvFuncTable::<T>::build_func();

        // SAFETY: [`bindings::init_net`] and [`tnl_cfg`] can be safely passed to [`bindings::setup_udp_tunnel_sock`]
        // [`sock`] will be pass to [`self.sk4`] later, it will live at least as long as the module, which is an implicit requirement
        unsafe { bindings::setup_udp_tunnel_sock(&mut bindings::init_net, sock, &mut tnl_cfg) }
        self.sk4 = Some(sock);
        Ok(())
    }

    /// if CONFIG_IPV6=y, init ipv6 socket
    fn ipv6_init(&mut self) -> Result<()> {
        #[cfg(CONFIG_IPV6)]
        {
            let mut udp_cfg = bindings::udp_port_cfg::default();
            let mut tnl_cfg = bindings::udp_tunnel_sock_cfg::default();
            let mut sock: *mut bindings::socket = ptr::null_mut();

            udp_cfg.family = bindings::AF_INET6 as u8;
            udp_cfg.set_ipv6_v6only(1);
            udp_cfg.local_udp_port = 46866;
            // SAFETY: [`bindings::init_net`] and [`udp_cfg`] can be safely passed to [`bindings::udp_sock_create4`]
            // [`sock`] will be pass to [`self.sk6`] later, it will live at least as long as the module, which is an implicit requirement
            let err = unsafe {
                bindings::udp_sock_create6(&mut bindings::init_net, &mut udp_cfg, &mut sock)
            };

            if err < 0 {
                // EAFNOSUPPORT
                if err == -97 {
                    pr_err!("IPv6 is not supported, can not create a UDPv6 socket\n");
                    return Ok(());
                } else {
                    pr_err!("Failed to create IPv6 UDP tunnel\n");
                    return Err(Error::from_kernel_errno(err));
                }
            }

            tnl_cfg.encap_type = 1;
            tnl_cfg.encap_rcv = RxeUdpEncapRecvFuncTable::<T>::build_func();

            // SAFETY: [`bindings::init_net`] and [`tnl_cfg`] can be safely passed to [`bindings::setup_udp_tunnel_sock`]
            // [`sock`] will be pass to [`self.sk6`] later, it will live at least as long as the module, which is an implicit requirement
            unsafe { bindings::setup_udp_tunnel_sock(&mut bindings::init_net, sock, &mut tnl_cfg) }
            self.sk6 = Some(sock);
        }
        Ok(())
    }

    /// Rxe receive notifier info and handle func
    fn net_notifier_register(&mut self) -> Result<()> {
        let err: i32;
        self.rxe_net_notifier = Some(RxeNotifyFuncTable::<T>::build());
        // SAFETY: [`self.rxe_net_notifier`] is Some, it was previously created by
        // RxeNotifyFuncTable::<T>::build().
        unsafe {
            err = bindings::register_netdevice_notifier(self.rxe_net_notifier.as_mut().unwrap());
        }
        if err != 0 {
            pr_err!("Failed to register netdev notifier\n");
            if self.rxe_net_notifier.is_some() {
                // SAFETY: [`self.rxe_net_notifier`] is Some, it was previously created by
                // RxeNotifyFuncTable::<T>::build().
                unsafe {
                    bindings::unregister_netdevice_notifier(
                        &mut self.rxe_net_notifier.take().unwrap(),
                    )
                };
            }
            return Err(Error::from_kernel_errno(err));
        }
        Ok(())
    }

    /// release registered socket when error occur
    fn rxe_net_release(&mut self) {
        if self.sk4.is_some() {
            // SAFETY: [`self.sk4`] is Some, it was previously created in ipv4_init(&mut self).
            unsafe {
                bindings::udp_tunnel_sock_release(self.sk4.take().unwrap());
            }
        }
        if self.sk6.is_some() {
            // SAFETY: [`self.sk6`] is Some, it was previously created in ipv6_init(&mut self).
            unsafe {
                bindings::udp_tunnel_sock_release(self.sk6.take().unwrap());
            }
        }
    }
}

impl<T: RxeOperation> Drop for RxeRecvSockets<T> {
    /// Removes the registration from the kernel if it has completed successfully before.
    fn drop(&mut self) {
        self.rxe_net_release();
        if self.rxe_net_notifier.is_some() {
            // SAFETY: [`self.rxe_net_notifier`] is Some, it was previously created by
            // RxeNotifyFuncTable::<T>::build().
            unsafe {
                bindings::unregister_netdevice_notifier(&mut self.rxe_net_notifier.take().unwrap());
            };
        }
    }
}

// SAFETY: `Registration` does not expose any of its state across threads
// (it is fine for multiple threads to have a shared reference to it).
unsafe impl<T: RxeOperation> Sync for RxeRecvSockets<T> {}

/// Implement this trait to complete the function.
#[vtable]
pub trait RxeOperation {
    /// notify() corresponds to the kernel's rxe_notify.
    fn notify() -> Result;
    /// newlink() corresponds to the kernel's rxe_newlink.
    fn newlink() -> Result;
    /// udp_recv() implement skb reception processing.
    fn udp_recv() -> Result;
}

///Build kernel's 'struct notifier_block' type with rxe device operation
struct RxeNotifyFuncTable<T>(marker::PhantomData<T>);

impl<T: RxeOperation> RxeNotifyFuncTable<T> {
    /// Builds an instance of [`struct notifier_block`].
    ///
    /// # Safety
    ///
    /// The caller must ensure that the adapter is compatible with the way the device is registered.
    pub(crate) fn build() -> bindings::notifier_block {
        Self::NOTIFYFUNC
    }

    const NOTIFYFUNC: bindings::notifier_block = bindings::notifier_block {
        notifier_call: Some(Self::rxe_notify),
        next: ptr::null_mut(),
        priority: 0,
    };

    unsafe extern "C" fn rxe_notify(
        _not_blk: *mut bindings::notifier_block,
        _event: core::ffi::c_ulong,
        _arg: *mut core::ffi::c_void,
    ) -> core::ffi::c_int {
        let _ = T::notify();
        return 0;
    }
}

/// Build kernel's 'struct rxe_link_ops' type with rxe device operation
struct RxeRdmaLinkTable<T>(marker::PhantomData<T>);

impl<T: RxeOperation> RxeRdmaLinkTable<T> {
    /// Builds an instance of [`struct rxe_link_ops`].
    ///
    /// # Safety
    ///
    /// The caller must ensure that the adapter is compatible with the way the device is registered.
    pub(crate) fn build() -> bindings::rdma_link_ops {
        Self::RXELINKFUNC
    }

    const RXELINKFUNC: bindings::rdma_link_ops = bindings::rdma_link_ops {
        type_: "rxe".as_ptr() as *const i8,
        newlink: Some(Self::rxe_newlink),
        list: bindings::list_head {
            next: ptr::null_mut(),
            prev: ptr::null_mut(),
        },
    };

    unsafe extern "C" fn rxe_newlink(
        _ibdev_name: *const core::ffi::c_char,
        _ndev: *mut bindings::net_device,
    ) -> core::ffi::c_int {
        let _ = T::newlink();
        return 0;
    }
}

/// Build kernel's rxe_udp_encap_recv function  
struct RxeUdpEncapRecvFuncTable<T>(marker::PhantomData<T>);

impl<T: RxeOperation> RxeUdpEncapRecvFuncTable<T> {
    /// # Safety
    ///
    /// The caller must ensure that the adapter is compatible with the way the device is registered.
    pub(crate) fn build_func() -> Option<
        unsafe extern "C" fn(
            sk: *mut bindings::sock,
            skb: *mut bindings::sk_buff,
        ) -> core::ffi::c_int,
    > {
        Some(Self::rxe_udp_encap_recv)
    }
    unsafe extern "C" fn rxe_udp_encap_recv(
        _sk: *mut bindings::sock,
        _skb: *mut bindings::sk_buff,
    ) -> core::ffi::c_int {
        let _ = T::udp_recv();
        return 0;
    }
}
