extern crate libc;
extern crate time;

use super::Strategy;
use super::super::os::Memory;
use super::super::{RootWalker, GcTypes, GcTypeLayout, GcOpts, get_header_mut, PTR_SIZE};
use libc::c_void;
use std::ptr;
use std::mem;

const PAGE_SIZE : usize = 4 * 1024;

struct Header {
	forward: *const c_void,
	size: usize
}

const HEADER_SIZE : usize = PTR_SIZE * 3;

impl Header {
	fn new(size: usize) -> Header {
		Header {
			forward: ptr::null(),
			size: size
		}
	}
}

struct Block {
	memory: Memory,
	offset: usize
}

impl Block {
	unsafe fn alloc(&mut self, size: usize) -> *mut c_void {
		let size = size + HEADER_SIZE;
		
		if self.offset + size > self.memory.size() {
			return ptr::null_mut();
		}
		
		let memory = self.memory.ptr().offset(self.offset as isize);
		
		(*(memory as *mut Header)) = Header::new(size);
		
		self.offset += size;
		
		memory.offset(HEADER_SIZE as isize)
	}
}

pub struct Copying {
	opts: GcOpts,
	from: Block,
	to: Memory,
	last_used: f64,
	last_failed: usize
}

impl Copying {
	pub fn new(opts: GcOpts) -> Copying {
		let memory = Memory::alloc(opts.initial_heap).unwrap();
		
		Copying {
			opts: opts,
			from: Block {
				memory: memory,
				offset: 0
			},
			to: Memory::empty(),
			last_used: 0f64,
			last_failed: 0
		}
	}
	
	unsafe fn copy(&mut self, types: &GcTypes, mut walker: RootWalker) {
		struct Forwarder {
			target: *mut c_void
		}
		
		impl Forwarder {
			unsafe fn forward(&mut self, ptr: *const c_void) -> *const c_void {
				let header = &mut *(ptr.offset(-(HEADER_SIZE as isize)) as *mut Header);
				
				if header.forward.is_null() {
					header.forward = self.target;
					
					(*(self.target as *mut Header)) = Header::new(header.size);
					
					ptr::copy(ptr, self.target.offset(HEADER_SIZE as isize), header.size - HEADER_SIZE);
					
					self.target = self.target.offset(header.size as isize);
				}
				
				header.forward.offset(HEADER_SIZE as isize)
			}
		}
		
		let allocated = self.from.offset;
		
		// Calculate the new size of the heap. We use the fill factor of the previous
		// run as a basis and ensure that we have at least enough room to accept the
		// allocation that failed last (were we not able to reclaim any memory).
		//
		// The room we allocate comes down to the current allocated memory times the
		// fill factor times the growth factor. The growth factor is taken from
		// the configuration. 
		
		let growth_factor = if self.last_used > 0.8 {
			self.opts.fast_growth_factor
		} else {
			self.opts.slow_growth_factor
		};
		
		let mut target_size = self.from.offset + self.last_failed;
		self.last_failed = 0;
		
		if self.last_used > 0f64 {
			target_size = (target_size as f64 * self.last_used) as usize
		}
		
		if target_size < self.opts.initial_heap {
			target_size = self.opts.initial_heap;
		}
		
		target_size = (target_size as f64 * growth_factor) as usize;
		target_size = (target_size + (PAGE_SIZE - 1)) & !(PAGE_SIZE - 1);
		
		self.last_failed = 0;
		
		// Ensure that the target heap is large enough.
		
		if self.to.size() < target_size {
			// First set to empty to first release our allocated memory.
			self.to = Memory::empty();
			self.to = Memory::alloc(target_size).unwrap();
		}
		
		let mut forwarder = Forwarder {
			target: self.to.ptr()
		};
		
		loop {
			let ptr = walker.next();
			if ptr.is_null() {
				break;
			}
			
			walker.rewrite(forwarder.forward(ptr));
		}
		
		let mut ptr = self.to.ptr().offset(HEADER_SIZE as isize);
		
		while ptr < forwarder.target {
			let header = &mut *(ptr.offset(-(HEADER_SIZE as isize)) as *mut Header);
			 
			let gc_header = get_header_mut(ptr);
			let type_ = &types.types[gc_header.get_type_id().usize()];
			let size = type_.size;
			
			match type_.layout {
				GcTypeLayout::None => {},
				GcTypeLayout::Bitmap(bitmap) => {
					let mut offset = (ptr as *mut *const c_void).offset(1);
	
					for i in 0..(size / PTR_SIZE) {
						let child = *offset;
						
						if bitmap & (1 << i) != 0 && !child.is_null() {
							*offset = forwarder.forward(child);
						}
						
						offset = offset.offset(1);
					}
				}
				_ => panic!()
			}
			
			ptr = ptr.offset(header.size as isize);
		}
		
		self.from.offset = forwarder.target as usize - self.to.ptr() as usize;
		mem::swap(&mut self.from.memory, &mut self.to);
		
		self.last_used = self.from.offset as f64 / allocated as f64;
	}
}

impl Strategy for Copying {
	unsafe fn alloc_raw(&mut self, size: usize) -> *mut c_void {
		// Round the size to the next pointer.
		let size = (size + (PTR_SIZE - 1)) & !(PTR_SIZE - 1);
		
		let result = self.from.alloc(size);
		
		if result.is_null() {
			self.last_failed = size;
		} else {
			ptr::write_bytes(result, 0, size);
		}
		
		result
	}
	
	fn mem_allocated(&self) -> usize {
		self.from.memory.size() + self.to.size()
	}
	
	fn mem_used(&self) -> usize {
		self.from.offset
	}
	
	fn gc(&mut self, types: &GcTypes, walker: RootWalker) {
		let start = time::precise_time_ns();
		
		unsafe {
			self.copy(types, walker);
		}
		
		let elapsed = (time::precise_time_ns() - start) / 1_000_000;

		// println!("=== GC === allocated {} used {} ms {}", self.mem_allocated(), self.mem_used(), elapsed);
	}
}
