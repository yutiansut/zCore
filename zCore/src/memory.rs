//! Define the FrameAllocator for physical memory
//! x86_64      --  64GB

use {
    bitmap_allocator::BitAlloc,
    buddy_system_allocator::{Heap, LockedHeapWithRescue},
    rboot::{BootInfo, MemoryType},
    spin::Mutex,
    x86_64::structures::paging::page_table::{PageTable, PageTableFlags as EF},
};

#[cfg(target_arch = "x86_64")]
type FrameAlloc = bitmap_allocator::BitAlloc16M;

static FRAME_ALLOCATOR: Mutex<FrameAlloc> = Mutex::new(FrameAlloc::DEFAULT);

const MEMORY_OFFSET: usize = 0;
const KERNEL_OFFSET: usize = 0xffffff00_00000000;
const PHYSICAL_MEMORY_OFFSET: usize = 0xffff8000_00000000;
const KERNEL_HEAP_SIZE: usize = 8 * 1024 * 1024; // 8 MB

const KERNEL_PM4: usize = (KERNEL_OFFSET >> 39) & 0o777;
const PHYSICAL_MEMORY_PM4: usize = (PHYSICAL_MEMORY_OFFSET >> 39) & 0o777;

const PAGE_SIZE: usize = 1 << 12;

#[used]
#[export_name = "hal_pmem_base"]
static PMEM_BASE: usize = PHYSICAL_MEMORY_OFFSET;

pub fn init_frame_allocator(boot_info: &BootInfo) {
    let mut ba = FRAME_ALLOCATOR.lock();
    for region in boot_info.memory_map.clone().iter {
        if region.ty == MemoryType::CONVENTIONAL {
            let start_frame = region.phys_start as usize / PAGE_SIZE;
            let end_frame = start_frame + region.page_count as usize;
            ba.insert(start_frame..end_frame);
        }
    }
    info!("Frame allocator init end");
}

pub fn init_heap() {
    const MACHINE_ALIGN: usize = core::mem::size_of::<usize>();
    const HEAP_BLOCK: usize = KERNEL_HEAP_SIZE / MACHINE_ALIGN;
    static mut HEAP: [usize; HEAP_BLOCK] = [0; HEAP_BLOCK];
    unsafe {
        HEAP_ALLOCATOR
            .lock()
            .init(HEAP.as_ptr() as usize, HEAP_BLOCK * MACHINE_ALIGN);
    }
    info!("heap init end");
}

#[no_mangle]
pub extern "C" fn hal_frame_alloc() -> Option<usize> {
    // get the real address of the alloc frame
    let ret = FRAME_ALLOCATOR
        .lock()
        .alloc()
        .map(|id| id * PAGE_SIZE + MEMORY_OFFSET);
    trace!("Allocate frame: {:x?}", ret);
    ret
}

#[no_mangle]
pub extern "C" fn hal_frame_dealloc(target: &usize) {
    trace!("Deallocate frame: {:x}", *target);
    FRAME_ALLOCATOR
        .lock()
        .dealloc((*target - MEMORY_OFFSET) / PAGE_SIZE);
}

#[no_mangle]
pub extern "C" fn hal_pt_map_kernel(pt: &mut PageTable, current: &PageTable) {
    let ekernel = current[KERNEL_PM4].clone();
    let ephysical = current[PHYSICAL_MEMORY_PM4].clone();
    pt[KERNEL_PM4].set_addr(ekernel.addr(), ekernel.flags() | EF::GLOBAL);
    pt[PHYSICAL_MEMORY_PM4].set_addr(ephysical.addr(), ephysical.flags() | EF::GLOBAL);
}

fn enlarge_heap(heap: &mut Heap) {
    error!("Enlarging heap to avoid oom");

    let mut addrs = [(0, 0); 32];
    let mut addr_len = 0;
    let va_offset = PMEM_BASE;
    for _ in 0..16384 {
        let page = hal_frame_alloc().unwrap();
        let va = va_offset + page;
        if addr_len > 0 {
            let (ref mut addr, ref mut len) = addrs[addr_len - 1];
            if *addr - PAGE_SIZE == va {
                *len += PAGE_SIZE;
                *addr -= PAGE_SIZE;
                continue;
            }
        }
        addrs[addr_len] = (va, PAGE_SIZE);
        addr_len += 1;
    }
    for (addr, len) in addrs[..addr_len].iter() {
        info!("Adding {:#X} {:#X} to heap", addr, len);
        unsafe {
            heap.init(*addr, *len);
        }
    }
}

/// Global heap allocator
///
/// Available after `memory::init_heap()`.
#[global_allocator]
static HEAP_ALLOCATOR: LockedHeapWithRescue = LockedHeapWithRescue::new(enlarge_heap);
