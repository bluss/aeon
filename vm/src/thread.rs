//! Virtual Machine Threads

use std::sync::{Arc, Mutex, Condvar};
use std::thread;

use process::RcProcess;

pub type RcThread = Arc<Thread>;
pub type JoinHandle = thread::JoinHandle<()>;

pub struct Thread {
    pub process_queue: Mutex<Vec<RcProcess>>,
    pub wake_up: Mutex<bool>,
    pub wakeup_signaler: Condvar,
    pub should_stop: Mutex<bool>,
    pub join_handle: Mutex<Option<JoinHandle>>,
    pub isolated: Mutex<bool>
}

impl Thread {
    pub fn new(handle: Option<JoinHandle>) -> RcThread {
        let thread = Thread {
            process_queue: Mutex::new(Vec::new()),
            wake_up: Mutex::new(false),
            wakeup_signaler: Condvar::new(),
            should_stop: Mutex::new(false),
            join_handle: Mutex::new(handle),
            isolated: Mutex::new(false)
        };

        Arc::new(thread)
    }

    pub fn isolated(handle: Option<JoinHandle>) -> RcThread {
        let thread = Thread::new(handle);

        *thread.isolated.lock().unwrap() = true;

        thread
    }

    pub fn stop(&self) {
        let mut stop = self.should_stop.lock().unwrap();
        let mut wake_up = self.wake_up.lock().unwrap();

        *stop = true;
        *wake_up = true;

        self.wakeup_signaler.notify_all();
    }

    pub fn take_join_handle(&self) -> Option<JoinHandle> {
        self.join_handle.lock().unwrap().take()
    }

    pub fn should_stop(&self) -> bool {
        *self.should_stop.lock().unwrap()
    }

    pub fn is_isolated(&self) -> bool {
        *self.isolated.lock().unwrap()
    }

    pub fn process_queue_size(&self) -> usize {
        self.process_queue.lock().unwrap().len()
    }

    pub fn schedule(&self, task: RcProcess) {
        let mut queue = self.process_queue.lock().unwrap();
        let mut wake_up = self.wake_up.lock().unwrap();

        queue.push(task);
        *wake_up = true;

        self.wakeup_signaler.notify_all();
    }

    pub fn wait_for_work(&self) {
        if self.should_stop() {
            return;
        }

        let empty = self.process_queue_size() == 0;

        if empty {
            let mut wake_up = self.wake_up.lock().unwrap();

            while !*wake_up {
                wake_up = self.wakeup_signaler.wait(wake_up).unwrap();
            }
        }
    }

    pub fn pop_process(&self) -> RcProcess {
        let mut queue = self.process_queue.lock().unwrap();
        let mut wake_up = self.wake_up.lock().unwrap();

        *wake_up = false;

        queue.pop().unwrap()
    }
}
