static INITIAL: usize = 128 * 4096;

extern crate libc;

use super::Strategy;
use super::super::os::Memory;
use libc::c_void;
use std::ptr;

pub struct Compacting {
	memory: Memory,
	offset: usize
}

impl Compacting {
	pub fn new() -> Compacting {
		Compacting {
			memory: Memory::alloc(INITIAL).unwrap(),
			offset: 0
		}
	}
}

impl Strategy for Compacting {
	unsafe fn alloc_raw(&mut self, size: usize) -> *mut c_void {
		// If we have enough room left, calculate the pointer and bump
		// the offset. Otherwise return nullptr.
		
		if self.offset + size > self.memory.size() {
			ptr::null_mut()
		} else {
			let ptr = self.memory.ptr().offset(self.offset as isize);
			
			self.offset += size;
			
			ptr
		}
	}
}
