use std::collections::HashSet;
use std::mem;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, Condvar};
use std::cell::UnsafeCell;

use immix::bucket::Bucket;
use immix::copy_object::CopyObject;
use immix::local_allocator::LocalAllocator;
use immix::global_allocator::RcGlobalAllocator;
use immix::mailbox_allocator::MailboxAllocator;

use binding::RcBinding;
use call_frame::CallFrame;
use compiled_code::RcCompiledCode;
use object_pointer::{ObjectPointer, ObjectPointerPointer};
use object_value;
use execution_context::ExecutionContext;
use queue::Queue;

pub type RcProcess = Arc<Process>;

use std::thread;
use crossbeam::sync::chase_lev::{deque, Steal};
pub struct SyncPointer<T> {
    pub raw: *const T,
}

unsafe impl<T> Send for SyncPointer<T> {}
unsafe impl<T> Sync for SyncPointer<T> {}

#[derive(Debug)]
pub enum ProcessStatus {
    /// The process has been scheduled for execution.
    Scheduled,

    /// The process is running.
    Running,

    /// The process has been suspended.
    Suspended,

    /// The process has been suspended by the garbage collector.
    SuspendedByGc,

    /// The process ran into some kind of error during execution.
    Failed,

    /// The process has finished execution.
    Finished,
}

impl ProcessStatus {
    pub fn is_running(&self) -> bool {
        match *self {
            ProcessStatus::Running => true,
            _ => false,
        }
    }
}

pub enum GcState {
    /// No collector activity is taking place.
    None,

    /// A collection has been scheduled.
    Scheduled,
}

pub struct LocalData {
    /// The process-local memory allocator.
    pub allocator: LocalAllocator,

    /// The current call frame of this process.
    pub call_frame: CallFrame, // TODO: use Box<CallFrame>

    /// The current execution context of this process.
    pub context: Box<ExecutionContext>,

    /// The state of the garbage collector for this process.
    pub gc_state: GcState,

    /// When set to "true" this process should suspend itself so it can be
    /// garbage collected.
    pub suspend_for_gc: bool,

    /// The remembered set of this process. This set is not synchronized via a
    /// lock of sorts. As such the collector must ensure this process is
    /// suspended upon examining the remembered set.
    pub remembered_set: HashSet<ObjectPointer>,
}

pub struct Process {
    /// The process identifier of this process.
    pub pid: usize,

    /// The status of this process.
    pub status: Mutex<ProcessStatus>,

    /// Condition variable used for waking up other threads waiting for this
    /// process' status to change.
    pub status_signaler: Condvar,

    /// A queue containing received messages.
    pub mailbox: Queue<ObjectPointer>,

    /// The allocator to use for storing objects in the mailbox heap.
    pub mailbox_allocator: Mutex<MailboxAllocator>,

    /// Data stored in a process that should only be modified by a single thread
    /// at once.
    pub local_data: UnsafeCell<LocalData>,
}

unsafe impl Sync for LocalData {}
unsafe impl Send for LocalData {}
unsafe impl Sync for Process {}

impl Process {
    pub fn new(pid: usize,
               call_frame: CallFrame,
               context: ExecutionContext,
               global_allocator: RcGlobalAllocator)
               -> RcProcess {
        let local_data = LocalData {
            allocator: LocalAllocator::new(global_allocator.clone()),
            call_frame: call_frame,
            context: Box::new(context),
            gc_state: GcState::None,
            suspend_for_gc: false,
            remembered_set: HashSet::new(),
        };

        let process = Process {
            pid: pid,
            status: Mutex::new(ProcessStatus::Scheduled),
            status_signaler: Condvar::new(),
            mailbox: Queue::new(),
            mailbox_allocator:
                Mutex::new(MailboxAllocator::new(global_allocator)),
            local_data: UnsafeCell::new(local_data),
        };

        Arc::new(process)
    }

