const MAX_TIME : u64 = 200 * 1000000; // 100 ms
const MAX_ALLOCS : usize = 1000;

use std::ops::{Deref, DerefMut};
use std::marker::PhantomData;
use std::ptr;
use std::mem;
use strategy::Strategy;
use strategy::marksweep::MarkSweep;
use time::precise_time_ns;

mod os;
mod strategy;

extern crate libc;
extern crate time;

#[macro_export]
macro_rules! field_offset {
	( $ty:ty, $ident:ident ) => {
		unsafe { ((&(& *(std::ptr::null::<$ty>() as *const $ty)).$ident) as *const GcPtr<_>) as usize }
	}
}

unsafe fn as_mut<T>(obj: &T) -> &mut T {
	&mut *((obj as *const T) as *mut T)
}

#[inline(always)]
unsafe fn get_header<'a>(ptr: *const libc::c_void) -> &'a GcMemHeader {
	mem::transmute(ptr)
}

#[inline(always)]
unsafe fn get_data<'a, T : ?Sized>(ptr: *const libc::c_void) -> &'a T {
	& *(ptr.offset(mem::size_of::<GcMemHeader>() as isize) as *const T)
}

#[inline(always)]
unsafe fn get_header_mut<'a>(ptr: *const libc::c_void) -> &'a mut GcMemHeader {
	mem::transmute(ptr)
}

#[inline(always)]
unsafe fn get_data_mut<'a, T : ?Sized>(ptr: *const libc::c_void) -> &'a mut T {
	&mut *(ptr.offset(mem::size_of::<GcMemHeader>() as isize) as *mut T)
}

pub struct Gc<'a, T: ?Sized> {
	owner: &'a GcHeap,
	handle: u32,
	_type: PhantomData<T>
}

impl<'a, T: ?Sized> Gc<'a, T> {
	pub fn as_ptr(&self) -> GcPtr<T> {
		panic!();
	}
}

impl<'a, T: ?Sized> Deref for Gc<'a, T> {
	type Target = T;
	
	fn deref(&self) -> &T {
		self.owner.handles.deref(self)
	}
}

impl<'a, T: ?Sized> DerefMut for Gc<'a, T> {
	fn deref_mut(&mut self) -> &mut T {
		self.owner.handles.deref_mut(self)
	}
}

impl<'a, T: ?Sized> Drop for Gc<'a, T> {
	fn drop(&mut self) {
		unsafe { as_mut(self.owner) }.handles.remove(self);
	}
}

#[derive(Copy, Clone)]
#[allow(raw_pointer_derive)]
pub struct GcPtr<T: ?Sized> {
	ptr: *mut libc::c_void,
	_type: PhantomData<T>
}

impl<T: ?Sized> Deref for GcPtr<T> {
	type Target = T;
	
	fn deref(&self) -> &T {
		unsafe { get_data(self.ptr) }
	}
}

impl<T: ?Sized> DerefMut for GcPtr<T> {
	fn deref_mut(&mut self) -> &mut T {
		unsafe { get_data_mut(self.ptr) }
	}
}

pub struct GcOpts {
	pub min_size: usize,
	pub max_size: usize
}

impl GcOpts {
	pub fn default() -> GcOpts {
		GcOpts {
			min_size: 0,
			max_size: 10 * 1024 * 1024
		}
	}
}

#[derive(Copy, Clone)]
pub struct GcTypeId(u32);

impl GcTypeId {
	fn usize(&self) -> usize {
		let GcTypeId(index) = *self;
		index as usize
	}
}

pub struct GcTypes {
	types: Vec<GcType>
}

impl GcTypes {
	fn new() -> GcTypes {
		GcTypes {
			types: Vec::new()
		}
	}
}

impl GcTypes {
	pub fn add(&mut self, type_: GcType) -> GcTypeId {
		let index = self.types.len() as u32;
		self.types.push(type_);
		GcTypeId(index)
	}
	
	pub fn get(&self, type_id: GcTypeId) -> &GcType {
		&self.types[type_id.usize()]
	}
}

pub struct GcType {
	size: usize,
	data: usize,
	layout: GcTypeLayout
}

impl GcType {
	pub fn new(size: usize, data: usize, layout: GcTypeLayout) -> GcType {
		GcType {
			size: size,
			data: data,
			layout: layout
		}
	}
}

pub enum GcTypeLayout {
	None,
	Bitmap(u64),
	Callback(Box<Fn(usize, u32) -> GcTypeWalk>)
}

impl GcTypeLayout {
	pub fn new_bitmap(size: usize, ptrs: Vec<usize>) -> GcTypeLayout {
		// The bitmap is stored in an u64. This means we have 64 bits available.
		// The bitmap is a bitmap of pointers, so this maps to size / sizeof(ptr).
		// Assert that the size of the struct does not go over this.
		
		assert!(size / mem::size_of::<usize>() <= mem::size_of::<u64>() * 8);
		
		let mut bitmap = 0u64;
		
		for ptr in ptrs {
			// ptr is a byte offset of the field into the structure. The bitmap is
			// based on pointer size offsets, so we need to divide ptr by the pointer
			// size. The bitmap itself is a n u64 so we have 64 bits available,
			// which means we have room for 64 pointers per index.
			
			assert!((ptr % mem::size_of::<usize>()) == 0);
			
			bitmap |= 1u64 << (ptr / mem::size_of::<usize>());
		}
		
		GcTypeLayout::Bitmap(bitmap)
	}
}

