extern crate libc;

use super::Strategy;
use super::super::os::Memory;
use super::super::{RootWalker, GcTypes, GcTypeLayout, GcMemHeader, get_header_mut};
use libc::c_void;
use std::ptr;
use std::mem;

static WORD_SIZE : usize = 8;

// We can't use size_of in a static so we use conditional compilation instead.

#[cfg(target_arch = "x86")]
const PTR_SIZE : usize = 4;
#[cfg(target_arch = "x86_64")]
const PTR_SIZE : usize = 8;

const BITMAP_BITS : usize = PTR_SIZE * 8;
const BITMAP_SIZE : usize = 256 / BITMAP_BITS;

#[inline(always)]
unsafe fn get_bitmap_mut(memory: &Memory, offset: usize) -> &mut usize {
	&mut *(memory.ptr().offset((offset * PTR_SIZE) as isize) as *mut usize)
}

unsafe fn block_offset(block_size: usize, bitmap_index: usize, bitmap_bit: usize) -> usize {
	(bitmap_index * BITMAP_BITS + bitmap_bit) * block_size * WORD_SIZE
}

struct Bucket {
	block_size: usize,
	memory: Vec<Memory>,
	free: Vec<usize>
}

impl Bucket {
	unsafe fn alloc(&mut self, stats: &mut Stats) -> *mut c_void {
		// Find a free memory block with enough blocks available, or create a new one.
		
		let mut bitmap_index = 0;
		let mut bitmap_bit = 0;
		
		if self.free.len() > 0 {
			let memory = &self.memory[self.free[self.free.len() - 1]];
			
			'outer: for i in 0..BITMAP_SIZE {
				let bitmap = get_bitmap_mut(memory, i);
				
				if !*bitmap != 0 {
					for j in 0..BITMAP_BITS {
						if *bitmap & (1 << j) == 0 {
							bitmap_index = i;
							bitmap_bit = j;
							break 'outer;
						}
					}
				}
			}
		} else {
			self.free.push(self.memory.len());
			let mem_size = block_offset(self.block_size, BITMAP_SIZE, 0);
			let memory = Memory::alloc(mem_size).unwrap();
			
			stats.allocated += mem_size;
			
			// Reserve the bitmap as allocated.
			
			*get_bitmap_mut(&memory, 0) |= 1;
			bitmap_bit = 1;

			self.memory.push(memory);
		}
		
		assert!(!(bitmap_index == 0 && bitmap_bit == 0));
		
		// Allocate the bit in the selected memory block.
		
		let memory = &self.memory[self.free[self.free.len() - 1]];
		
		let bitmap = get_bitmap_mut(memory, bitmap_index);
		*bitmap |= 1 << bitmap_bit;
		
		stats.used += self.block_size * WORD_SIZE;
		
		let result = memory.ptr().offset(block_offset(self.block_size, bitmap_index, bitmap_bit) as isize);
		
		// Remove the block from the free list if it's full (i.e. we're in the last
		// index of the bitmap and the bitmap is full).
		
		let mut any_free = false;
		
		'outer: for i in 0..BITMAP_SIZE {
			let bitmap = get_bitmap_mut(memory, i);
			if !*bitmap != 0 {
				any_free = true;
				break;
			}
		}
		
		if !any_free {
			self.free.pop();
		}
		
		result
	}
}

pub struct MarkSweep {
	buckets: Vec<Bucket>,
	stats: Stats
}

struct Stats {
	allocated: usize,
	used: usize
}

impl MarkSweep {
	pub fn new() -> MarkSweep {
		// We use with word sizes of 8 bytes. Bucketes are created for
		// word sizes 4, 8, 16, 32, 64, 128 and 256.
		
		let mut buckets = Vec::new();
		
		let mut block_size = 4;
		
		while block_size <= 256 {
			buckets.push(Bucket {
				block_size: block_size,
				memory: Vec::new(),
				free: Vec::new()
			});
			
			block_size *= 2;
		}
		
		MarkSweep {
			buckets: buckets,
			stats: Stats {
				allocated: 0,
				used: 0
			}
		}
	}

