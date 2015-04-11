extern crate gc;

use gc::*;
use std::mem;

struct MyStruct {
	a: i32,
	b: i32,
	c: i32
}

struct MyStructWithRef {
	a: GcPtr<MyStruct>
}

fn main() {
	let mut heap = GcHeap::new(GcOpts::default());
	
	let struct_id = heap.types().add(GcType::new(mem::size_of::<MyStruct>(), 0, GcTypeLayout::None));
	let with_ref_id = heap.types().add(GcType::new(mem::size_of::<MyStructWithRef>(), 0, GcTypeLayout::None));
	
	let mut a = heap.alloc_handle(struct_id, MyStruct {
		a: 1,
		b: 2,
		c: 3
	});
	
	let mut b = heap.alloc_handle(with_ref_id, MyStructWithRef {
		a: heap.alloc(struct_id, MyStruct {
			a: 1,
			b: 2,
			c: 3
		})
	});
	
	a.a = 7;
	
	assert_eq!(b.a.c, 3);
	println!("hi {} {} {}", b.a.a, b.a.b, b.a.c);
	
	b.a.b = 42;
	
	println!("hi {} {} {}", b.a.a, b.a.b, b.a.c);
}
