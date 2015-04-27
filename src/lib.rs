extern crate libc;
extern crate time;

// We can't use size_of in a static so we use conditional compilation instead.

#[cfg(target_pointer_width = "32")]
const PTR_SIZE : usize = 4;
#[cfg(target_pointer_width = "64")]
const PTR_SIZE : usize = 8;

use std::ops::{Deref, DerefMut, Index, IndexMut};
use std::marker::PhantomData;
use std::ptr;
use std::mem;
use std::mem::size_of;
use std::cell::RefCell;
use std::slice;
use self::strategy::Strategy;
use self::strategy::copying::Copying;
use self::libc::c_void;

mod os;
mod strategy;

#[macro_export]
macro_rules! field_offset {
	( $ty:ty, $ident:ident ) => {
		unsafe { ((&(& *(std::ptr::null::<$ty>() as *const $ty)).$ident) as *const _) as usize }
	}
}

#[inline(always)]
unsafe fn get_data<'a, T : ?Sized>(ptr: *const c_void) -> &'a T {
	& *(ptr.offset(size_of::<GcMemHeader>() as isize) as *const T)
}

#[inline(always)]
unsafe fn get_header_mut<'a>(ptr: *const c_void) -> &'a mut GcMemHeader {
	mem::transmute(ptr)
}

unsafe fn get_array_size_mut<'a>(ptr: *const c_void) -> &'a mut usize {
	&mut *(ptr.offset(size_of::<GcMemHeader>() as isize) as *mut usize)
}

#[inline(always)]
unsafe fn get_data_mut<'a, T : ?Sized>(ptr: *const c_void) -> &'a mut T {
	&mut *(ptr.offset(size_of::<GcMemHeader>() as isize) as *mut T)
}

#[derive(Copy, Clone)]
pub struct GcRoot(u32);

impl GcRoot {
	fn u32(&self) -> u32 {
		let GcRoot(index) = *self;
		index
	}
	
	pub unsafe fn as_handle<'a, T>(self, heap: &'a GcHeap) -> Gc<'a, T> {
		Gc {
			handles: &*heap.handles,
			handle: self,
			_type: PhantomData
		}
	}
	
	pub unsafe fn as_new_handle<'a, T>(self, heap: &'a GcHeap) -> Gc<'a, T> {
		Gc {
			handles: &*heap.handles,
			handle: heap.handles.clone_root(self),
			_type: PhantomData
		}
	}
	
	pub unsafe fn as_vec_handle<'a, T>(self, heap: &'a GcHeap) -> GcVec<'a, T> {
		GcVec {
			handles: &*heap.handles,
			handle: self,
			_type: PhantomData
		}
	}
	
	pub unsafe fn as_new_vec_handle<'a, T>(self, heap: &'a GcHeap) -> GcVec<'a, T> {
		GcVec {
			handles: &*heap.handles,
			handle: heap.handles.clone_root(self),
			_type: PhantomData
		}
	}
	
	pub unsafe fn deref<T>(self, heap: &GcHeap) -> &T {
		mem::transmute((&*heap.handles).deref(self))
	}
	
	pub unsafe fn deref_mut<T>(self, heap: &GcHeap) -> &mut T {
		mem::transmute((&*heap.handles).deref(self))
	}
}

pub struct Gc<'a, T> {
	handles: &'a GcHandles,
	handle: GcRoot,
	_type: PhantomData<T>
}

impl<'a, T> Gc<'a, T> {
	pub fn as_ptr(&self) -> GcPtr<T> {
		GcPtr {
			ptr: self.handles.data.borrow().ptrs[self.handle.u32() as usize],
			_type: PhantomData
		}
	}
}

impl<'a, T> Clone for Gc<'a, T> {
	fn clone(&self) -> Gc<'a, T> {
		self.handles.clone(self)
	}
}

impl<'a, T> Deref for Gc<'a, T> {
	type Target = T;
	
	fn deref(&self) -> &T {
		unsafe { mem::transmute(self.handles.deref(self.handle)) }
	}
}

impl<'a, T> DerefMut for Gc<'a, T> {
	fn deref_mut(&mut self) -> &mut T {
		unsafe { mem::transmute(self.handles.deref(self.handle)) }
	}
}

