pub mod marksweep;

extern crate libc;

use libc::c_void;
use super::{RootWalker, GcTypes};

pub trait Strategy {
	unsafe fn alloc_raw(&mut self, size: usize) -> *mut c_void;
	
	fn mem_allocated(&self) -> usize;
	
	fn mem_used(&self) -> usize;
	
	fn gc(&mut self, types: &GcTypes, walker: RootWalker);
}
