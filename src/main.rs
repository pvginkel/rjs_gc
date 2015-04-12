#![allow(dead_code)]

#[macro_use]
extern crate gc;
extern crate time;

use gc::*;
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

struct MyStructWithRef {
	a: GcPtr<MyStruct>,
	b: GcPtr<MyStruct>
}

fn print_stats(heap: &GcHeap) { 
	println!("STATS: allocated {}, used {}", heap.mem_allocated(), heap.mem_used());
}

struct Types {
	id: GcTypeId,
	ref_id: GcTypeId
}

fn main() {
	bench("Simple allocs", &|| { simple_allocs() });
	bench("Large allocs", &|| { large_allocs() });
	bench("Many allocs", &|| { many_allocs() });
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
		id: heap.types().add(GcType::new(mem::size_of::<MyStruct>(), 0, GcTypeLayout::None)),
		ref_id: heap.types().add(GcType::new(mem::size_of::<MyStructWithRef>(), 0, ref_bitmap))
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
}

fn alloc_struct(heap: &GcHeap, types: &Types, a: i32, b: i32, c: i32) -> GcPtr<MyStruct> {
	heap.alloc(types.id, MyStruct {
		a: a,
		b: b,
		c: c
	})
}

fn large_allocs() {
	let (heap, types) = create_heap();
	
	let mut small = Vec::new();
	
	for _ in 0..400000 {
		small.push(heap.alloc_handle(types.ref_id, MyStructWithRef {
			a: alloc_struct(&heap, &types, 1, 2, 3),
			b: alloc_struct(&heap, &types, 4, 5, 6)
		}));
	}
	
//	println!("after init");
//	print_stats(&heap);
	
	heap.gc();
	
//	println!("after init gc");
//	print_stats(&heap);
	
	for i in 0..4000 {
		small[i * 10] = heap.alloc_handle(types.ref_id, MyStructWithRef {
			a: alloc_struct(&heap, &types, 1, 2, 3),
			b: alloc_struct(&heap, &types, 4, 5, 6)
		});
	}
	
//	println!("after replace");
//	print_stats(&heap);
	
	heap.gc();
	
//	println!("after replace gc");
//	print_stats(&heap);
	
	for i in (0..4000).rev() {
		small.remove(i * 10);
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
			heap.alloc_handle(types.ref_id, MyStructWithRef {
				a: alloc_struct(&heap, &types, 1, 2, 3),
				b: alloc_struct(&heap, &types, 4, 5, 6)
			});
		}
	}
	
	heap.gc();
	
	print_stats(&heap);
}
