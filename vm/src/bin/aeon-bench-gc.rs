extern crate libaeon;
extern crate time;

use libaeon::config::Config;
use libaeon::virtual_machine::VirtualMachineState;
use libaeon::immix::global_allocator::GlobalAllocator;
use libaeon::immix::permanent_allocator::PermanentAllocator;
use libaeon::process::{Process, RcProcess};
use libaeon::compiled_code::CompiledCode;
use libaeon::gc::thread::Thread as GcThread;
use libaeon::gc::request::{Generation, Request};
use libaeon::execution_context::ExecutionContext;
use libaeon::thread::{Thread as VmThread, RcThread};
use libaeon::object_value;

#[no_mangle]
pub fn measure_gc(gc_thread: GcThread, vm_thread: RcThread, process: RcProcess) {
    for _ in 0..4 {
        let request =
            Request::new(Generation::Young, vm_thread.clone(), process.clone());

        gc_thread.process_request(request);
    }
}

#[no_mangle]
pub fn measure_roots_serial(process: RcProcess) {
    let mut timings = Vec::new();

    for _ in 0..50 {
        let start = time::precise_time_ns();
        let mut pointers = Vec::new();

        for context in process.context().contexts() {
            context.binding.push_pointers(&mut pointers);
            context.register.push_pointers(&mut pointers);
        }

        let duration = (time::precise_time_ns() - start) as f64;

        timings.push(duration);
    }

    println!("Serial average: {:.2} ms",
             (timings.iter().sum::<f64>() / timings.len() as f64) / 1000000.0);
}

#[no_mangle]
pub fn measure_roots_parallel(process: RcProcess) {
    let mut timings = Vec::new();

    for _ in 0..50 {
        let start = time::precise_time_ns();

        process.roots();

        let duration = (time::precise_time_ns() - start) as f64;

        timings.push(duration);
    }

    println!("Parallel average: {:.2} ms",
             (timings.iter().sum::<f64>() / timings.len() as f64) / 1000000.0);
}

fn main() {
    let global_alloc = GlobalAllocator::new();
    let mut perm_alloc = PermanentAllocator::new(global_alloc.clone());
    let self_obj = perm_alloc.allocate_empty();

    let code =
        CompiledCode::with_rc("a".to_string(), "a".to_string(), 1, Vec::new());

    let process = Process::from_code(1, code.clone(), self_obj, global_alloc);
    let vm_state = VirtualMachineState::new(Config::new());
    let gc_thread = GcThread::new(vm_state);
    let vm_thread = VmThread::new(false, None);

    let self_obj = process.self_object();

    for _ in 0..500 {
        for index in 0..5000 {
            let ptr1 =
                process.allocate_without_prototype(object_value::string("Hello"
                    .to_string()));
            let ptr2 =
                process.allocate_without_prototype(object_value::string("World"
                    .to_string()));

            process.set_local(index, ptr1);
            process.set_register(index, ptr2);
        }

        let context = ExecutionContext::with_object(self_obj, code.clone(), None);

        process.push_context(context);
    }

    measure_roots_serial(process.clone());
    measure_roots_parallel(process);
}
