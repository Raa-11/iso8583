//! Proof that parse & pack do not allocate. Separate file because it overrides the global allocator.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

use iso_8583_rs::{Builder, CompiledSpec, Message};

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
}

#[global_allocator]
static A: Counting = Counting;

#[test]
fn parse_and_pack_do_not_allocate() {
    let spec = CompiledSpec::from_file("spec1987.yml").unwrap();
    let mut out = Vec::with_capacity(512);

    let before = ALLOCS.load(Ordering::Relaxed);
    for _ in 0..1000 {
        let mut b = Builder::new(&spec, b"0200").unwrap();
        b.set(2, b"4111111111111111")
            .unwrap()
            .set(4, b"000000010000")
            .unwrap()
            .set(48, b"hello world")
            .unwrap()
            .set(102, b"ACC-1")
            .unwrap();
        b.pack_into(&mut out);
        let m = Message::parse(&spec, &out).unwrap();
        black_box(m.get_u64(4));
        black_box(m.fields().count());
    }
    assert_eq!(ALLOCS.load(Ordering::Relaxed) - before, 0);
}
