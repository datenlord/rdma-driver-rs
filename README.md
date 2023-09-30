# rdma-driver-rs
Currently provides the basic architecture and security layer abstraction of Soft-RoCE rxe driver and the InfiniBand mlx4 driver.

Provides two driver samples.

Add the files of this project in the corresponding folder of Rust for Linux's main branch 'rust'.

Add the following content to rust/kernel/lib
```rust
pub mod mlx4;
pub mod rxe;
```

Add the following content to samples/rust/Kconfig
```
config SAMPLE_RUST_RXE
	tristate "Soft-Roce"
	help
	  This option builds the self test cases for Rust.

	  If unsure, say N.

config SAMPLE_RUST_MLX4
	tristate "infiniband mlx4"
	help
	  This option builds the infiniband mlx4 driver cases for Rust.

	  If unsure, say N.
```

Add the following content to samples/rust/Makefile
```Makefile
obj-$(CONFIG_SAMPLE_RUST_RXE)		+= rust_rxe.o
obj-$(CONFIG_SAMPLE_RUST_MLX4)		+= rust_mlx4.o
```

Enable the CONFIG of the corresponding sample during compilation of the Linux kernel.Run the newly compiled kernel along with the samples that are included in it. 
