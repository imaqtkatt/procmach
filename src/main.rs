use std::{
    cell::{Cell, UnsafeCell},
    collections::{BTreeMap, VecDeque},
    rc::Rc,
    sync::atomic::AtomicUsize,
};

static PID: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Pid(usize);

impl Pid {
    pub fn next() -> Self {
        Self(PID.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }
}

#[derive(Clone, Debug)]
pub enum Value {
    Pid(Pid),
    Int(i32),
    Str(String),
    Tuple(Vec<Value>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    // TODO: do we need this Runnable state?
    Runnable,
    Waiting,
    Running,
    Exiting,
}

#[derive(Clone, Copy, Debug)]
pub enum Instruction {
    DbgStack,
    PushInt(i32),
    PushPid(Pid),
    MakeTuple(u16),
    Add,
    Sub,
    Mul,
    Div,
    This,
    Send,
    Receive,
    Store(u16),
    Load(u16),
    Exit,
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub code: Vec<Instruction>,
    pub locals: Vec<Value>,
}

#[derive(Debug)]
pub struct Process {
    pub pid: Pid,
    pub ip: Cell<usize>,
    pub state: ProcessState,
    pub stack: Vec<Value>,
    pub frames: Vec<Frame>,
    pub mailbox: VecDeque<Value>,
    pub scheduler: Rc<UnsafeCell<Scheduler>>,
}

impl Process {
    pub fn fetch(&mut self) -> Instruction {
        let frame = self.frames.last_mut().expect("not empty frames");
        let ip = self.ip.get_mut();
        let instruction = frame.code[*ip];
        *ip += 1;
        instruction
    }

    fn rewind(&mut self) {
        let ip = self.ip.get_mut();
        debug_assert_ne!(*ip, 0);
        *ip -= 1;
    }

    pub fn step(&mut self) -> ProcessState {
        let instruction = self.fetch();
        println!("{:?} : step {instruction:?}", self.pid);
        match instruction {
            Instruction::DbgStack => {
                println!("{:?}", self.stack);
            }
            Instruction::PushInt(i) => self.stack.push(Value::Int(i)),
            Instruction::PushPid(pid) => self.stack.push(Value::Pid(pid)),
            Instruction::MakeTuple(len) => {
                let values = self.stack.split_off(self.stack.len() - len as usize);
                self.stack.push(Value::Tuple(values));
            }
            Instruction::Add | Instruction::Sub | Instruction::Mul | Instruction::Div => {
                let Value::Int(rhs) = self.stack.pop().expect("rhs") else {
                    panic!("not an integer")
                };
                let Value::Int(lhs) = self.stack.pop().expect("lhs") else {
                    panic!("not an integer")
                };
                let result = match instruction {
                    Instruction::Add => lhs + rhs,
                    Instruction::Sub => lhs - rhs,
                    Instruction::Mul => lhs * rhs,
                    Instruction::Div => lhs / rhs,
                    _ => unreachable!(),
                };
                self.stack.push(Value::Int(result));
            }
            Instruction::This => self.stack.push(Value::Pid(self.pid)),
            Instruction::Send => {
                let message = self.stack.pop().expect("message");
                let Value::Pid(pid) = self.stack.pop().unwrap() else {
                    panic!("expected pid")
                };
                unsafe {
                    let scheduler = self.scheduler.get();
                    let other = (*scheduler)
                        .task_tree
                        .get_mut(&pid)
                        .expect("process exists");
                    let other_pid = other.pid.clone();

                    other.mailbox.push_back(message);
                    if other.state == ProcessState::Waiting {
                        other.state = ProcessState::Runnable;
                        (*scheduler).task_queue.push_back(other_pid);
                    }
                }
            }
            Instruction::Receive => match self.mailbox.pop_front() {
                Some(message) => self.stack.push(message),
                None => {
                    self.rewind();
                    self.state = ProcessState::Waiting;
                }
            },
            Instruction::Store(index) => {
                let frame = self.frames.last_mut().expect("not empty");
                let value = self.stack.pop().expect("value");
                frame.locals[index as usize] = value;
            }
            Instruction::Load(index) => {
                let frame = self.frames.last_mut().expect("not empty");
                let value = frame.locals[index as usize].clone();
                self.stack.push(value);
            }
            Instruction::Exit => self.state = ProcessState::Exiting,
        };
        self.state
    }
}

#[derive(Debug, Default)]
pub struct Scheduler {
    pub task_queue: VecDeque<Pid>,
    pub task_tree: BTreeMap<Pid, Process>,
}

thread_local! {
    // TODO: don't like this UnsafeCell thing, search something better?
    static GLOBAL_SCHEDULER: Rc<UnsafeCell<Scheduler>> = Rc::new(UnsafeCell::new(Scheduler::default()));
}

impl Scheduler {
    pub fn add_process(&mut self, process: Process) {
        self.task_queue.push_back(process.pid);
        self.task_tree.insert(process.pid, process);
    }

    pub fn run(&mut self) {
        while let Some(pid) = self.task_queue.pop_front() {
            let process = self.task_tree.get_mut(&pid).unwrap();

            let mut w = 0u8;
            'run: while w < 10 {
                match process.step() {
                    ProcessState::Runnable => {
                        process.state = ProcessState::Running;
                        w += 2;
                    }
                    ProcessState::Running => w += 2,
                    ProcessState::Waiting | ProcessState::Exiting => break 'run,
                }
            }

            match process.state {
                ProcessState::Runnable => {}
                ProcessState::Waiting => {}
                ProcessState::Running => self.task_queue.push_back(pid),
                ProcessState::Exiting => { /* TODO: implement observers, notify them */ }
            }
        }
    }
}

fn main() {
    let process_main = {
        let main_frame = Frame {
            code: vec![
                Instruction::Receive,
                Instruction::Store(0),
                Instruction::Receive,
                Instruction::Store(1),
                Instruction::Load(0),
                Instruction::Load(1),
                Instruction::Add,
                Instruction::DbgStack,
                Instruction::Exit,
            ],
            locals: vec![Value::Int(69), Value::Int(69)],
        };
        Process {
            pid: Pid::next(),
            ip: Cell::new(0),
            state: ProcessState::Runnable,
            stack: vec![],
            frames: vec![main_frame],
            mailbox: VecDeque::new(),
            scheduler: GLOBAL_SCHEDULER.with(Rc::clone),
        }
    };

    let process_sender = {
        let frame = Frame {
            code: vec![
                Instruction::PushPid(process_main.pid),
                Instruction::PushInt(3),
                Instruction::Send,
                Instruction::PushPid(process_main.pid),
                Instruction::PushInt(3),
                Instruction::Send,
                Instruction::Exit,
            ],
            locals: vec![],
        };
        Process {
            pid: Pid::next(),
            ip: Cell::new(0),
            state: ProcessState::Runnable,
            stack: vec![],
            frames: vec![frame],
            mailbox: VecDeque::new(),
            scheduler: GLOBAL_SCHEDULER.with(Rc::clone),
        }
    };

    let scheduler = GLOBAL_SCHEDULER.with(Rc::clone);

    unsafe {
        let scheduler = scheduler.get();
        (*scheduler).add_process(process_main);
        (*scheduler).add_process(process_sender);
        (*scheduler).run();
    }
}