	fn alloc_large(&mut self, _: usize) -> *mut c_void {
		panic!();
	}
	
	unsafe fn gc_mark(&mut self, types: &GcTypes, mut walker: RootWalker) {
		let mut queue = Vec::new();
		
		loop {
			let ptr = walker.next();
			if ptr == ptr::null() {
				break;
			}
			
//			println!("mark 1");
			get_header_mut(ptr).set_marked();
			queue.push(ptr);
		}
		
//		println!("queue {}", queue.len());
		
		while let Some(ptr) = queue.pop() {
			let header = get_header_mut(ptr);
			let type_ = &types.types[header.get_type_id().usize()];
			
			match type_.layout {
				GcTypeLayout::None => {},
				GcTypeLayout::Bitmap(bitmap) => mark_bitmap(&mut queue, ptr, bitmap, type_.size),
				_ => panic!()
			}
		}
	}
	
	unsafe fn gc_sweep(&mut self) {
		for bucket in &mut self.buckets {
			let alloc_size = bucket.block_size * WORD_SIZE;
			
			for i in 0..bucket.memory.len() {
				let memory = &bucket.memory[i];
				
				let mut ptr = memory.ptr();
				let mut was_free = false;
				let mut any_free = false;
				
				for j in 0..BITMAP_SIZE {
					let bitmap = get_bitmap_mut(memory, j);
					
					// Check whether this block should be on the free list.
					
					if !*bitmap != 0 {
						was_free = true;
					}
					
					for k in 0..BITMAP_BITS {
						// Is this part allocated?
						
						if !(j == 0 && k == 0) && *bitmap & (1 << k) != 0 {
							let header = get_header_mut(ptr);
							
							// If this part is marked, clear the bit. Otherwise
							// mark it as free.
							
							if header.get_marked() {
//								println!("clearing {} {} {}", i, j, k);
								header.clear_marked();
							} else {
//								println!("freeing {} {} {}", i, j, k);
								*bitmap &= !(1 << k);
								self.stats.used -= alloc_size;
								any_free = true;
							}
						} else {
//							println!("free {} {} {} {}", i, j, k, *bitmap);
						}

						ptr = ptr.offset(alloc_size as isize);
					}
				}
				
				// If this block became free, put it on the free list.
				
				if !was_free && any_free {
//					println!("new free");
					bucket.free.push(i);
				}
			}
		}
	}
}

unsafe fn mark_bitmap(queue: &mut Vec<*const c_void>, ptr: *const c_void, bitmap: u64, size: usize) {
	let mut offset = (ptr as *const *const c_void).offset(1);
	
	for i in 0..(size / PTR_SIZE) {
		let child = *offset;
		
		if bitmap & (1 << i) != 0 && child != ptr::null() {
			let header = get_header_mut(child);
			if !header.get_marked() {
//				println!("mark 2");
				header.set_marked();
				queue.push(ptr);
			}
		}
		
		offset = offset.offset(1);
	}
}

impl Strategy for MarkSweep {
	unsafe fn alloc_raw(&mut self, size: usize) -> *mut c_void {
		let words = (size + WORD_SIZE - 1) / WORD_SIZE;
		
		let bucket_index = match words {
			0...4 => 0,
			5...8 => 1,
			9...16 => 2,
			17...32 => 3,
			33...64 => 4,
			65...128 => 5,
			129...256 => 6,
			_ => return self.alloc_large(size)
		};
		
		self.buckets[bucket_index].alloc(&mut self.stats)
	}
	
	fn mem_allocated(&self) -> usize {
		self.stats.allocated
	}
	
	fn mem_used(&self) -> usize {
		self.stats.used
	}
	
	fn gc(&mut self, types: &GcTypes, walker: RootWalker) {
		unsafe {
			self.gc_mark(types, walker);
			self.gc_sweep();
		}
	}
}
