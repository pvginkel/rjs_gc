#![allow(dead_code)]

#[macro_use]
extern crate rjs_gc;
extern crate time;
extern crate libc;

use rjs_gc::*;
use std::mem;

struct Stopwatch {
	started: u64
}

impl Stopwatch {
	fn new() -> Stopwatch {
		Stopwatch {
			started: time::precise_time_ns()
		}
	}
	
	fn elapsed(&self) -> u64 {
		time::precise_time_ns() - self.started
	}
	
	fn elapsed_ms(&self) -> f64 {
		self.elapsed() as f64 / 1_000_000_000f64
	}
}

struct MyStruct {
	a: i32,
	b: i32,
	c: i32
}

#[derive(Copy, Clone)]
struct MyStructWithRef {
	a: GcPtr<MyStruct>,
	b: GcPtr<MyStruct>
}

struct MyMaybeRef {
	is_ref: bool,
	value: usize
}

fn print_stats(heap: &GcHeap) { 
	println!("STATS: allocated {}, used {}", heap.mem_allocated(), heap.mem_used());
}

struct Types {
	id: GcTypeId,
	ref_id: GcTypeId,
	callback_id: GcTypeId
}

fn main() {
	bench("Callback type", &|| { callback_type() });
	bench("Arrays", &|| { arrays() });
	bench("Simple allocs", &|| { simple_allocs() });
	bench("Large allocs", &|| { large_allocs() });
	bench("Many allocs", &|| { many_allocs() });
}

fn arrays() {
	let (heap, types) = create_heap();
	
	let mut array = heap.alloc_array_handle::<MyStructWithRef>(types.ref_id, 10);
	
	for i in 0..array.len() {
		let mut result = heap.alloc_handle::<MyStructWithRef>(types.ref_id);
		
		result.a = alloc_struct(&heap, &types, 1, 2, 3);
		result.b = alloc_struct(&heap, &types, 4, 5, 6);
		
		array[i] = *result;
	}
	
	print_stats(&heap);
	
	heap.gc();
	
	print_stats(&heap);
	
	for i in 0..array.len() {
		let item = &array[i];
		
		assert_eq!(item.a.a + item.a.b + item.a.c + item.b.a + item.b.b + item.b.c, 21);
	}
}

fn create_heap() -> (GcHeap, Types) {
	let mut heap = GcHeap::new(GcOpts::default());
	
	let ref_bitmap = GcTypeLayout::new_bitmap(
		mem::size_of::<MyStructWithRef>(),
		vec![
			field_offset!(MyStructWithRef, a),
			field_offset!(MyStructWithRef, b)
		]
	);
	
	let types = Types {
		id: heap.types().add(GcType::new(mem::size_of::<MyStruct>(), GcTypeLayout::None)),
		ref_id: heap.types().add(GcType::new(mem::size_of::<MyStructWithRef>(), ref_bitmap)),
		callback_id: heap.types().add(GcType::new(mem::size_of::<MyMaybeRef>(), GcTypeLayout::Callback(Box::new(callback_walker))))
	};
	
	(heap, types)
}

fn bench(msg: &str, callback: &Fn()) {
	println!("");
	println!("==> Running {}", msg);
	println!("");
	
	let stopwatch = Stopwatch::new();
	callback();
	
	println!("");
	println!("==> {} took {}", msg, stopwatch.elapsed_ms());
	println!("");
}

fn simple_allocs() {
	/*
	let (heap, types) = create_heap();
	
	let mut a = heap.alloc_handle(types.id, MyStruct {
		a: 1,
		b: 2,
		c: 3
	});
	
	let mut b = heap.alloc_handle(types.ref_id, MyStructWithRef {
		a: heap.alloc(types.id, MyStruct {
			a: 1,
			b: 2,
			c: 3
		}),
		b: heap.alloc(types.id, MyStruct {
			a: 1,
			b: 2,
			c: 3
		})
	});
	
	a.a = 7;
	
	assert_eq!(b.a.c, 3);
//	println!("{} {} {}", b.a.a, b.a.b, b.a.c);
	
	b.a.b = 42;
	
//	println!("{} {} {}", b.a.a, b.a.b, b.a.c);
	
	print_stats(&heap);
	*/
}