    pub fn from_code(pid: usize,
                     code: RcCompiledCode,
                     self_obj: ObjectPointer,
                     global_allocator: RcGlobalAllocator)
                     -> RcProcess {
        let frame = CallFrame::from_code(code.clone());
        let context = ExecutionContext::with_object(self_obj, code, None);

        Process::new(pid, frame, context, global_allocator)
    }

    pub fn local_data_mut(&self) -> &mut LocalData {
        unsafe { &mut *self.local_data.get() }
    }

    pub fn local_data(&self) -> &LocalData {
        unsafe { &*self.local_data.get() }
    }

    pub fn push_call_frame(&self, mut frame: CallFrame) {
        let mut local_data = self.local_data_mut();
        let ref mut target = local_data.call_frame;

        mem::swap(target, &mut frame);

        target.set_parent(frame);
    }

    pub fn pop_call_frame(&self) {
        let mut local_data = self.local_data_mut();

        if local_data.call_frame.parent.is_none() {
            return;
        }

        let parent = local_data.call_frame.parent.take().unwrap();

        local_data.call_frame = *parent;
    }

    pub fn push_context(&self, context: ExecutionContext) {
        let mut boxed = Box::new(context);
        let mut local_data = self.local_data_mut();
        let ref mut target = local_data.context;

        mem::swap(target, &mut boxed);

        target.set_parent(boxed);
    }

    pub fn pop_context(&self) {
        let mut local_data = self.local_data_mut();

        if local_data.context.parent.is_none() {
            return;
        }

        let parent = local_data.context.parent.take().unwrap();

        local_data.context = parent;
    }

    pub fn get_register(&self, register: usize) -> Result<ObjectPointer, String> {
        self.local_data()
            .context
            .get_register(register)
            .ok_or_else(|| format!("Undefined object in register {}", register))
    }

    pub fn get_register_option(&self, register: usize) -> Option<ObjectPointer> {
        self.local_data().context.get_register(register)
    }

    pub fn set_register(&self, register: usize, value: ObjectPointer) {
        self.local_data_mut().context.set_register(register, value);
    }

    pub fn set_local(&self, index: usize, value: ObjectPointer) {
        self.local_data_mut().context.set_local(index, value);
    }

    pub fn get_local(&self, index: usize) -> Result<ObjectPointer, String> {
        self.local_data().context.get_local(index)
    }

    pub fn local_exists(&self, index: usize) -> bool {
        let local_data = self.local_data();

        local_data.context.binding.local_exists(index)
    }

    pub fn allocate_empty(&self) -> ObjectPointer {
        self.local_data_mut().allocator.allocate_empty()
    }

    pub fn allocate(&self,
                    value: object_value::ObjectValue,
                    proto: ObjectPointer)
                    -> ObjectPointer {
        let mut local_data = self.local_data_mut();

        local_data.allocator.allocate_with_prototype(value, proto)
    }

    pub fn allocate_without_prototype(&self,
                                      value: object_value::ObjectValue)
                                      -> ObjectPointer {
        let mut local_data = self.local_data_mut();

        local_data.allocator.allocate_without_prototype(value)
    }

    /// Sends a message to the current process.
    pub fn send_message(&self, message: ObjectPointer) {
        let mut to_send = message;

        // TODO: Instead of using is_local we can use an enum with two variants:
        // Remote and Local. A Remote message requires copying the message into
        // the message allocator, a Local message can be used as-is.
        //
        // When we receive() a Remote message we copy it to the eden allocator. If
        // the message is a Local message we just leave things as-is.
        //
        // This can also be used for globals as when sending a global object as
        // a message we can just use the Local variant.
        if to_send.is_local() {
            to_send = unlock!(self.mailbox_allocator).copy_object(to_send);
        }

        self.mailbox.push(to_send);
    }

    /// Pops a message from the current process' message queue.
    pub fn receive_message(&self) -> Option<ObjectPointer> {
        // TODO: copy to the heap
        self.mailbox.pop_nonblock()
    }

    pub fn should_be_rescheduled(&self) -> bool {
        match *unlock!(self.status) {
            ProcessStatus::Suspended => true,
            _ => false,
        }
    }