impl<'a, T> Drop for Gc<'a, T> {
	fn drop(&mut self) {
		self.handles.remove(self.handle);
	}
}

#[allow(raw_pointer_derive)]
pub struct GcPtr<T> {
	ptr: *const c_void,
	_type: PhantomData<T>
}

impl<T> GcPtr<T> {
	pub unsafe fn usize(&self) -> usize {
		self.ptr as usize
	}
	
	pub unsafe fn from_usize(ptr: usize) -> GcPtr<T> {
		GcPtr {
			ptr: ptr as *const c_void,
			_type: PhantomData
		}
	}
}

impl<T> Copy for GcPtr<T> { }

impl<T> Clone for GcPtr<T> {
	fn clone(&self) -> GcPtr<T> {
		GcPtr {
			ptr: self.ptr,
			_type: PhantomData
		}
	}
}

impl<T> Deref for GcPtr<T> {
	type Target = T;
	
	fn deref(&self) -> &T {
		unsafe { get_data(self.ptr) }
	}
}

impl<T> DerefMut for GcPtr<T> {
	fn deref_mut(&mut self) -> &mut T {
		unsafe { get_data_mut(self.ptr) }
	}
}

pub struct GcVec<'a, T> {
	handles: &'a GcHandles,
	handle: GcRoot,
	_type: PhantomData<T>
}

impl<'a, T> GcVec<'a, T> {
	pub fn len(&self) -> usize {
		unsafe { *mem::transmute::<*const c_void, *const usize>(self.ptr()) }
	}
	
	pub fn as_ptr(&self) -> GcVecPtr<T> {
		GcVecPtr {
			ptr: self.handles.data.borrow().ptrs[self.handle.u32() as usize],
			_type: PhantomData
		}
	}
	
	unsafe fn ptr(&self) -> *const c_void {
		mem::transmute(self.handles.deref(self.handle))
	}
}

impl<'a, T> Index<usize> for GcVec<'a, T> {
	type Output = T;
	
	fn index<'b>(&self, index: usize) -> &'b T {
		if index >= self.len() {
			panic!("Index out of bounds");
		}
		
		let offset = size_of::<usize>() + (size_of::<T>() * index);
		
		unsafe { & *(self.ptr().offset(offset as isize) as *const T) }
	}
}

impl<'a, T> IndexMut<usize> for GcVec<'a, T> {
	fn index_mut<'b>(&mut self, index: usize) -> &'b mut T {
		let offset = size_of::<usize>() + (size_of::<T>() * index);
		
		unsafe { &mut *(self.ptr().offset(offset as isize) as *mut T) }
	}
}

#[allow(raw_pointer_derive)]
pub struct GcVecPtr<T> {
	ptr: *const c_void,
	_type: PhantomData<T>
}

impl<T> GcVecPtr<T> {
	pub unsafe fn usize(&self) -> usize {
		self.ptr as usize
	}
	
	pub unsafe fn from_usize(ptr: usize) -> GcVecPtr<T> {
		GcVecPtr {
			ptr: ptr as *const c_void,
			_type: PhantomData
		}
	}
}

impl<T> Copy for GcVecPtr<T> { }

impl<T> Clone for GcVecPtr<T> {
	fn clone(&self) -> GcVecPtr<T> {
		GcVecPtr {
			ptr: self.ptr,
			_type: PhantomData
		}
	}
}

impl<T> GcVecPtr<T> {
	pub fn len(&self) -> usize {
		unsafe { *(self.ptr.offset(size_of::<GcMemHeader>() as isize) as *const usize) }
	}
	
	pub unsafe fn as_slice(&self) -> &[T] {
		slice::from_raw_parts(
			self.ptr.offset((size_of::<GcMemHeader>() + size_of::<usize>()) as isize) as *const T,
			self.len()
		)
	}
	
	pub unsafe fn as_slice_mut(&self) -> &mut [T] {
		slice::from_raw_parts_mut(
			self.ptr.offset((size_of::<GcMemHeader>() + size_of::<usize>()) as isize) as *mut T,
			self.len()
		)
	}
}

impl<T> Index<usize> for GcVecPtr<T> {
	type Output = T;
	