fn alloc_struct(heap: &GcHeap, types: &Types, a: i32, b: i32, c: i32) -> GcPtr<MyStruct> {
	let mut result = heap.alloc(types.id);
	
	*result = MyStruct {
		a: a,
		b: b,
		c: c
	};
	
	result
}

fn large_allocs() {
	let (heap, types) = create_heap();
	
	let mut small = Vec::new();
	
	for _ in 0..400000 {
		let mut result = heap.alloc_handle::<MyStructWithRef>(types.ref_id);
		
		result.a = alloc_struct(&heap, &types, 1, 2, 3);
		result.b = alloc_struct(&heap, &types, 4, 5, 6);
		
		small.push(Some(result));
	}
	
//	println!("after init");
//	print_stats(&heap);
	
	heap.gc();
	
//	println!("after init gc");
//	print_stats(&heap);
	/*
	for i in 0..4000 {
		let mut result = heap.alloc_handle::<MyStructWithRef>(types.ref_id);
		
		result.a = alloc_struct(&heap, &types, 1, 2, 3);
		result.b = alloc_struct(&heap, &types, 4, 5, 6);
		
		small[i * 10] = result;
	}
	*/
	for _ in 0..100 {
		for i in 0..100 {
			let mut offset = i;
			let mut inc = 1;
			
			while offset < small.len() {
				let mut result = heap.alloc_handle::<MyStructWithRef>(types.ref_id);
			
				result.a = alloc_struct(&heap, &types, 1, 2, 3);
				result.b = alloc_struct(&heap, &types, 4, 5, 6);
				
				small[offset] = Some(result);
				
				offset += inc;
				inc += 1;
			}
		}
	}
	
//	println!("after replace");
//	print_stats(&heap);

	heap.gc();
	
//	println!("after replace gc");
//	print_stats(&heap);
	
	for i in (0..4000).rev() {
		small[i * 10] = None;
	}
	
//	println!("after remove");
//	print_stats(&heap);

	heap.gc();
	
//	println!("after remove gc");
	print_stats(&heap);
}

fn many_allocs() {
	let (heap, types) = create_heap();
	
	for _ in 0..10 {
		print_stats(&heap);
		
		for _ in 0..400000 {
			let mut result = heap.alloc_handle::<MyStructWithRef>(types.ref_id);
			
			result.a = alloc_struct(&heap, &types, 1, 2, 3);
			result.b = alloc_struct(&heap, &types, 4, 5, 6);
		}
	}
	
	heap.gc();
	
	print_stats(&heap);
}

fn callback_type() {
	let (heap, types) = create_heap();
	
	{
		// Test without reference.
		
		let mut result = heap.alloc_handle(types.callback_id);
		
		*result = MyMaybeRef {
			is_ref: false,
			value: 0
		};
		
		heap.gc();
		
		print_stats(&heap);
	}
	
	{
		// Test with reference.
		
		let mut result = heap.alloc_handle(types.callback_id);
		
		*result = MyMaybeRef {
			is_ref: true,
			value: unsafe { alloc_struct(&heap, &types, 1, 2, 3).usize() }
		};
		
		heap.gc();
		
		print_stats(&heap);
		
		let value : GcPtr<MyStruct> = unsafe { GcPtr::from_usize(result.value) };
		let my_struct = &*value;
		
		assert_eq!(1 + 2 + 3, my_struct.a + my_struct.b + my_struct.c);
	}
}

fn callback_walker(ptr: *const libc::c_void, index: u32) -> GcTypeWalk {
	match index {
		0 => GcTypeWalk::Skip,
		1 => {
			// The boolean at the start indicates whether this is a reference.
			
			let is_ref = unsafe { *mem::transmute::<_, &bool>(ptr) };
			if is_ref { GcTypeWalk::Pointer } else { GcTypeWalk::Skip }
		}
		_ => GcTypeWalk::End
	}
}