pub enum GcTypeWalk {
	Pointer,
	Skip,
	End
}

struct GcHandles {
	ptrs: Vec<*mut libc::c_void>,
	free: Vec<u32>
}

impl GcHandles {
	fn new() -> GcHandles {
		GcHandles {
			ptrs: Vec::new(),
			free: Vec::new()
		}
	}
	
	fn add<'a, T: ?Sized>(&mut self, heap: &'a GcHeap, ptr: *mut libc::c_void) -> Gc<'a, T> {
		let index = if let Some(index) = self.free.pop() {
			assert_eq!(self.ptrs[index as usize], ptr::null_mut());
			
			self.ptrs[index as usize] = ptr;
			index
		} else {
			let index = self.ptrs.len() as u32;
			self.ptrs.push(ptr);
			index
		};
		
		Gc {
			owner: heap,
			handle: index,
			_type: PhantomData
		}
	}
	
	fn remove<T: ?Sized>(&mut self, handle: &Gc<T>) {
		self.free.push(handle.handle);
		self.ptrs[handle.handle as usize] = ptr::null_mut();
	}
	
	fn deref<T: ?Sized>(&self, handle: &Gc<T>) -> &T {
		unsafe { get_data(self.ptrs[handle.handle as usize]) }
	}
	
	fn deref_mut<T: ?Sized>(&self, handle: &mut Gc<T>) -> &mut T {
		unsafe { get_data_mut(self.ptrs[handle.handle as usize]) }
	}
}

pub struct GcHeap {
	opts: GcOpts,
	types: GcTypes,
	handles: GcHandles,
	gen1: MarkSweep,
	allocs: usize,
	last_gc: u64
}

struct GcMemHeader {
	header: usize
}

impl GcMemHeader {
	fn new(type_id: GcTypeId) -> GcMemHeader {
		GcMemHeader {
			header: type_id.usize() << 1
		}
	}
	
	#[inline(always)]
	fn get_marked(&self) -> bool {
		(self.header & 0x1) != 0
	}
	
	#[inline(always)]
	fn set_marked(&mut self) {
		self.header |= 0x1;
	}
	
	fn clear_marked(&mut self) {
		self.header &= !0x1;
	}
	
	#[inline(always)]
	fn get_type_id(&self) -> GcTypeId {
		GcTypeId((self.header >> 1) as u32)
	}
}

impl GcHeap {
	pub fn new(opts: GcOpts) -> GcHeap {
		GcHeap {
			opts: opts,
			types: GcTypes::new(),
			handles: GcHandles::new(),
			gen1: MarkSweep::new(),
			allocs: 0,
			last_gc: precise_time_ns()
		}
	}
	
	pub fn types(&mut self) -> &mut GcTypes {
		&mut self.types
	}
	
	unsafe fn alloc_raw(&mut self, size: usize) -> *mut libc::c_void {
		self.allocs += 1;
		
		if self.allocs > MAX_ALLOCS {
			self.allocs = 0;
			
			let time = precise_time_ns();
			if time - self.last_gc > MAX_TIME {
				self.gc();
			}
		} 
		
		let ptr = self.gen1.alloc_raw(size);
		if ptr.is_null() {
			self.gc();
			
			let ptr = self.gen1.alloc_raw(size);
			if ptr.is_null() {
				panic!("Could not allocate memory after GC");
			}
		}
		
		ptr
	}
	
	pub fn alloc<T>(&self, type_id: GcTypeId, value: T) -> GcPtr<T> {
		unsafe {
			let ty = self.types.get(type_id);
			
			let ptr = as_mut(self).alloc_raw(ty.size + mem::size_of::<GcMemHeader>());
			
			let header = GcMemHeader::new(type_id);
			
			*get_header_mut(ptr) = header;
			*get_data_mut(ptr) = value;
			
			GcPtr {
				ptr: ptr,
				_type: PhantomData
			}
		}
	}
	
	pub fn alloc_handle<T>(&self, type_id: GcTypeId, value: T) -> Gc<T> {
		let pointer = self.alloc(type_id, value).ptr;
		unsafe { as_mut(self) }.handles.add(self, pointer)
	}
	
	pub fn gc(&self) {
		let heap = unsafe { as_mut(self) };
		
		heap.last_gc = precise_time_ns();
		heap.gen1.gc(&self.types, RootWalker {
			handles: &self.handles,
			offset: 0
		});
	}
	
	pub fn mem_allocated(&self) -> usize {
		self.gen1.mem_allocated()
	}
	
	pub fn mem_used(&self) -> usize {
		self.gen1.mem_used()
	}
}

struct RootWalker<'a> {
	handles: &'a GcHandles,
	offset: usize
}

impl<'a> RootWalker<'a> {
	fn next(&mut self) -> *const libc::c_void {
		for i in self.offset..self.handles.ptrs.len() {
			let ptr = self.handles.ptrs[i];
			if ptr != ptr::null_mut() {
				self.offset = i + 1;
				return ptr;
			}
		}
		
		ptr::null()
	}
}
