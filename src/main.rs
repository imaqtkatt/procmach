use core::panic;
use std::{
  cell::{Cell, UnsafeCell},
  collections::{BTreeMap, VecDeque},
  rc::Rc,
  sync::atomic::AtomicUsize,
};

static PID: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Pid(usize);

impl std::fmt::Debug for Pid {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "PID#[{:#06x}]", self.0)
  }
}

impl Pid {
  pub fn next() -> Self {
    Self(PID.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Atom(u32);

const ATOM_EXIT: Atom = Atom(0);

#[derive(Clone, Copy, Debug)]
pub struct HeapHandle {
  index: usize,
}

#[derive(Clone, Copy, Debug)]
pub enum Value {
  Pid(Pid),
  Int(i32),
  Atom(Atom),
  Heap(HeapHandle),
}

#[derive(Clone, Debug)]
pub enum HeapValue {
  Str(String),
  Tuple(Vec<Value>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
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
  DbgDrop,
  PushInt(i32),
  PushPid(Pid),
  MakeTuple(u16),
  GetTuple(u16),
  Add,
  Sub,
  Mul,
  Div,
  This,
  Link,
  Monitor,
  Send,
  Receive,
  Store(u16),
  Load(u16),
  Goto(u16),
  Exit,
}

#[derive(Clone, Debug)]
pub struct Frame {
  pub code: Vec<Instruction>,
  pub locals: Vec<Value>,
}

#[derive(Debug, Default)]
#[repr(transparent)]
pub struct Mailbox {
  queue: VecDeque<Value>,
}

#[derive(Debug, Default)]
pub struct Heap {
  memory: Vec<HeapValue>,
}

impl Heap {
  fn get(&self, handle: HeapHandle) -> &HeapValue {
    &self.memory[handle.index]
  }

  fn allocate_string(&mut self, s: impl Into<String>) -> Value {
    let index = self.memory.len();
    self.memory.push(HeapValue::Str(s.into()));
    Value::Heap(HeapHandle { index })
  }

  fn allocate_tuple(&mut self, values: Vec<Value>) -> Value {
    let index = self.memory.len();
    self.memory.push(HeapValue::Tuple(values));
    Value::Heap(HeapHandle { index })
  }

  fn deep_clone_at(&self, value: Value, other: &mut Heap) -> Value {
    fn deep_clone_value(this_heap: &Heap, value: Value, heap: &mut Heap) -> Value {
      match value {
        Value::Pid(pid) => Value::Pid(pid),
        Value::Int(int) => Value::Int(int),
        Value::Atom(atom) => Value::Atom(atom),
        Value::Heap(heap_handle) => match &this_heap.memory[heap_handle.index] {
          HeapValue::Str(str) => heap.allocate_string(str),
          HeapValue::Tuple(values) => {
            let mut new_values = vec![];
            for value in values.iter().cloned() {
              new_values.push(deep_clone_value(this_heap, value, heap));
            }
            heap.allocate_tuple(new_values)
          }
        },
      }
    }

    deep_clone_value(self, value, other)
  }
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
  pub links: Vec<Pid>,
  pub monitors: Vec<Pid>,
  pub heap: Heap,
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
      Instruction::DbgDrop => {
        let value = self.stack.pop().expect("not empty");
        fn debug_value(
          value: &Value,
          heap: &Heap,
          f: &mut std::fmt::Formatter,
        ) -> std::fmt::Result {
          match value {
            Value::Pid(pid) => write!(f, "Pid({:?})", pid),
            Value::Int(int) => write!(f, "Int({:?})", int),
            Value::Atom(atom) => write!(f, "Atom({:?})", atom),
            Value::Heap(heap_handle) => match heap.get(*heap_handle) {
              HeapValue::Str(str) => write!(f, "{str}"),
              HeapValue::Tuple(values) => {
                write!(f, "{{")?;
                for i in 0..values.len() {
                  if i != 0 {
                    write!(f, ", ")?;
                  }
                  debug_value(&values[i], heap, f)?;
                }
                write!(f, "}}")
              }
            },
          }
        }
        struct Wrapper<'a>(&'a Value, &'a Heap);
        impl std::fmt::Debug for Wrapper<'_> {
          fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            debug_value(self.0, self.1, f)
          }
        }
        println!("{:?}", Wrapper(&value, &self.heap))
      }
      Instruction::PushInt(i) => self.stack.push(Value::Int(i)),
      Instruction::PushPid(pid) => self.stack.push(Value::Pid(pid)),
      Instruction::MakeTuple(len) => {
        let values = self.stack.split_off(self.stack.len() - len as usize);
        let tuple = self.heap.allocate_tuple(values);
        self.stack.push(tuple);
      }
      Instruction::GetTuple(len) => match self.stack.pop() {
        Some(Value::Heap(handle)) => match self.heap.get(handle) {
          HeapValue::Tuple(values) if (len as usize) < values.len() as usize => {
            let value = values[len as usize];
            self.stack.push(value);
          }
          HeapValue::Tuple(_) => panic!("arity error"),
          HeapValue::Str(_) => panic!("expected tuple"),
        },
        None | _ => panic!("expected value"),
      },
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
      Instruction::Link => {
        let Value::Pid(pid) = self.stack.pop().expect("target") else {
          panic!("expected pid");
        };

        unsafe {
          let scheduler = self.scheduler.get();
          let other = (*scheduler).processes.get_mut(&pid).expect("exists");
          if !other.links.contains(&self.pid) {
            other.links.push(self.pid);
          }
        }

        if !self.links.contains(&pid) {
          self.links.push(pid);
        }
      }
      Instruction::Monitor => {
        let Value::Pid(pid) = self.stack.pop().expect("target") else {
          panic!("expected pid");
        };

        unsafe {
          let scheduler = self.scheduler.get();
          let other = (*scheduler).processes.get_mut(&pid).expect("exists");
          if !other.monitors.contains(&self.pid) {
            other.monitors.push(self.pid);
          }
        }
      }
      Instruction::Send => {
        let message = self.stack.pop().expect("message");
        let Value::Pid(pid) = self.stack.pop().unwrap() else {
          panic!("expected pid")
        };
        unsafe {
          let scheduler = self.scheduler.get();
          let other = (*scheduler)
            .processes
            .get_mut(&pid)
            .expect("process exists");
          let other_pid = other.pid.clone();

          let message = self.heap.deep_clone_at(message, &mut other.heap);
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
      Instruction::Goto(index) => {
        self.ip.set(index as usize);
      }
      Instruction::Exit => self.state = ProcessState::Exiting,
    };
    self.state
  }
}

#[derive(Debug, Default)]
pub struct Scheduler {
  pub task_queue: VecDeque<Pid>,
  pub processes: BTreeMap<Pid, Process>,
}

thread_local! {
    // TODO: don't like this UnsafeCell thing, search something better?
    static GLOBAL_SCHEDULER: Rc<UnsafeCell<Scheduler>> = Rc::new(UnsafeCell::new(Scheduler::default()));
}

impl Scheduler {
  pub fn add_process(&mut self, process: Process) {
    self.task_queue.push_back(process.pid);
    self.processes.insert(process.pid, process);
  }

  pub fn run(&mut self) {
    while let Some(pid) = self.task_queue.pop_front() {
      let process = match self.processes.get_mut(&pid) {
        Some(process) => {
          println!("{pid:?} : queued");
          process
        }
        None => {
          eprintln!("{pid:?} : ignored");
          continue;
        }
      };

      let mut w = 0u8;

      while (w < 8) & (process.state != ProcessState::Exiting) {
        match process.step() {
          ProcessState::Runnable => {
            process.state = ProcessState::Running;
            w += 2;
          }
          ProcessState::Running => w += 2,
          ProcessState::Waiting | ProcessState::Exiting => break,
        }
      }

      match process.state {
        ProcessState::Runnable => {}
        ProcessState::Waiting => {}
        ProcessState::Running => self.task_queue.push_back(pid),
        ProcessState::Exiting => {
          let process = self.processes.remove(&pid).unwrap();

          for linked_pid in process.links {
            // TODO: cascading?
            if let Some(link) = self.processes.get_mut(&linked_pid) {
              eprintln!(
                "process {:?} exiting due to linked {:?}",
                link.pid, process.pid
              );
              if link.state != ProcessState::Exiting {
                link.state = ProcessState::Exiting;
                self.task_queue.push_front(linked_pid);
              }
            }
          }

          for monitoring_pid in process.monitors {
            if let Some(monitor) = self.processes.get_mut(&monitoring_pid) {
              let exit = monitor
                .heap
                .allocate_tuple(vec![Value::Atom(ATOM_EXIT), Value::Pid(pid)]);
              monitor.mailbox.push_back(exit);

              if monitor.state == ProcessState::Waiting {
                monitor.state = ProcessState::Runnable;
                self.task_queue.push_front(monitor.pid);
              }
            }
          }
          self.task_queue.retain(|e| *e != pid);
        }
      }
    }
  }
}

fn main() {
  let process_exit = {
    let this_frame = Frame {
      code: vec![
        Instruction::PushInt(0),
        Instruction::PushInt(3),
        Instruction::Sub,
        Instruction::Exit,
      ],
      locals: vec![],
    };
    Process {
      pid: Pid::next(),
      ip: Cell::new(0),
      state: ProcessState::Runnable,
      stack: vec![],
      frames: vec![this_frame],
      mailbox: VecDeque::new(),
      scheduler: GLOBAL_SCHEDULER.with(Rc::clone),
      links: vec![],
      monitors: vec![],
      heap: Heap::default(),
    }
  };

  let process_main = {
    let this_frame = Frame {
      code: vec![
        Instruction::PushPid(process_exit.pid),
        Instruction::Monitor,
        Instruction::Receive,
        Instruction::DbgDrop,
        Instruction::Goto(3),
        Instruction::Exit,
      ],
      locals: vec![],
    };
    Process {
      pid: Pid::next(),
      ip: Cell::new(0),
      state: ProcessState::Runnable,
      stack: vec![],
      frames: vec![this_frame],
      mailbox: VecDeque::new(),
      scheduler: GLOBAL_SCHEDULER.with(Rc::clone),
      links: vec![],
      monitors: vec![],
      heap: Heap::default(),
    }
  };

  let scheduler = GLOBAL_SCHEDULER.with(Rc::clone);

  unsafe {
    let scheduler = scheduler.get();
    (*scheduler).add_process(process_main);
    (*scheduler).add_process(process_exit);
    (*scheduler).run();
  }
}

#[allow(dead_code)]
fn program_adder() {
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
      links: vec![],
      monitors: vec![],
      heap: Heap::default(),
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
      links: vec![],
      monitors: vec![],
      heap: Heap::default(),
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

#[allow(dead_code)]
fn program_link() {
  let process_exit1 = {
    let this_frame = Frame {
      code: vec![Instruction::DbgStack, Instruction::Exit],
      locals: vec![],
    };
    Process {
      pid: Pid::next(),
      ip: Cell::new(0),
      state: ProcessState::Runnable,
      stack: vec![],
      frames: vec![this_frame],
      mailbox: VecDeque::new(),
      scheduler: GLOBAL_SCHEDULER.with(Rc::clone),
      links: vec![],
      monitors: vec![],
      heap: Heap::default(),
    }
  };

  let process_exit2 = {
    let this_frame = Frame {
      code: vec![
        Instruction::PushPid(process_exit1.pid),
        Instruction::Link,
        Instruction::Goto(2),
        Instruction::Exit,
      ],
      locals: vec![],
    };
    Process {
      pid: Pid::next(),
      ip: Cell::new(0),
      state: ProcessState::Runnable,
      stack: vec![],
      frames: vec![this_frame],
      mailbox: VecDeque::new(),
      scheduler: GLOBAL_SCHEDULER.with(Rc::clone),
      links: vec![],
      monitors: vec![],
      heap: Heap::default(),
    }
  };

  let process_main = {
    let main_frame = Frame {
      code: vec![
        Instruction::PushPid(process_exit2.pid),
        Instruction::Link,
        Instruction::Goto(2),
        Instruction::Exit,
      ],
      locals: vec![],
    };
    Process {
      pid: Pid::next(),
      ip: Cell::new(0),
      state: ProcessState::Runnable,
      stack: vec![],
      frames: vec![main_frame],
      mailbox: VecDeque::new(),
      scheduler: GLOBAL_SCHEDULER.with(Rc::clone),
      links: vec![],
      monitors: vec![],
      heap: Heap::default(),
    }
  };

  let scheduler = GLOBAL_SCHEDULER.with(Rc::clone);

  unsafe {
    let scheduler = scheduler.get();
    (*scheduler).add_process(process_main);
    (*scheduler).add_process(process_exit2);
    (*scheduler).add_process(process_exit1);
    (*scheduler).run();
  }
}
