pub mod compacting;

extern crate libc;

use libc::c_void;

pub trait Strategy {
	unsafe fn alloc_raw(&mut self, size: usize) -> *mut c_void;
}
