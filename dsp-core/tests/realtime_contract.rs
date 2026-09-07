use dsp_core::{beamform::DelayAndSum, DspEngine};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
struct CountingAllocator;
thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ENABLED.try_with(Cell::get).unwrap_or(false) {
            COUNT.with(|n| n.set(n.get() + 1));
        }
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn processing_and_bass_do_not_allocate() {
    let mut e = DspEngine::new(44100.0);
    let mut raw = [0.0; 128];
    let mut monitor = [0.0; 128];
    let mut bass = [0.0; 72];
    COUNT.with(|n| n.set(0));
    ENABLED.with(|v| v.set(true));
    for _ in 0..100 {
        e.process_with_monitor(&mut raw, &mut monitor);
        e.bass_scan(&mut bass);
    }
    ENABLED.with(|v| v.set(false));
    assert_eq!(COUNT.with(Cell::get), 0);
}

#[test]
fn actual_beam_ramp_crosses_ring_boundaries() {
    for fs in [44100.0, 48000.0] {
        let mut beam = DelayAndSum::new(vec![[-4.2, 0.0, 0.0]], 1500.0, fs);
        for n in 0..2000 {
            let value = beam.process_sample(&[n as f32], [1.0, 0.0, 0.0]);
            if n > 300 {
                assert!((value - (n as f32 - 1.0 - 4.2 / 1500.0 * fs)).abs() < 0.001);
            }
        }
    }
}

#[test]
fn ambient_reaches_bass_and_monitor_does_not_change_raw() {
    let mut levels = Vec::new();
    for wind in [0.0, 15.0] {
        let mut e = DspEngine::new(44100.0);
        e.set_targets(&[]);
        e.set_ocean(wind, 0.0);
        e.process(&mut vec![0.0; 44100]);
        let mut scan = [0.0; 72];
        e.bass_scan(&mut scan);
        assert!(scan.iter().all(|x| (*x - scan[0]).abs() < 1e-6));
        levels.push(scan[0]);
    }
    assert!(levels[1] > levels[0] + 10.0);
    let mut a = DspEngine::new(44100.0);
    let mut b = DspEngine::new(44100.0);
    let mut raw = [0.0; 4096];
    let mut expected = [0.0; 4096];
    let mut monitor = [0.0; 4096];
    a.process_with_monitor(&mut raw, &mut monitor);
    b.process(&mut expected);
    assert_eq!(raw, expected);
    assert!(monitor.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
}

#[test]
fn explicit_receiver_depth_changes_measured_surface_target() {
    let target = [45.0, 1000.0, 6.0, 1.0, 16.0, 172.9, 0.0, 1.0, 140.0, 4.0];
    let mut a = DspEngine::new(44100.0);
    let mut b = DspEngine::new(44100.0);
    a.set_world_scene(&[], &target, 0.0);
    b.set_world_scene(&[], &target, 150.0);
    let mut x = [0.0; 4096];
    let mut y = [0.0; 4096];
    a.process(&mut x);
    b.process(&mut y);
    assert_eq!(b.target_count(), 1);
    assert_ne!(x, y);
}