    /// Adds a new call frame pointing to the given line number.
    pub fn advance_line(&self, line: u32) {
        let frame = CallFrame::new(self.compiled_code(), line);

        self.push_call_frame(frame);
    }

    pub fn binding(&self) -> RcBinding {
        self.context().binding()
    }

    pub fn self_object(&self) -> ObjectPointer {
        self.context().self_object()
    }

    pub fn context(&self) -> &Box<ExecutionContext> {
        &self.local_data().context
    }

    pub fn context_mut(&self) -> &mut Box<ExecutionContext> {
        &mut self.local_data_mut().context
    }

    pub fn at_top_level(&self) -> bool {
        self.context().parent.is_none()
    }

    pub fn call_frame(&self) -> &CallFrame {
        &self.local_data().call_frame
    }

    pub fn compiled_code(&self) -> RcCompiledCode {
        self.context().code.clone()
    }

    pub fn instruction_index(&self) -> usize {
        self.context().instruction_index
    }

    pub fn set_instruction_index(&self, index: usize) {
        self.context_mut().instruction_index = index;
    }

    pub fn is_alive(&self) -> bool {
        match *unlock!(self.status) {
            ProcessStatus::Failed => false,
            ProcessStatus::Finished => false,
            _ => true,
        }
    }

    pub fn available_for_execution(&self) -> bool {
        match *unlock!(self.status) {
            ProcessStatus::Scheduled => true,
            ProcessStatus::Suspended => true,
            _ => false,
        }
    }

    pub fn running(&self) {
        self.set_status(ProcessStatus::Running);
    }

    pub fn set_status(&self, new_status: ProcessStatus) {
        let mut status = unlock!(self.status);

        *status = new_status;

        self.status_signaler.notify_all();
    }

    pub fn set_status_without_overwriting_gc_status(&self,
                                                    new_status: ProcessStatus) {
        let mut status = unlock!(self.status);

        let overwrite = match *status {
            ProcessStatus::SuspendedByGc => false,
            _ => true,
        };

        // Don't overwrite the process status if it was suspended by the GC.
        if overwrite {
            let mut local_data = self.local_data_mut();

            if local_data.suspend_for_gc {
                local_data.suspend_for_gc = false;
                *status = ProcessStatus::SuspendedByGc;
            } else {
                *status = new_status;
            }

            self.status_signaler.notify_all();
        }
    }

    pub fn finished(&self) {
        self.set_status_without_overwriting_gc_status(ProcessStatus::Finished);
    }

    pub fn suspend(&self) {
        self.set_status_without_overwriting_gc_status(ProcessStatus::Suspended);
    }

    pub fn suspend_for_gc(&self) {
        self.local_data_mut().suspend_for_gc = false;
        self.set_status(ProcessStatus::SuspendedByGc);
    }

    pub fn suspended_by_gc(&self) -> bool {
        match *unlock!(self.status) {
            ProcessStatus::SuspendedByGc => true,
            _ => false,
        }
    }

    pub fn request_gc_suspension(&self) {
        if !self.suspended_by_gc() {
            self.local_data_mut().suspend_for_gc = true;
        }

        self.wait_while_running();
    }

    pub fn wait_while_running(&self) {
        let mut status = unlock!(self.status);

        while status.is_running() {
            status = self.status_signaler.wait(status).unwrap();
        }
    }

    pub fn should_suspend_for_gc(&self) -> bool {
        self.suspended_by_gc() || self.local_data().suspend_for_gc
    }

    pub fn gc_state(&self) -> &GcState {
        &self.local_data().gc_state
    }

    pub fn set_gc_state(&self, new_state: GcState) {
        self.local_data_mut().gc_state = new_state;
    }

    pub fn gc_scheduled(&self) {
        self.set_gc_state(GcState::Scheduled);
    }

    pub fn should_schedule_gc(&self) -> bool {
        match *self.gc_state() {
            GcState::None => self.should_collect_young_generation(),
            _ => false,
        }
    }

    pub fn should_collect_young_generation(&self) -> bool {
        self.local_data()
            .allocator
            .young_block_allocation_threshold_exceeded()
    }

