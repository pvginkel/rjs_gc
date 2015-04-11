use std::ops::{Deref, DerefMut};
use std::marker::PhantomData;
use std::ptr;
use std::mem;
use strategy::Strategy;
use strategy::compacting::Compacting;

mod os;
mod strategy;

extern crate libc;

unsafe fn as_mut<T>(obj: &T) -> &mut T {
	&mut *((obj as *const T) as *mut T)
}

unsafe fn get_header<'a>(ptr: *const libc::c_void) -> &'a GcMemHeader {
	mem::transmute(ptr)
}

unsafe fn get_data<'a, T : ?Sized>(ptr: *const libc::c_void) -> &'a T {
	& *(ptr.offset(mem::size_of::<GcMemHeader>() as isize) as *const T)
}

unsafe fn get_header_mut<'a>(ptr: *mut libc::c_void) -> &'a mut GcMemHeader {
	mem::transmute(ptr)
}

unsafe fn get_data_mut<'a, T : ?Sized>(ptr: *mut libc::c_void) -> &'a mut T {
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
	Bitmap(Vec<u32>),
	Callback(Box<Fn(usize, u32) -> GcTypeWalk>)
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
	memory: os::Memory,
	gen1: Compacting
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
	
	fn get_marked(&self) -> bool {
		(self.header & 0x1) != 0
	}
	
	fn set_marked(&mut self, marked: bool) {
		if marked {
			self.header |= 0x1;
		} else {
			self.header & !0x1;
		}
	}
	
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
			memory: os::Memory::alloc(4096).unwrap(),
			gen1: Compacting::new()
		}
	}
	
	pub fn types(&mut self) -> &mut GcTypes {
		&mut self.types
	}
	
	unsafe fn alloc_raw(&mut self, size: usize) -> *mut libc::c_void {
		let ptr = self.gen1.alloc_raw(size);
		if ptr.is_null() {
			panic!();
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
}