	fn index<'a>(&self, index: usize) -> &'a T {
		if index >= self.len() {
			panic!("Index out of bounds");
		}
		
		let offset =
			size_of::<GcMemHeader>() +
			size_of::<usize>() +
			(size_of::<T>() * index);
		
		unsafe { & *(self.ptr.offset(offset as isize) as *const T) }
	}
}

impl<T> IndexMut<usize> for GcVecPtr<T> {
	fn index_mut<'a>(&mut self, index: usize) -> &'a mut T {
		let offset =
			size_of::<GcMemHeader>() +
			size_of::<usize>() +
			(size_of::<T>() * index);
		
		unsafe { &mut *(self.ptr.offset(offset as isize) as *mut T) }
	}
}

pub struct GcOpts {
	pub initial_heap: usize,
	pub slow_growth_factor: f64,
	pub fast_growth_factor: f64
}

impl GcOpts {
	pub fn default() -> GcOpts {
		GcOpts {
			initial_heap: 16 * 1024 * 1024, // 16M
			slow_growth_factor: 1.5f64,
			fast_growth_factor: 3f64
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
	layout: GcTypeLayout
}

impl GcType {
	pub fn new(size: usize, layout: GcTypeLayout) -> GcType {
		GcType {
			size: size,
			layout: layout
		}
	}
}

pub enum GcTypeLayout {
	None,
	Bitmap(u64),
	Callback(Box<Fn(*const c_void, u32) -> GcTypeWalk>)
}

impl GcTypeLayout {
	pub fn new_bitmap(size: usize, ptrs: Vec<usize>) -> GcTypeLayout {
		// The bitmap is stored in an u64. This means we have 64 bits available.
		// The bitmap is a bitmap of pointers, so this maps to size / sizeof(ptr).
		// Assert that the size of the struct does not go over this.
		
		assert!(size / size_of::<usize>() <= size_of::<u64>() * 8);
		
		let mut bitmap = 0u64;
		
		for ptr in ptrs {
			// ptr is a byte offset of the field into the structure. The bitmap is
			// based on pointer size offsets, so we need to divide ptr by the pointer
			// size. The bitmap itself is a n u64 so we have 64 bits available,
			// which means we have room for 64 pointers per index.
			
			assert!((ptr % size_of::<usize>()) == 0);
			
			bitmap |= 1u64 << (ptr / size_of::<usize>());
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
	data: RefCell<GcHandlesData>
}

struct GcHandlesData {
	ptrs: Vec<*const c_void>,
	free: Vec<u32>
}

impl GcHandles {
	fn new() -> GcHandles {
		GcHandles {
			data: RefCell::new(GcHandlesData {
				ptrs: Vec::new(),
				free: Vec::new()
			})
		}
	}
	
	fn add(&self, ptr: *const c_void) -> GcRoot {
		let mut data = self.data.borrow_mut();
		
		let index = if let Some(index) = data.free.pop() {
			assert_eq!(data.ptrs[index as usize], ptr::null_mut());
			
			data.ptrs[index as usize] = ptr;
			index
		} else {
			let index = data.ptrs.len() as u32;
			data.ptrs.push(ptr);
			index
		};
		
		GcRoot(index)
	}
	
	fn remove(&self, handle: GcRoot) {
		let mut data = self.data.borrow_mut();
		
		data.free.push(handle.u32());
		data.ptrs[handle.u32() as usize] = ptr::null_mut();
	}
	
	fn clone_root(&self, handle: GcRoot) -> GcRoot {
		let data = self.data.borrow_mut();
		self.add(data.ptrs[handle.u32() as usize])
	}
	
	fn clone<'a, T>(&self, gc: &Gc<'a, T>) -> Gc<'a, T> {
		let data = self.data.borrow_mut();
		let handle = self.add(data.ptrs[gc.handle.u32() as usize]);
		
		Gc {
			handles: gc.handles,
			handle: handle,
			_type: PhantomData
		}
	}
	
	unsafe fn deref(&self, handle: GcRoot) -> *const c_void {
		self.data.borrow().ptrs[handle.u32() as usize].offset(size_of::<GcMemHeader>() as isize)
	}
}

pub struct GcHeap {
	types: GcTypes,
	handles: Box<GcHandles>,
	heap: RefCell<Copying>
}

struct GcMemHeader {
	header: usize
}

impl GcMemHeader {
	fn new(type_id: GcTypeId, is_array: bool) -> GcMemHeader {
		let mut header = type_id.usize() << 1;
		if is_array {
			header |= 1;
		}
		
		GcMemHeader {
			header: header
		}
	}
	
	#[inline(always)]
	fn get_type_id(&self) -> GcTypeId {
		GcTypeId((self.header >> 1) as u32)
	}
	
	fn is_array(&self) -> bool {
		self.header & 1 != 0
	}
}

impl GcHeap {
	pub fn new(opts: GcOpts) -> GcHeap {
		if opts.fast_growth_factor <= 1f64 {
			panic!("fast_growth_factor must be more than 1");
		}
		if opts.slow_growth_factor <= 1f64 {
			panic!("slow_growth_factor must be more than 1");
		}
		
		GcHeap {
			types: GcTypes::new(),
			handles: Box::new(GcHandles::new()),
			heap: RefCell::new(Copying::new(opts))
		}
	}
	
	pub fn types(&mut self) -> &mut GcTypes {
		&mut self.types
	}
	
	unsafe fn alloc_raw(&self, size: usize) -> *mut c_void {
		let mut ptr = self.heap.borrow_mut().alloc_raw(size);
		if ptr.is_null() {
			self.gc();
			
			ptr = self.heap.borrow_mut().alloc_raw(size);
			if ptr.is_null() {
				panic!("Could not allocate memory after GC");
			}
		}
		
		ptr
	}
	
	pub fn alloc<T>(&self, type_id: GcTypeId) -> GcPtr<T> {
		unsafe {
			let ptr = self.alloc_raw(
				self.types.get(type_id).size +
				size_of::<GcMemHeader>()
			);
			
			*get_header_mut(ptr) = GcMemHeader::new(type_id, false);
			
			GcPtr {
				ptr: ptr,
				_type: PhantomData
			}
		}
	}
	
	pub unsafe fn alloc_root<T>(&self, type_id: GcTypeId) -> GcRoot {
		self.handles.add(self.alloc::<T>(type_id).ptr)
	}
	
	pub fn alloc_handle<T>(&self, type_id: GcTypeId) -> Gc<T> {
		unsafe { self.alloc_root::<T>(type_id).as_handle(self) }
	}
	
	pub fn alloc_array<T>(&self, type_id: GcTypeId, size: usize) -> GcVecPtr<T> {
		unsafe {
			let ptr = self.alloc_raw(
				PTR_SIZE +
				(self.types.get(type_id).size * size) +
				size_of::<GcMemHeader>()
			);
			
			*get_header_mut(ptr) = GcMemHeader::new(type_id, true);
			*get_array_size_mut(ptr) = size;
			
			GcVecPtr {
				ptr: ptr,
				_type: PhantomData
			}
		}
	}
	
	pub unsafe fn alloc_array_root<T>(&self, type_id: GcTypeId, size: usize) -> GcRoot {
		self.handles.add(self.alloc_array::<T>(type_id, size).ptr)
	}
	
	pub fn alloc_array_handle<T>(&self, type_id: GcTypeId, size: usize) -> GcVec<T> {
		unsafe { self.alloc_array_root::<T>(type_id, size).as_vec_handle(self) }
	}
	
	pub fn gc(&self) {
		self.heap.borrow_mut().gc(&self.types, RootWalker {
			handles: &mut self.handles.data.borrow_mut(),
			offset: 0
		});
	}
	
	pub fn mem_allocated(&self) -> usize {
		self.heap.borrow().mem_allocated()
	}
	
	pub fn mem_used(&self) -> usize {
		self.heap.borrow().mem_used()
	}
}

struct RootWalker<'a> {
	handles: &'a mut GcHandlesData,
	offset: usize
}

impl<'a> RootWalker<'a> {
	fn next(&mut self) -> *const c_void {
		let end = self.handles.ptrs.len();
		while self.offset < end {
			let ptr = self.handles.ptrs[self.offset];
			self.offset += 1;
			
			if !ptr.is_null() {
				return ptr;
			}
		}
		
		ptr::null()
	}
	
	fn rewrite(&mut self, ptr: *const c_void) {
		self.handles.ptrs[self.offset - 1] = ptr as *mut c_void;
	}
}