    pub fn should_collect_mature_generation(&self) -> bool {
        self.local_data()
            .allocator
            .mature_block_allocation_threshold_exceeded()
    }

    pub fn reset_status(&self) {
        self.set_status(ProcessStatus::Scheduled);
        self.set_gc_state(GcState::None);
    }

    /// Scans all the root objects and returns a list containing the objects to
    /// scan for references to other objects.
    ///
    /// This method returns a vector of raw pointers to object pointers. Care
    /// must be taken to ensure that the raw pointers do not outlive the
    /// underlying object pointers.
    pub fn roots(&self) -> Vec<ObjectPointerPointer> {
        let thread_count = 3;
        let mut threads = Vec::with_capacity(thread_count);
        let mut pointers = Vec::new();
        let (mut worker, stealer) = deque();

        for context in self.context().contexts() {
            worker.push(SyncPointer { raw: context as *const ExecutionContext });
        }

        for _ in 0..thread_count {
            let t_stealer = stealer.clone();

            threads.push(thread::spawn(move || {
                let mut pointers = Vec::new();

                loop {
                    match t_stealer.steal() {
                        Steal::Data(ctx) => {
                            let context = unsafe { &*ctx.raw };

                            context.binding.push_pointers(&mut pointers);
                            context.register.push_pointers(&mut pointers);
                        }
                        Steal::Empty => break,
                        _ => {}
                    }
                }

                pointers
            }));
        }

        while let Some(ctx) = worker.try_pop() {
            let context = unsafe { &*ctx.raw };

            context.binding.push_pointers(&mut pointers);
            context.register.push_pointers(&mut pointers);
        }

        for thread in threads {
            pointers.append(&mut thread.join().unwrap());
        }

        pointers
    }

    pub fn remembered_set_mut(&self) -> &mut HashSet<ObjectPointer> {
        &mut self.local_data_mut().remembered_set
    }

    /// Write barrier for tracking cross generation writes.
    ///
    /// This barrier is based on the Steele write barrier and tracks the object
    /// that is *written to*, not the object that is being written.
    pub fn write_barrier(&self,
                         written_to: ObjectPointer,
                         written: ObjectPointer) {
        if written_to.is_mature() && written.is_young() {
            self.remembered_set_mut().insert(written_to);
        }
    }

    pub fn increment_young_ages(&self) {
        self.local_data_mut().allocator.increment_young_ages()
    }

    pub fn mature_generation_mut(&self) -> &mut Bucket {
        self.local_data_mut().allocator.mature_generation_mut()
    }
}

impl PartialEq for Process {
    fn eq(&self, other: &Process) -> bool {
        self.pid == other.pid
    }
}

impl Eq for Process {}

impl Hash for Process {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pid.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use immix::global_allocator::GlobalAllocator;
    use compiled_code::CompiledCode;
    use object::Object;
    use object_pointer::ObjectPointer;

    fn new_process() -> RcProcess {
        let code = CompiledCode::with_rc("a".to_string(),
                                         "a".to_string(),
                                         1,
                                         Vec::new());

        let self_obj = ObjectPointer::null();

        Process::from_code(1, code, self_obj, GlobalAllocator::new())
    }

    #[test]
    fn test_roots() {
        let process = new_process();
        let pointer = process.allocate_empty();

        process.set_local(0, pointer);
        process.set_register(0, pointer);

        assert_eq!(process.roots().len(), 3);
    }

    #[test]
    fn test_roots_doesnt_copy_pointers() {
        let process = new_process();
        let pointer = process.allocate_empty();

        process.set_local(0, pointer);
        process.set_register(0, pointer);

        for pointer_pointer in process.roots() {
            let pointer_ref = pointer_pointer.get_mut();

            pointer_ref.raw.raw = 0x4 as *mut Object;
        }

        assert_eq!(process.get_local(0).unwrap().raw.raw as usize, 0x4);
        assert_eq!(process.get_register(0).unwrap().raw.raw as usize, 0x4);
        assert_eq!(process.self_object().raw.raw as usize, 0x4);
    }
}
